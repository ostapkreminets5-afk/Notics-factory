use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use url::Url;

pub const API_HOST: &str = "127.0.0.1";
pub const DEFAULT_LIMIT: usize = 25;
pub const MAX_LIMIT: usize = 100;
pub const MAX_QUERY_CHARS: usize = 200;

const LOCAL_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "[::1]"];
const MAX_REQUEST_BYTES: usize = 8 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONNECTIONS: usize = 16;
const HEAD_END: &[u8] = b"\r\n\r\n";
const LIMIT_PROBLEM: &str = "limit має бути числом від 1 до 100.";
const QUERY_TOO_LONG: &str = "Запит задовгий — максимум 200 символів.";

#[derive(Debug, PartialEq, Eq)]
struct Request {
    method: String,
    target: String,
    host: Option<String>,
    origin: Option<String>,
}

pub struct Reply {
    pub status: u16,
    pub body: Value,
}

impl Reply {
    pub fn new(status: u16, body: Value) -> Self {
        Self { status, body }
    }

    pub fn error(status: u16, code: &str, message: &str) -> Self {
        Self::new(status, json!({ "error": code, "message": message }))
    }

    pub fn not_found() -> Self {
        Self::error(404, "not_found", "Такого шляху немає. Відкрий / щоб побачити довідку.")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Search {
    pub query: String,
    pub limit: usize,
}

pub fn address(port: u16) -> String {
    format!("http://{API_HOST}:{port}")
}

pub fn serve<F>(port: u16, answer: F)
where
    F: Fn(&Url) -> Reply + Send + Sync + 'static,
{
    let answer = Arc::new(answer);
    let open = Arc::new(AtomicUsize::new(0));
    thread::spawn(move || {
        let Ok(listener) = TcpListener::bind((API_HOST, port)) else {
            return;
        };
        for mut stream in listener.incoming().flatten() {
            if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                open.fetch_sub(1, Ordering::SeqCst);
                let _ = write_reply(&mut stream, &Reply::error(503, "busy", "Забагато одночасних запитів."), None);
                continue;
            }
            let answer = Arc::clone(&answer);
            let open = Arc::clone(&open);
            thread::spawn(move || {
                let _ = handle(stream, answer.as_ref());
                open.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
}

pub fn search_terms(address: &Url, missing: &'static str) -> Result<Search, &'static str> {
    let query = parameter(address, "q")
        .map(|raw| raw.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    if query.is_empty() {
        return Err(missing);
    }
    if query.chars().count() > MAX_QUERY_CHARS {
        return Err(QUERY_TOO_LONG);
    }
    let limit = match parameter(address, "limit") {
        None => DEFAULT_LIMIT,
        Some(raw) => match raw.trim().parse::<usize>() {
            Ok(value) if (1..=MAX_LIMIT).contains(&value) => value,
            _ => return Err(LIMIT_PROBLEM),
        },
    };
    Ok(Search { query, limit })
}

fn parameter(address: &Url, name: &str) -> Option<String> {
    address
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

fn handle(mut stream: TcpStream, answer: &dyn Fn(&Url) -> Reply) -> std::io::Result<()> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    let Some(request) = read_head(&mut stream)?.as_deref().and_then(parse_request) else {
        return write_reply(&mut stream, &Reply::error(400, "bad_request", "Некоректний HTTP-запит."), None);
    };
    if !request.host.as_deref().is_some_and(is_local_host) {
        return write_reply(&mut stream, &Reply::error(403, "forbidden_host", "API приймає лише локальні запити."), None);
    }
    let allowed_origin = match request.origin.as_deref() {
        None => None,
        Some(origin) if is_local_origin(origin) => Some(origin.to_owned()),
        Some(_) => {
            return write_reply(
                &mut stream,
                &Reply::error(403, "forbidden_origin", "API відповідає лише сторінкам з localhost."),
                None,
            );
        }
    };
    let reply = respond(&request.method, &request.target, answer);
    write_reply(&mut stream, &reply, allowed_origin.as_deref())
}

fn respond(method: &str, target: &str, answer: &dyn Fn(&Url) -> Reply) -> Reply {
    match method {
        "GET" => match Url::parse(&format!("http://{API_HOST}{target}")) {
            Ok(address) => answer(&address),
            Err(_) => Reply::not_found(),
        },
        "OPTIONS" => Reply::new(204, Value::Null),
        _ => Reply::error(405, "method_not_allowed", "Підтримується лише GET."),
    }
}

fn read_head(stream: &mut TcpStream) -> std::io::Result<Option<String>> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !buffer.windows(HEAD_END.len()).any(|window| window == HEAD_END) {
        if buffer.len() > MAX_REQUEST_BYTES {
            return Ok(None);
        }
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    Ok(String::from_utf8(buffer).ok())
}

fn parse_request(head: &str) -> Option<Request> {
    let mut lines = head.split("\r\n");
    let mut start = lines.next()?.split(' ');
    let method = start.next()?.to_owned();
    let target = start.next()?.to_owned();
    if !start.next()?.starts_with("HTTP/") {
        return None;
    }
    let mut host = None;
    let mut origin = None;
    for line in lines.take_while(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "host" => host = Some(value.trim().to_owned()),
            "origin" => origin = Some(value.trim().to_owned()),
            _ => {}
        }
    }
    Some(Request {
        method,
        target,
        host,
        origin,
    })
}

fn is_local_host(host: &str) -> bool {
    let name = host
        .rsplit_once(':')
        .filter(|(_, port)| port.bytes().all(|byte| byte.is_ascii_digit()) && !port.is_empty())
        .map_or(host, |(name, _)| name);
    LOCAL_HOSTS.contains(&name.to_ascii_lowercase().as_str())
}

fn is_local_origin(origin: &str) -> bool {
    Url::parse(origin).is_ok_and(|address| {
        matches!(address.scheme(), "http" | "https")
            && address
                .host_str()
                .is_some_and(|host| LOCAL_HOSTS.contains(&host.to_ascii_lowercase().as_str()))
    })
}

fn write_reply(stream: &mut TcpStream, reply: &Reply, origin: Option<&str>) -> std::io::Result<()> {
    stream.write_all(&encode_reply(reply, origin))?;
    stream.flush()
}

fn encode_reply(reply: &Reply, origin: Option<&str>) -> Vec<u8> {
    let body = if reply.body.is_null() {
        String::new()
    } else {
        reply.body.to_string()
    };
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n",
        reply.status,
        reason(reply.status),
        body.len()
    );
    if let Some(origin) = origin {
        head.push_str(&format!(
            "Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Methods: GET, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nVary: Origin\r\n"
        ));
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    }
}

#[cfg(test)]
pub fn local(target: &str) -> Url {
    Url::parse(&format!("http://{API_HOST}{target}")).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MISSING: &str = "Додай запит";

    #[test]
    fn parses_request_heads() {
        let request = parse_request(
            "GET /pins?q=neon HTTP/1.1\r\nHost: 127.0.0.1:47821\r\norigin: http://localhost:5173\r\n\r\n",
        )
        .unwrap();
        assert_eq!(request.method, "GET");
        assert_eq!(request.target, "/pins?q=neon");
        assert_eq!(request.host.as_deref(), Some("127.0.0.1:47821"));
        assert_eq!(request.origin.as_deref(), Some("http://localhost:5173"));
        assert_eq!(parse_request("garbage\r\n\r\n"), None);
    }

    #[test]
    fn reads_search_terms() {
        assert_eq!(
            search_terms(&local("/x?q=cyberpunk+%20ui&limit=40"), MISSING),
            Ok(Search {
                query: "cyberpunk ui".into(),
                limit: 40
            })
        );
        assert_eq!(
            search_terms(&local("/x?q=%D0%BD%D1%96%D1%87%D0%BD%D0%B5+%D0%BC%D1%96%D1%81%D1%82%D0%BE"), MISSING),
            Ok(Search {
                query: "нічне місто".into(),
                limit: DEFAULT_LIMIT
            })
        );
        assert_eq!(search_terms(&local("/x"), MISSING), Err(MISSING));
        assert_eq!(search_terms(&local("/x?q=a&limit=0"), MISSING), Err(LIMIT_PROBLEM));
        assert_eq!(search_terms(&local("/x?q=a&limit=101"), MISSING), Err(LIMIT_PROBLEM));
        assert_eq!(
            search_terms(&local(&format!("/x?q={}", "x".repeat(201))), MISSING),
            Err(QUERY_TOO_LONG)
        );
    }

    #[test]
    fn answers_only_get_and_preflight() {
        let echo = |address: &Url| Reply::new(200, json!({ "path": address.path() }));
        assert_eq!(respond("GET", "/health", &echo).body["path"], "/health");
        assert_eq!(respond("OPTIONS", "/pins", &echo).status, 204);
        assert_eq!(respond("POST", "/pins?q=a", &echo).status, 405);
    }

    #[test]
    fn accepts_only_local_hosts_and_origins() {
        for host in ["127.0.0.1:47821", "localhost:47821", "LOCALHOST", "[::1]:47821"] {
            assert!(is_local_host(host), "{host}");
        }
        for host in ["evil.example:47821", "127.0.0.1.evil.example", ""] {
            assert!(!is_local_host(host), "{host}");
        }
        assert!(is_local_origin("http://localhost:5173"));
        assert!(is_local_origin("http://127.0.0.1:1420"));
        assert!(!is_local_origin("https://evil.example"));
        assert!(!is_local_origin("null"));
        assert!(!is_local_origin("file:///C:/game/index.html"));
    }

    #[test]
    fn encodes_json_replies_with_cors_for_local_pages() {
        let reply = Reply::error(400, "bad_query", "Додай запит");
        let text = String::from_utf8(encode_reply(&reply, Some("http://localhost:5173"))).unwrap();
        let (head, body) = text.split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("HTTP/1.1 400 Bad Request\r\n"));
        assert!(head.contains(&format!("Content-Length: {}", body.len())));
        assert!(head.contains("Access-Control-Allow-Origin: http://localhost:5173"));
        assert_eq!(serde_json::from_str::<Value>(body).unwrap()["error"], "bad_query");
        let plain = String::from_utf8(encode_reply(&Reply::new(204, Value::Null), None)).unwrap();
        assert!(plain.ends_with("\r\n\r\n") && !plain.contains("Access-Control"));
    }
}
