use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use noctics_shared::{PINTEREST_API_PORT, Pin, PinterestError, pin_page};
use serde_json::{Value, json};
use url::Url;

use crate::pinterest::Pinterest;


const API_PORT: u16 = PINTEREST_API_PORT;
const API_HOST: &str = "127.0.0.1";
const LOCAL_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "[::1]"];
const DEFAULT_LIMIT: usize = 25;
const MAX_LIMIT: usize = 100;
const MAX_QUERY_CHARS: usize = 200;
const BASE_TIMEOUT: Duration = Duration::from_secs(25);
const TIMEOUT_PER_PIN: Duration = Duration::from_millis(500);
const MAX_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_REQUEST_BYTES: usize = 8 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONNECTIONS: usize = 16;
const HEAD_END: &[u8] = b"\r\n\r\n";
const SIGNED_OUT_MESSAGE: &str = "Pinterest нічого не віддав. Увійди в Pinterest у Noctics і спробуй ще раз.";

static OPEN_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, PartialEq, Eq)]
struct Request {
    method: String,
    target: String,
    host: Option<String>,
    origin: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum Route {
    Help,
    Health,
    Pins { query: String, limit: usize },
    Preflight,
    BadQuery(&'static str),
    NotFound,
    MethodNotAllowed,
}

struct Reply {
    status: u16,
    body: Value,
}

impl Reply {
    fn new(status: u16, body: Value) -> Self {
        Self { status, body }
    }

    fn error(status: u16, code: &str, message: &str) -> Self {
        Self::new(status, json!({ "error": code, "message": message }))
    }
}

pub fn serve(pinterest: Arc<Pinterest>) {
    thread::spawn(move || {
        let Ok(listener) = TcpListener::bind((API_HOST, API_PORT)) else {
            return;
        };
        for stream in listener.incoming().flatten() {
            if OPEN_CONNECTIONS.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                OPEN_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
                let mut stream = stream;
                let _ = write_reply(
                    &mut stream,
                    &Reply::error(503, "busy", "Забагато одночасних запитів."),
                    None,
                );
                continue;
            }
            let pinterest = Arc::clone(&pinterest);
            thread::spawn(move || {
                let _ = handle(stream, &pinterest);
                OPEN_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
}

fn address() -> String {
    format!("http://{API_HOST}:{API_PORT}")
}

fn handle(mut stream: TcpStream, pinterest: &Pinterest) -> std::io::Result<()> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    let Some(head) = read_head(&mut stream)? else {
        return write_reply(&mut stream, &Reply::error(400, "bad_request", "Некоректний HTTP-запит."), None);
    };
    let Some(request) = parse_request(&head) else {
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
    let reply = match route(&request.method, &request.target) {
        Route::Help => Reply::new(200, help()),
        Route::Health => Reply::new(200, json!({ "ok": true, "signed_in": pinterest.status().signed_in })),
        Route::Pins { query, limit } => search(pinterest, &query, limit),
        Route::Preflight => Reply::new(204, Value::Null),
        Route::BadQuery(message) => Reply::error(400, "bad_query", message),
        Route::NotFound => Reply::error(404, "not_found", "Такого шляху немає. Відкрий / щоб побачити довідку."),
        Route::MethodNotAllowed => Reply::error(405, "method_not_allowed", "Підтримується лише GET."),
    };
    write_reply(&mut stream, &reply, allowed_origin.as_deref())
}

fn search(pinterest: &Pinterest, query: &str, limit: usize) -> Reply {
    let signed_in = pinterest.status().signed_in;
    match pinterest.search_pins(query, limit, timeout_for(limit)) {
        Ok(pins) if pins.is_empty() && signed_in != Some(true) => {
            Reply::error(401, "signed_out", SIGNED_OUT_MESSAGE)
        }
        Ok(pins) => Reply::new(
            200,
            json!({
                "query": query,
                "count": pins.len(),
                "signed_in": signed_in,
                "pins": pins.iter().map(pin_json).collect::<Vec<_>>(),
            }),
        ),
        Err(PinterestError::Browser(detail)) => Reply::error(502, "browser", &detail),
        Err(other) => Reply::error(500, "internal", &other.to_string()),
    }
}

fn pin_json(pin: &Pin) -> Value {
    json!({
        "id": pin.id,
        "image": pin.image,
        "title": pin.title,
        "link": pin_page(&pin.id),
    })
}

fn help() -> Value {
    json!({
        "name": "Noctics Pinterest API",
        "free": true,
        "address": address(),
        "endpoints": {
            "/pins?q=<запит>&limit=<1-100>": "Піни з пошуку Pinterest: id, image, title, link",
            "/health": "Чи працює API і чи ввійдено в Pinterest",
        },
        "note": "Працює, поки відкрито Noctics. Запити виконуються по одному.",
    })
}

fn timeout_for(limit: usize) -> Duration {
    let extra = TIMEOUT_PER_PIN.saturating_mul(u32::try_from(limit).unwrap_or(u32::MAX));
    (BASE_TIMEOUT + extra).min(MAX_TIMEOUT)
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

fn route(method: &str, target: &str) -> Route {
    match method {
        "GET" => {}
        "OPTIONS" => return Route::Preflight,
        _ => return Route::MethodNotAllowed,
    }
    let Ok(address) = Url::parse(&format!("http://{API_HOST}{target}")) else {
        return Route::NotFound;
    };
    match address.path() {
        "/" => Route::Help,
        "/health" => Route::Health,
        "/pins" => pins_route(&address),
        _ => Route::NotFound,
    }
}

fn pins_route(address: &Url) -> Route {
    let parameter = |name: &str| {
        address
            .query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    let query = parameter("q")
        .map(|raw| raw.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    if query.is_empty() {
        return Route::BadQuery("Додай запит: /pins?q=cyberpunk+ui");
    }
    if query.chars().count() > MAX_QUERY_CHARS {
        return Route::BadQuery("Запит задовгий — максимум 200 символів.");
    }
    let limit = match parameter("limit") {
        None => DEFAULT_LIMIT,
        Some(raw) => match raw.trim().parse::<usize>() {
            Ok(value) if (1..=MAX_LIMIT).contains(&value) => value,
            _ => return Route::BadQuery("limit має бути числом від 1 до 100."),
        },
    };
    Route::Pins { query, limit }
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
mod tests {
    use super::*;

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
    fn routes_pin_searches() {
        assert_eq!(
            route("GET", "/pins?q=cyberpunk+%20ui&limit=40"),
            Route::Pins {
                query: "cyberpunk ui".into(),
                limit: 40
            }
        );
        assert_eq!(
            route("GET", "/pins?q=%D0%BD%D1%96%D1%87%D0%BD%D0%B5+%D0%BC%D1%96%D1%81%D1%82%D0%BE"),
            Route::Pins {
                query: "нічне місто".into(),
                limit: DEFAULT_LIMIT
            }
        );
        assert!(matches!(route("GET", "/pins"), Route::BadQuery(_)));
        assert!(matches!(route("GET", "/pins?q=a&limit=0"), Route::BadQuery(_)));
        assert!(matches!(route("GET", "/pins?q=a&limit=101"), Route::BadQuery(_)));
        assert!(matches!(route("GET", &format!("/pins?q={}", "x".repeat(201))), Route::BadQuery(_)));
        assert_eq!(route("GET", "/"), Route::Help);
        assert_eq!(route("GET", "/health"), Route::Health);
        assert_eq!(route("GET", "/admin"), Route::NotFound);
        assert_eq!(route("POST", "/pins?q=a"), Route::MethodNotAllowed);
        assert_eq!(route("OPTIONS", "/pins"), Route::Preflight);
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
    fn scales_timeouts_with_the_limit() {
        assert_eq!(timeout_for(10), BASE_TIMEOUT + Duration::from_secs(5));
        assert_eq!(timeout_for(MAX_LIMIT), Duration::from_secs(75));
        assert_eq!(timeout_for(10_000), MAX_TIMEOUT);
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
