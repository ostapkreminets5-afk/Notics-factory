use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use noctics_shared::{AssetDraft, AssetKind, COMFY_API_PORT, ComfyError, TaskOrigin};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::comfy_forge::Forge;
use crate::comfy_store::Plan;

const API_HOST: &str = "127.0.0.1";
const PORT_ATTEMPTS: u16 = 10;
const LOCAL_HOSTS: [&str; 4] = ["127.0.0.1", "localhost", "[::1]", "tauri.localhost"];
const ORIGIN_SCHEMES: [&str; 3] = ["http", "https", "tauri"];
const MAX_HEAD_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 64 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONNECTIONS: usize = 32;
const HEAD_END: &[u8] = b"\r\n\r\n";
const MODEL_VIEWER: &[u8] = include_bytes!("../vendor/model-viewer.min.js");
const MODEL_VIEWER_PATH: &str = "/vendor/model-viewer.min.js";
const JSON_TYPE: &str = "application/json";

static OPEN_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Default, PartialEq, Eq)]
struct Request {
    method: String,
    target: String,
    host: Option<String>,
    origin: Option<String>,
    content_type: Option<String>,
    content_length: usize,
    range: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum Route {
    Help,
    Health,
    Models,
    Tasks,
    Task(i64),
    Create,
    File { task: String, name: String },
    Viewer,
    Preflight,
    NotFound,
    MethodNotAllowed,
}

struct Reply {
    status: u16,
    body: Value,
}

impl Reply {
    fn json(status: u16, body: Value) -> Self {
        Self { status, body }
    }

    fn error(status: u16, code: &str, message: &str) -> Self {
        Self::json(status, json!({ "error": code, "message": message }))
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TaskRequest {
    brief: String,
    kind: Option<String>,
    kinds: Vec<String>,
    project_id: Option<i64>,
    title: Option<String>,
    prompt: Option<String>,
    tags: Option<String>,
    negative: Option<String>,
    aspect: Option<String>,
    seconds: Option<f64>,
    bpm: Option<u32>,
    keyscale: Option<String>,
    lyrics: Option<String>,
}

impl TaskRequest {
    fn kinds(&self) -> Result<Vec<AssetKind>, String> {
        let codes: Vec<&str> = self
            .kind
            .iter()
            .map(String::as_str)
            .chain(self.kinds.iter().map(String::as_str))
            .collect();
        if codes.iter().any(|code| code.trim().eq_ignore_ascii_case("all")) {
            return Ok(AssetKind::ALL.to_vec());
        }
        codes
            .into_iter()
            .map(|code| AssetKind::from_code(code).ok_or_else(|| format!("Невідомий тип «{code}».")))
            .collect()
    }

    fn plan(&self) -> Option<Plan> {
        let written = [&self.prompt, &self.tags]
            .into_iter()
            .any(|field| field.as_deref().is_some_and(|text| !text.trim().is_empty()));
        written.then(|| Plan {
            title: self.title.clone().unwrap_or_default(),
            prompt: self.prompt.clone().unwrap_or_default(),
            tags: self.tags.clone().unwrap_or_default(),
            negative: self.negative.clone().unwrap_or_default(),
            aspect: self.aspect.clone().unwrap_or_default(),
            seconds: self.seconds.unwrap_or_default(),
            bpm: self.bpm.unwrap_or_default(),
            keyscale: self.keyscale.clone().unwrap_or_default(),
            lyrics: self.lyrics.clone().unwrap_or_default(),
        })
    }
}

pub fn serve(forge: Arc<Forge>) {
    thread::spawn(move || {
        let bound = (0..PORT_ATTEMPTS).find_map(|offset| {
            let port = COMFY_API_PORT + offset;
            TcpListener::bind((API_HOST, port)).ok().map(|listener| (listener, port))
        });
        let Some((listener, port)) = bound else {
            return;
        };
        forge.set_media_base(format!("http://{API_HOST}:{port}"));
        for stream in listener.incoming().flatten() {
            if OPEN_CONNECTIONS.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                OPEN_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
                let mut stream = stream;
                let _ = write_json(&mut stream, &Reply::error(503, "busy", "Забагато одночасних запитів."), None);
                continue;
            }
            let forge = Arc::clone(&forge);
            thread::spawn(move || {
                let _ = handle(stream, &forge, port);
                OPEN_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
}

fn handle(mut stream: TcpStream, forge: &Arc<Forge>, port: u16) -> std::io::Result<()> {
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    let Some((request, leftover)) = read_head(&mut stream)? else {
        return write_json(&mut stream, &Reply::error(400, "bad_request", "Некоректний HTTP-запит."), None);
    };
    if !request.host.as_deref().is_some_and(is_local_host) {
        return write_json(&mut stream, &Reply::error(403, "forbidden_host", "API приймає лише локальні запити."), None);
    }
    let origin = match request.origin.as_deref() {
        None => None,
        Some(origin) if is_local_origin(origin) => Some(origin.to_owned()),
        Some(_) => {
            return write_json(
                &mut stream,
                &Reply::error(403, "forbidden_origin", "API відповідає лише локальним сторінкам."),
                None,
            );
        }
    };
    let origin = origin.as_deref();
    match route(&request.method, &request.target) {
        Route::Help => write_json(&mut stream, &Reply::json(200, help(port)), origin),
        Route::Health => {
            let status = forge.overview().map(|overview| overview.status.online).unwrap_or(false);
            write_json(&mut stream, &Reply::json(200, json!({ "ok": true, "comfyui": status })), origin)
        }
        Route::Models => write_json(&mut stream, &Reply::json(200, json!({ "models": forge.models() })), origin),
        Route::Tasks => {
            let reply = match forge.overview() {
                Ok(overview) => Reply::json(200, json!({ "tasks": overview.tasks })),
                Err(problem) => storage_reply(&problem),
            };
            write_json(&mut stream, &reply, origin)
        }
        Route::Task(id) => {
            let reply = match forge.task(id) {
                Ok(task) => Reply::json(200, json!({ "task": task })),
                Err(ComfyError::NotFound) => Reply::error(404, "not_found", "Такої задачі немає."),
                Err(problem) => storage_reply(&problem),
            };
            write_json(&mut stream, &reply, origin)
        }
        Route::Create => {
            let reply = match read_body(&mut stream, &request, leftover) {
                Ok(body) => create(forge, &body),
                Err(reply) => reply,
            };
            write_json(&mut stream, &reply, origin)
        }
        Route::File { task, name } => match forge.served_file(&task, &name) {
            Some(path) => write_file(&mut stream, &path, request.range.as_deref(), origin),
            None => write_json(&mut stream, &Reply::error(404, "not_found", "Файлу немає."), origin),
        },
        Route::Viewer => write_bytes(&mut stream, 200, "text/javascript; charset=utf-8", MODEL_VIEWER, origin),
        Route::Preflight => write_json(&mut stream, &Reply::json(204, Value::Null), origin),
        Route::NotFound => write_json(
            &mut stream,
            &Reply::error(404, "not_found", "Такого шляху немає. Відкрий / щоб побачити довідку."),
            origin,
        ),
        Route::MethodNotAllowed => write_json(
            &mut stream,
            &Reply::error(405, "method_not_allowed", "Підтримуються GET і POST /tasks."),
            origin,
        ),
    }
}

fn create(forge: &Arc<Forge>, body: &[u8]) -> Reply {
    let Ok(request) = serde_json::from_slice::<TaskRequest>(body) else {
        return Reply::error(400, "bad_json", "Тіло має бути JSON: {\"kind\": \"sprite\", \"brief\": \"...\"}.");
    };
    let kinds = match request.kinds() {
        Ok(kinds) if !kinds.is_empty() => kinds,
        Ok(_) => return Reply::error(400, "no_kind", "Вкажи kind або kinds — напр. \"sprite\" чи [\"music\", \"sound\"]."),
        Err(message) => return Reply::error(400, "bad_kind", &message),
    };
    let draft = AssetDraft {
        brief: request.brief.clone(),
        kinds,
        project_id: request.project_id,
    };
    match forge.create(&draft, TaskOrigin::Api, request.plan()) {
        Ok(ids) => {
            let tasks: Vec<Value> = ids
                .iter()
                .filter_map(|id| forge.task(*id).ok())
                .map(|task| json!(task))
                .collect();
            Reply::json(201, json!({ "tasks": tasks, "follow": ids.iter().map(|id| format!("/tasks/{id}")).collect::<Vec<_>>() }))
        }
        Err(ComfyError::BriefMissing) => Reply::error(400, "brief_missing", "Додай brief — що згенерувати."),
        Err(ComfyError::BriefTooLong) => Reply::error(400, "brief_too_long", "brief задовгий — максимум 600 символів."),
        Err(ComfyError::NoKinds) => Reply::error(400, "no_kind", "Вкажи kind або kinds."),
        Err(ComfyError::NoModels) => Reply::error(
            409,
            "no_models",
            "У ComfyUI немає увімкнених моделей для цього типу. Подивись GET /models.",
        ),
        Err(problem) => storage_reply(&problem),
    }
}

fn storage_reply(problem: &ComfyError) -> Reply {
    Reply::error(500, "internal", &problem.to_string())
}

fn help(port: u16) -> Value {
    let kinds: Vec<Value> = AssetKind::ALL
        .iter()
        .map(|kind| json!({ "kind": kind.code(), "label": kind.label(), "hint": kind.hint() }))
        .collect();
    json!({
        "name": "Noctics ComfyUI API",
        "address": format!("http://{API_HOST}:{port}"),
        "idea": "Одна задача — усі встановлені моделі ComfyUI. Результати з’являються в Noctics → ComfyUI у галереї поруч.",
        "endpoints": {
            "GET /models": "Усі моделі ComfyUI, які бачить Noctics: готові, вимкнені, чого бракує",
            "POST /tasks": "Нова задача. JSON: kind або kinds (або \"all\"), brief; необов’язково prompt, tags, negative, aspect (square|portrait|landscape), seconds, bpm, keyscale, lyrics, title, project_id. Без prompt і tags промпт напише AI-ядро",
            "GET /tasks": "Останні задачі з результатами кожної моделі",
            "GET /tasks/<id>": "Одна задача",
            "GET /files/<task>/<file>": "Файл результату",
            "GET /health": "Чи працює API і ComfyUI"
        },
        "kinds": kinds,
        "example": {
            "kind": "sprite",
            "brief": "кристал мани, що світиться синім",
            "prompt": "Game sprite of a glowing blue mana crystal, faceted, centered, plain light background"
        },
        "note": "Працює, поки відкрито Noctics. Задачі виконуються по одній на GPU."
    })
}

fn read_head(stream: &mut TcpStream) -> std::io::Result<Option<(Request, Vec<u8>)>> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let end = loop {
        if let Some(position) = buffer.windows(HEAD_END.len()).position(|window| window == HEAD_END) {
            break position;
        }
        if buffer.len() > MAX_HEAD_BYTES {
            return Ok(None);
        }
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let leftover = buffer[end + HEAD_END.len()..].to_vec();
    let Ok(head) = String::from_utf8(buffer[..end].to_vec()) else {
        return Ok(None);
    };
    Ok(parse_head(&head).map(|request| (request, leftover)))
}

fn parse_head(head: &str) -> Option<Request> {
    let mut lines = head.split("\r\n");
    let mut start = lines.next()?.split(' ');
    let mut request = Request {
        method: start.next()?.to_owned(),
        target: start.next()?.to_owned(),
        ..Request::default()
    };
    if !start.next()?.starts_with("HTTP/") {
        return None;
    }
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().to_owned();
        match name.trim().to_ascii_lowercase().as_str() {
            "host" => request.host = Some(value),
            "origin" => request.origin = Some(value),
            "content-type" => request.content_type = Some(value),
            "content-length" => request.content_length = value.parse().ok()?,
            "range" => request.range = Some(value),
            _ => {}
        }
    }
    Some(request)
}

fn read_body(stream: &mut TcpStream, request: &Request, mut body: Vec<u8>) -> Result<Vec<u8>, Reply> {
    let json = request
        .content_type
        .as_deref()
        .is_some_and(|value| value.to_ascii_lowercase().starts_with(JSON_TYPE));
    if !json {
        return Err(Reply::error(415, "json_only", "Надсилай Content-Type: application/json."));
    }
    if request.content_length > MAX_BODY_BYTES {
        return Err(Reply::error(413, "too_large", "Тіло запиту завелике."));
    }
    let mut chunk = [0_u8; 4096];
    while body.len() < request.content_length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Err(Reply::error(400, "short_body", "Тіло запиту обірвалось.")),
            Ok(read) => body.extend_from_slice(&chunk[..read]),
        }
    }
    body.truncate(request.content_length);
    Ok(body)
}

fn route(method: &str, target: &str) -> Route {
    if method == "OPTIONS" {
        return Route::Preflight;
    }
    let Ok(address) = Url::parse(&format!("http://{API_HOST}{target}")) else {
        return Route::NotFound;
    };
    let segments: Vec<&str> = address
        .path()
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    match (method, segments.as_slice()) {
        ("GET", []) => Route::Help,
        ("GET", ["health"]) => Route::Health,
        ("GET", ["models"]) => Route::Models,
        ("GET", ["tasks"]) => Route::Tasks,
        ("POST", ["tasks"]) => Route::Create,
        ("GET", ["tasks", id]) => id.parse().map_or(Route::NotFound, Route::Task),
        ("GET", ["files", task, name]) => Route::File {
            task: (*task).to_owned(),
            name: (*name).to_owned(),
        },
        ("GET", _) if address.path() == MODEL_VIEWER_PATH => Route::Viewer,
        ("GET", _) => Route::NotFound,
        _ => Route::MethodNotAllowed,
    }
}

fn is_local_host(host: &str) -> bool {
    let name = host
        .rsplit_once(':')
        .filter(|(_, port)| !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()))
        .map_or(host, |(name, _)| name);
    LOCAL_HOSTS.contains(&name.to_ascii_lowercase().as_str())
}

fn is_local_origin(origin: &str) -> bool {
    Url::parse(origin).is_ok_and(|address| {
        ORIGIN_SCHEMES.contains(&address.scheme())
            && address
                .host_str()
                .is_some_and(|host| LOCAL_HOSTS.contains(&host.to_ascii_lowercase().as_str()))
    })
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("glb") => "model/gltf-binary",
        Some("gltf") => "model/gltf+json",
        Some("flac") => "audio/flac",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg" | "opus") => "audio/ogg",
        _ => "application/octet-stream",
    }
}

fn parse_range(header: &str, size: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?;
    if spec.contains(',') || size == 0 {
        return None;
    }
    let (start, end) = spec.split_once('-')?;
    let (first, last) = match (start.trim(), end.trim()) {
        ("", "") => return None,
        ("", suffix) => {
            let length: u64 = suffix.parse().ok()?;
            (size.saturating_sub(length), size - 1)
        }
        (start, "") => (start.parse().ok()?, size - 1),
        (start, end) => (start.parse().ok()?, end.parse::<u64>().ok()?.min(size - 1)),
    };
    (first <= last && first < size).then_some((first, last))
}

fn cors_headers(origin: Option<&str>) -> String {
    origin.map_or_else(String::new, |origin| {
        format!(
            "Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Range\r\nAccess-Control-Expose-Headers: Content-Length, Content-Range\r\nVary: Origin\r\n"
        )
    })
}

fn write_json(stream: &mut TcpStream, reply: &Reply, origin: Option<&str>) -> std::io::Result<()> {
    let body = if reply.body.is_null() {
        String::new()
    } else {
        reply.body.to_string()
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n{}\r\n",
        reply.status,
        reason(reply.status),
        body.len(),
        cors_headers(origin)
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

fn write_bytes(stream: &mut TcpStream, status: u16, kind: &str, bytes: &[u8], origin: Option<&str>) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: public, max-age=86400\r\nConnection: close\r\n{}\r\n",
        reason(status),
        bytes.len(),
        cors_headers(origin)
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(bytes)?;
    stream.flush()
}

fn write_file(stream: &mut TcpStream, path: &Path, range: Option<&str>, origin: Option<&str>) -> std::io::Result<()> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let span = range.and_then(|header| parse_range(header, size));
    let (status, first, length, extra) = match span {
        Some((first, last)) => (
            206,
            first,
            last - first + 1,
            format!("Content-Range: bytes {first}-{last}/{size}\r\n"),
        ),
        None => (200, 0, size, String::new()),
    };
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {}\r\nContent-Length: {length}\r\nAccept-Ranges: bytes\r\n{extra}Cache-Control: public, max-age=31536000, immutable\r\nConnection: close\r\n{}\r\n",
        reason(status),
        content_type(path),
        cors_headers(origin)
    );
    stream.write_all(head.as_bytes())?;
    file.seek(SeekFrom::Start(first))?;
    std::io::copy(&mut file.take(length), stream)?;
    stream.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        206 => "Partial Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_requests() {
        assert_eq!(route("GET", "/"), Route::Help);
        assert_eq!(route("GET", "/models"), Route::Models);
        assert_eq!(route("POST", "/tasks"), Route::Create);
        assert_eq!(route("GET", "/tasks/12"), Route::Task(12));
        assert_eq!(route("GET", "/tasks/x"), Route::NotFound);
        assert_eq!(
            route("GET", "/files/4/9-0.png?v=1700000000"),
            Route::File { task: "4".into(), name: "9-0.png".into() }
        );
        assert_eq!(route("GET", MODEL_VIEWER_PATH), Route::Viewer);
        assert_eq!(route("DELETE", "/tasks/1"), Route::MethodNotAllowed);
        assert_eq!(route("OPTIONS", "/tasks"), Route::Preflight);
    }

    #[test]
    fn parses_heads_with_bodies_and_ranges() {
        let request = parse_head(
            "POST /tasks HTTP/1.1\r\nHost: 127.0.0.1:47822\r\nContent-Type: application/json\r\nContent-Length: 42\r\nRange: bytes=0-99",
        )
        .unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.content_length, 42);
        assert_eq!(request.range.as_deref(), Some("bytes=0-99"));
        assert!(parse_head("garbage").is_none());
    }

    #[test]
    fn serves_byte_ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=900-", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=990-5000", 1000), Some((990, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("bytes=0-1,5-6", 1000), None);
    }

    #[test]
    fn accepts_only_local_callers() {
        assert!(is_local_host("127.0.0.1:47822"));
        assert!(is_local_host("tauri.localhost"));
        assert!(!is_local_host("evil.example:47822"));
        assert!(is_local_origin("http://tauri.localhost"));
        assert!(is_local_origin("http://localhost:1420"));
        assert!(!is_local_origin("https://evil.example"));
        assert!(!is_local_origin("null"));
    }

    #[test]
    fn reads_task_requests() {
        let request: TaskRequest =
            serde_json::from_str(r#"{"kinds":["vfx","sfx"],"brief":"вибух"}"#).unwrap();
        assert_eq!(request.kinds().unwrap(), vec![AssetKind::Effect, AssetKind::Sound]);
        assert!(request.plan().is_none());
        let explicit: TaskRequest =
            serde_json::from_str(r#"{"kind":"all","brief":"x","prompt":"a castle"}"#).unwrap();
        assert_eq!(explicit.kinds().unwrap().len(), AssetKind::ALL.len());
        assert_eq!(explicit.plan().unwrap().prompt, "a castle");
        let wrong: TaskRequest = serde_json::from_str(r#"{"kind":"video","brief":"x"}"#).unwrap();
        assert!(wrong.kinds().is_err());
    }
}
