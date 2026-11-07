use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use noctics_shared::{
    MIXAMO_API_PORT, MIXAMO_CHANGED_EVENT, MixamoError, MixamoOverview, MixamoStatus, MotionClip, MotionDraft,
    MotionOptions, MotionOrder, MotionStatus,
};
use serde_json::Value;
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use url::Url;

use crate::local_api;
use crate::mixamo_catalog::{self, Character, Details, Monitor};
use crate::mixamo_store::{MixamoStore, NewOrder, StoredOrder};
use crate::recipes;
use crate::studio::Studio;

const SITE_URL: &str = "https://www.mixamo.com/";
const API_BASE: &str = "https://www.mixamo.com/api/v1";
const API_KEY: &str = "mixamo2";
const LOGIN_WINDOW: &str = "mixamo-login";
const LOGIN_TITLE: &str = "Вхід у Mixamo · Noctics";
const PROBE_WINDOW_PREFIX: &str = "mixamo-probe-";
const TOKEN_SCRIPT: &str = r#"(() => {
  try {
    if (window.top !== window || location.hostname !== 'www.mixamo.com') {
      return '';
    }
    return localStorage.getItem('access_token') || '';
  } catch (error) {
    return '';
  }
})()"#;
const STUDIO_FOLDER: &str = "studio";
const LIST_LIMIT: usize = 200;
const SEARCH_PAGE: usize = 96;
const REMEMBERED_CLIPS: usize = 400;
const MIN_TOKEN_CHARS: usize = 20;
const MAX_TOKEN_CHARS: usize = 8192;
const MAX_JSON_BYTES: u64 = 16 * 1024 * 1024;
const RATE_LIMIT_RETRIES: u32 = 4;
const STARTUP_DELAY: Duration = Duration::from_secs(6);
const PROBE_TIMEOUT: Duration = Duration::from_secs(25);
const PROBE_COOLDOWN: Duration = Duration::from_secs(90);
const TOKEN_POLL: Duration = Duration::from_millis(1200);
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
const LOGIN_WATCH: Duration = Duration::from_secs(20 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const MONITOR_INTERVAL: Duration = Duration::from_secs(2);
const EXPORT_TIMEOUT: Duration = Duration::from_secs(240);
const UNLINKED_RECHECKS: u32 = 3;
const MIXAMO_NAME: &str = "Mixamo";
const RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(3);
const IDLE_WAIT: Duration = Duration::from_secs(60);
const SIGNED_OUT_WAIT: Duration = Duration::from_secs(30 * 60);

#[derive(Default)]
struct Session {
    token: Option<String>,
    rejected: Option<String>,
    character: Option<Character>,
    signed_in: Option<bool>,
    probed_at: Option<Instant>,
}

pub struct Mixamo {
    app: AppHandle,
    store: Arc<MixamoStore>,
    studio: Arc<Studio>,
    library: PathBuf,
    agent: ureq::Agent,
    downloads: ureq::Agent,
    session: Mutex<Session>,
    remembered: Mutex<Vec<MotionClip>>,
    probing: Mutex<()>,
    watching_login: AtomicBool,
    working: AtomicBool,
    rung: Mutex<bool>,
    bell: Condvar,
    next_window: AtomicU64,
}

impl Mixamo {
    pub fn new(store: Arc<MixamoStore>, studio: Arc<Studio>, app: AppHandle, library: PathBuf) -> Self {
        let build = |timeout: Duration| -> ureq::Agent {
            ureq::Agent::config_builder()
                .timeout_global(Some(timeout))
                .http_status_as_error(false)
                .build()
                .into()
        };
        Self {
            app,
            store,
            studio,
            library,
            agent: build(REQUEST_TIMEOUT),
            downloads: build(DOWNLOAD_TIMEOUT),
            session: Mutex::new(Session::default()),
            remembered: Mutex::new(Vec::new()),
            probing: Mutex::new(()),
            watching_login: AtomicBool::new(false),
            working: AtomicBool::new(false),
            rung: Mutex::new(false),
            bell: Condvar::new(),
            next_window: AtomicU64::new(1),
        }
    }

    pub fn start(self: &Arc<Self>) {
        let _ = self.store.recover();
        let worker = Arc::clone(self);
        thread::spawn(move || worker.work());
        let prober = Arc::clone(self);
        thread::spawn(move || {
            thread::sleep(STARTUP_DELAY);
            if prober.probe().is_some() {
                let _ = prober.character();
            }
            prober.notify();
        });
    }

    pub fn status(&self) -> MixamoStatus {
        let session = self.session();
        MixamoStatus {
            signed_in: session.signed_in,
            character: session.character.as_ref().map(|character| character.name.clone()),
            working: self.working.load(Ordering::SeqCst),
        }
    }

    pub fn overview(&self) -> Result<MixamoOverview, MixamoError> {
        Ok(MixamoOverview {
            status: self.status(),
            api: local_api::address(MIXAMO_API_PORT),
            orders: self.store.list(LIST_LIMIT)?,
        })
    }

    pub fn order(&self, id: i64) -> Result<MotionOrder, MixamoError> {
        self.store
            .order(id)?
            .map(|stored| stored.order)
            .ok_or(MixamoError::NotFound)
    }

    pub fn orders(&self, project_id: Option<i64>) -> Result<Vec<MotionOrder>, MixamoError> {
        let orders = self.store.list(LIST_LIMIT)?;
        Ok(match project_id {
            Some(id) => orders.into_iter().filter(|order| order.project_id == Some(id)).collect(),
            None => orders,
        })
    }

    pub fn motion_api(&self) -> Option<String> {
        (self.session().signed_in == Some(true)).then(|| local_api::address(MIXAMO_API_PORT))
    }

    pub fn open_login(self: &Arc<Self>) -> Result<(), MixamoError> {
        if let Some(window) = self.app.get_webview_window(LOGIN_WINDOW) {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
            return Ok(());
        }
        let window = WebviewWindowBuilder::new(&self.app, LOGIN_WINDOW, WebviewUrl::External(site()?))
            .title(LOGIN_TITLE)
            .inner_size(1180.0, 820.0)
            .center()
            .build()
            .map_err(browser)?;
        let mixamo = Arc::clone(self);
        window.on_window_event(move |event| {
            if matches!(event, WindowEvent::Destroyed) {
                mixamo.notify();
                mixamo.ring();
            }
        });
        let watcher = Arc::clone(self);
        thread::spawn(move || watcher.watch_login());
        Ok(())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<MotionClip>, MixamoError> {
        let query = query.split_whitespace().collect::<Vec<_>>().join(" ");
        if query.is_empty() {
            return Err(MixamoError::QueryMissing);
        }
        let mut link = endpoint("products")?;
        link.query_pairs_mut()
            .append_pair("page", "1")
            .append_pair("limit", &SEARCH_PAGE.to_string())
            .append_pair("order", "")
            .append_pair("type", "Motion,MotionPack")
            .append_pair("query", &query);
        let body = self.call(link.as_str(), None)?;
        let ranked: Vec<MotionClip> = mixamo_catalog::rank(&query, mixamo_catalog::parse_products(&body))
            .into_iter()
            .take(limit)
            .collect();
        self.remember(&ranked);
        Ok(ranked)
    }

    pub fn create(&self, draft: &MotionDraft) -> Result<MotionOrder, MixamoError> {
        let draft = draft.normalized()?;
        let project = match draft.project_id {
            Some(id) => Some((id, self.project_name(id)?)),
            None => None,
        };
        let new = NewOrder {
            project,
            name: &draft.name,
            query: &draft.query,
            clip_id: draft.clip_id.as_deref(),
            options: draft.options,
        };
        let id = match self.store.twin(&new)? {
            Some(id) => id,
            None => {
                let id = self.store.create(&new)?;
                if let Some(clip) = draft.clip_id.as_deref().and_then(|clip_id| self.recalled(clip_id)) {
                    self.store.set_clip(id, &clip)?;
                }
                id
            }
        };
        self.notify();
        self.ring();
        self.order(id)
    }

    pub fn plan_project(&self, project_id: i64) -> Result<Vec<MotionOrder>, MixamoError> {
        let detail = self
            .studio
            .store
            .detail(project_id)
            .map_err(|_| MixamoError::NotFound)?;
        let text = format!(
            "{} {} {}",
            detail.project.prompt, detail.project.pitch, detail.design_document
        );
        mixamo_catalog::plan_for(&text)
            .into_iter()
            .map(|planned| {
                self.create(&MotionDraft {
                    name: planned.name.to_owned(),
                    query: planned.query.to_owned(),
                    clip_id: None,
                    project_id: Some(project_id),
                    options: MotionOptions {
                        in_place: planned.in_place,
                        ..MotionOptions::default()
                    },
                })
            })
            .collect()
    }

    pub fn retry(&self, id: i64) -> Result<(), MixamoError> {
        self.store.requeue(id)?;
        self.notify();
        self.ring();
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<(), MixamoError> {
        if let Some(file) = self.store.delete(id)? {
            let path = PathBuf::from(file);
            if path.starts_with(&self.library) {
                let _ = std::fs::remove_file(path);
            }
        }
        self.notify();
        Ok(())
    }

    pub fn file(&self, id: i64) -> Result<PathBuf, MixamoError> {
        self.order(id)?
            .file
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .ok_or(MixamoError::NotFound)
    }

    fn notify(&self) {
        let _ = self.app.emit(MIXAMO_CHANGED_EVENT, ());
    }

    fn session(&self) -> MutexGuard<'_, Session> {
        self.session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn ring(&self) {
        if let Ok(mut rung) = self.rung.lock() {
            *rung = true;
        }
        self.bell.notify_all();
    }

    fn rest(&self, wait: Duration) {
        let Ok(rung) = self.rung.lock() else {
            thread::sleep(wait);
            return;
        };
        if let Ok((mut rung, _)) = self.bell.wait_timeout_while(rung, wait, |rung| !*rung) {
            *rung = false;
        }
    }

    fn work(self: Arc<Self>) {
        loop {
            let Ok(Some(stored)) = self.store.next_queued() else {
                self.rest(IDLE_WAIT);
                continue;
            };
            if self.token().is_err() {
                self.rest(SIGNED_OUT_WAIT);
                continue;
            }
            self.working.store(true, Ordering::SeqCst);
            self.process(stored);
            self.working.store(false, Ordering::SeqCst);
            self.notify();
        }
    }

    fn process(&self, stored: StoredOrder) {
        let id = stored.order.id;
        let query = stored.order.query.clone();
        match self.fulfil(&stored) {
            Ok((path, size)) => {
                let _ = self.store.finish(id, &path, size);
            }
            Err(MixamoError::SignedOut) => {
                let _ = self.store.set_status(id, MotionStatus::Queued);
            }
            Err(problem) => {
                let _ = self.store.fail(id, &describe(&problem, &query));
            }
        }
    }

    fn fulfil(&self, stored: &StoredOrder) -> Result<(PathBuf, u64), MixamoError> {
        let order = &stored.order;
        self.advance(order.id, MotionStatus::Searching)?;
        let character = self.character()?;
        let picked = match stored.clip_id.as_deref() {
            Some(clip_id) => order.clip.clone().unwrap_or_else(|| MotionClip {
                id: clip_id.to_owned(),
                ..MotionClip::default()
            }),
            None => self
                .search(&order.query, 1)?
                .into_iter()
                .next()
                .ok_or(MixamoError::NoMatch)?,
        };
        let details = self.details(&picked.id, &character)?;
        let clip = mixamo_catalog::merge_clip(Some(&picked), &details.clip, &picked.id);
        self.store.set_clip(order.id, &clip)?;
        self.advance(order.id, MotionStatus::Exporting)?;
        let body = mixamo_catalog::export_body(&character, &clip, &details, &order.options)
            .ok_or_else(|| MixamoError::Network("Mixamo віддав незнайомі параметри анімації".into()))?;
        self.call(&format!("{API_BASE}/animations/export"), Some(&body))?;
        let link = self.await_export(&character)?;
        self.advance(order.id, MotionStatus::Downloading)?;
        let destination = self.destination(order);
        let size = self.download(&link, &destination)?;
        Ok((destination, size))
    }

    fn advance(&self, id: i64, status: MotionStatus) -> Result<(), MixamoError> {
        self.store.set_status(id, status)?;
        self.notify();
        Ok(())
    }

    fn details(&self, clip_id: &str, character: &Character) -> Result<Details, MixamoError> {
        if !mixamo_catalog::is_id(clip_id) {
            return Err(MixamoError::NotFound);
        }
        let mut link = endpoint(&format!("products/{clip_id}"))?;
        link.query_pairs_mut()
            .append_pair("similar", "0")
            .append_pair("character_id", &character.id);
        let body = self.call(link.as_str(), None)?;
        mixamo_catalog::read_details(&body)
            .ok_or_else(|| MixamoError::Network("Mixamo не віддав параметри цієї анімації".into()))
    }

    fn await_export(&self, character: &Character) -> Result<String, MixamoError> {
        let monitor = format!("{API_BASE}/characters/{}/monitor", character.id);
        let deadline = Instant::now() + EXPORT_TIMEOUT;
        let mut unlinked = 0;
        while Instant::now() < deadline {
            thread::sleep(MONITOR_INTERVAL);
            let progress = match self.call(&monitor, None) {
                Err(MixamoError::NotFound) => continue,
                other => other?,
            };
            match mixamo_catalog::read_monitor(&progress) {
                Monitor::Working => {}
                Monitor::Ready(link) => return Ok(link),
                Monitor::Unlinked(reason) => {
                    unlinked += 1;
                    if unlinked > UNLINKED_RECHECKS {
                        return Err(MixamoError::Network(reason));
                    }
                }
                Monitor::Failed(message) => return Err(MixamoError::Network(message)),
            }
        }
        Err(MixamoError::Network("Mixamo запікає анімацію задовго — спробуй ще раз".into()))
    }

    fn destination(&self, order: &MotionOrder) -> PathBuf {
        let folder = order
            .project_id
            .map_or_else(|| STUDIO_FOLDER.to_owned(), |id| format!("project-{id}"));
        let file = format!(
            "{}-{}.{}",
            recipes::slugify(&order.name),
            order.id,
            mixamo_catalog::FILE_EXTENSION
        );
        self.library.join(folder).join(file)
    }

    fn download(&self, link: &str, destination: &Path) -> Result<u64, MixamoError> {
        let mut response = self.downloads.get(link).call().map_err(network)?;
        if !response.status().is_success() {
            return Err(MixamoError::Network(format!(
                "файл не завантажився ({})",
                response.status().as_u16()
            )));
        }
        let parent = destination.parent().unwrap_or(&self.library);
        std::fs::create_dir_all(parent).map_err(disk)?;
        let partial = destination.with_extension("part");
        let mut file = File::create(&partial).map_err(disk)?;
        let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
        let copied = std::io::copy(&mut reader, &mut file).map_err(disk)?;
        file.flush().map_err(disk)?;
        drop(file);
        if copied == 0 {
            let _ = std::fs::remove_file(&partial);
            return Err(MixamoError::Network("Mixamo віддав порожній файл".into()));
        }
        std::fs::rename(&partial, destination).map_err(disk)?;
        Ok(copied)
    }

    fn character(&self) -> Result<Character, MixamoError> {
        if let Some(character) = self.session().character.clone() {
            return Ok(character);
        }
        let primary = self.call(&format!("{API_BASE}/characters/primary"), None);
        let found = match primary {
            Err(MixamoError::SignedOut) => return Err(MixamoError::SignedOut),
            Ok(body) => mixamo_catalog::read_primary(&body),
            Err(_) => None,
        };
        let character = match found {
            Some(character) => character,
            None => {
                let mut link = endpoint("products")?;
                link.query_pairs_mut()
                    .append_pair("page", "1")
                    .append_pair("limit", &SEARCH_PAGE.to_string())
                    .append_pair("order", "")
                    .append_pair("type", "Character")
                    .append_pair("query", "");
                mixamo_catalog::pick_character(&self.call(link.as_str(), None)?).ok_or(MixamoError::NoMatch)?
            }
        };
        self.session().character = Some(character.clone());
        self.notify();
        Ok(character)
    }

    fn call(&self, link: &str, body: Option<&Value>) -> Result<Value, MixamoError> {
        let token = self.token()?;
        let bearer = format!("Bearer {token}");
        for attempt in 0..=RATE_LIMIT_RETRIES {
            let sent = match body {
                Some(body) => self
                    .agent
                    .post(link)
                    .header("Authorization", &bearer)
                    .header("X-Api-Key", API_KEY)
                    .header("Accept", "application/json")
                    .header("Content-Type", "application/json")
                    .send(body.to_string()),
                None => self
                    .agent
                    .get(link)
                    .header("Authorization", &bearer)
                    .header("X-Api-Key", API_KEY)
                    .header("Accept", "application/json")
                    .call(),
            };
            let mut response = sent.map_err(network)?;
            match response.status().as_u16() {
                200..=299 => {
                    let text = response
                        .body_mut()
                        .with_config()
                        .limit(MAX_JSON_BYTES)
                        .read_to_string()
                        .unwrap_or_default();
                    return Ok(serde_json::from_str(&text).unwrap_or(Value::Null));
                }
                401 | 403 => {
                    self.reject(&token);
                    return Err(MixamoError::SignedOut);
                }
                404 => return Err(MixamoError::NotFound),
                429 if attempt < RATE_LIMIT_RETRIES => thread::sleep(RATE_LIMIT_BACKOFF * (attempt + 1)),
                status => return Err(MixamoError::Network(format!("Mixamo відповів {status}"))),
            }
        }
        Err(MixamoError::Network("Mixamo просить зачекати — забагато запитів".into()))
    }

    fn token(&self) -> Result<String, MixamoError> {
        if let Some(token) = self.session().token.clone() {
            return Ok(token);
        }
        self.probe().ok_or(MixamoError::SignedOut)
    }

    fn probe(&self) -> Option<String> {
        let _turn = self.probing.lock().ok()?;
        {
            let mut session = self.session();
            if let Some(token) = session.token.clone() {
                return Some(token);
            }
            if session.probed_at.is_some_and(|at| at.elapsed() < PROBE_COOLDOWN) {
                return None;
            }
            session.probed_at = Some(Instant::now());
        }
        let found = self.read_site_token();
        let fresh = found.filter(|token| self.session().rejected.as_deref() != Some(token.as_str()));
        match fresh {
            Some(token) => {
                self.adopt(token.clone());
                Some(token)
            }
            None => {
                self.session().signed_in = Some(false);
                self.notify();
                None
            }
        }
    }

    fn read_site_token(&self) -> Option<String> {
        let label = format!(
            "{PROBE_WINDOW_PREFIX}{}",
            self.next_window.fetch_add(1, Ordering::SeqCst)
        );
        let window = WebviewWindowBuilder::new(&self.app, label, WebviewUrl::External(site().ok()?))
            .title("Mixamo")
            .inner_size(900.0, 700.0)
            .visible(false)
            .focused(false)
            .skip_taskbar(true)
            .on_new_window(|_, _| NewWindowResponse::Deny)
            .build()
            .ok()?;
        let deadline = Instant::now() + PROBE_TIMEOUT;
        let mut token = None;
        while token.is_none() && Instant::now() < deadline {
            thread::sleep(TOKEN_POLL);
            token = read_token(&window);
        }
        let _ = window.destroy();
        token
    }

    fn watch_login(self: Arc<Self>) {
        if self.watching_login.swap(true, Ordering::SeqCst) {
            return;
        }
        let deadline = Instant::now() + LOGIN_WATCH;
        while Instant::now() < deadline {
            thread::sleep(TOKEN_POLL);
            let Some(window) = self.app.get_webview_window(LOGIN_WINDOW) else {
                break;
            };
            let Some(token) = read_token(&window) else {
                continue;
            };
            let known = {
                let session = self.session();
                session.rejected.as_deref() == Some(token.as_str())
                    || (session.token.as_deref() == Some(token.as_str()) && session.character.is_some())
            };
            if known {
                continue;
            }
            self.adopt(token);
            if self.character().is_ok() {
                let _ = window.close();
                break;
            }
        }
        self.watching_login.store(false, Ordering::SeqCst);
    }

    fn adopt(&self, token: String) {
        {
            let mut session = self.session();
            if session.token.as_deref() != Some(token.as_str()) {
                session.character = None;
            }
            session.token = Some(token);
            session.signed_in = Some(true);
        }
        self.notify();
        self.ring();
    }

    fn reject(&self, token: &str) {
        {
            let mut session = self.session();
            if session.token.as_deref() == Some(token) {
                session.token = None;
                session.character = None;
            }
            session.rejected = Some(token.to_owned());
            session.signed_in = Some(false);
        }
        self.notify();
    }

    fn project_name(&self, project_id: i64) -> Result<String, MixamoError> {
        self.studio
            .store
            .list_projects()
            .map_err(|problem| MixamoError::Storage(problem.to_string()))?
            .into_iter()
            .find(|project| project.id == project_id)
            .map(|project| project.name)
            .ok_or(MixamoError::NotFound)
    }

    fn remember(&self, clips: &[MotionClip]) {
        let Ok(mut remembered) = self.remembered.lock() else {
            return;
        };
        remembered.retain(|known| !clips.iter().any(|clip| clip.id == known.id));
        remembered.extend(clips.iter().cloned());
        let overflow = remembered.len().saturating_sub(REMEMBERED_CLIPS);
        remembered.drain(..overflow);
    }

    fn recalled(&self, clip_id: &str) -> Option<MotionClip> {
        self.remembered
            .lock()
            .ok()?
            .iter()
            .find(|clip| clip.id == clip_id)
            .cloned()
    }
}

fn read_token(window: &WebviewWindow) -> Option<String> {
    let (sender, receiver) = mpsc::channel();
    window
        .eval_with_callback(TOKEN_SCRIPT, move |reply| {
            let _ = sender.send(reply);
        })
        .ok()?;
    let reply = receiver.recv_timeout(REPLY_TIMEOUT).ok()?;
    let token: String = serde_json::from_str(&reply).ok()?;
    let token = token.trim();
    is_token(token).then(|| token.to_owned())
}

fn is_token(token: &str) -> bool {
    (MIN_TOKEN_CHARS..=MAX_TOKEN_CHARS).contains(&token.len())
        && token
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\'' | b'\\'))
}

fn site() -> Result<Url, MixamoError> {
    Url::parse(SITE_URL).map_err(browser)
}

fn endpoint(path: &str) -> Result<Url, MixamoError> {
    Url::parse(&format!("{API_BASE}/{path}")).map_err(browser)
}

fn describe(problem: &MixamoError, query: &str) -> String {
    match problem {
        MixamoError::NoMatch => format!("На Mixamo немає анімації під запит «{query}»"),
        MixamoError::NotFound => "Mixamo не знайшов цю анімацію".to_owned(),
        MixamoError::Network(detail) if detail.starts_with(MIXAMO_NAME) => detail.clone(),
        MixamoError::Network(detail) => format!("{MIXAMO_NAME}: {detail}"),
        MixamoError::Storage(detail) => format!("Не вдалося зберегти анімацію: {detail}"),
        other => other.to_string(),
    }
}

fn browser(error: impl std::fmt::Display) -> MixamoError {
    MixamoError::Browser(error.to_string())
}

fn network(error: impl std::fmt::Display) -> MixamoError {
    MixamoError::Network(error.to_string())
}

fn disk(error: std::io::Error) -> MixamoError {
    MixamoError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_plausible_tokens() {
        assert!(is_token(&"a".repeat(MIN_TOKEN_CHARS)));
        assert!(is_token("eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl"));
        assert!(!is_token("short"));
        assert!(!is_token("has space in the middle of token"));
        assert!(!is_token("quote\"injection-attempt-here"));
    }

    #[test]
    fn explains_failures_for_the_order_card() {
        assert_eq!(
            describe(&MixamoError::NoMatch, "flying kick"),
            "На Mixamo немає анімації під запит «flying kick»"
        );
        assert_eq!(describe(&MixamoError::Network("429".into()), "x"), "Mixamo: 429");
        assert_eq!(
            describe(&MixamoError::Network("Mixamo не дав посилання на файл".into()), "x"),
            "Mixamo не дав посилання на файл"
        );
    }
}
