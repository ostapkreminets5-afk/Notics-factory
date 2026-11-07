use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use noctics_shared::{
    DETAIL_ARTWORK_SIZE, DETAIL_PIN_SIZE, DesignRunStatus, DesignSource, DesignVariant, OPEN_DESIGN_PORT,
    OpenDesignBoard, OpenDesignError, ProjectDetail, Reference, ReferenceTopic, StudioError, variant_screen,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};
use url::Url;

use crate::artstation::ArtStation;
use crate::pinterest::Pinterest;
use crate::process;
use crate::studio::{CHANGED_EVENT, Studio};
use crate::studio_store::StudioStore;

const LOCAL_APP_DATA: &str = "LOCALAPPDATA";
const INSTALL_PARENT: &str = "Programs";
const INSTALL_FOLDER: &str = "Open Design";
const APP_EXE: &str = "Open Design.exe";
const RESOURCES_FOLDER: &str = "resources";
const RESOURCE_ROOT_FOLDER: &str = "open-design";
const CONFIG_FILE: &str = "open-design-config.json";
const CLI_ENTRY_KEY: &str = "daemonCliEntryRelative";
const PROJECTS_FOLDER: &str = "projects";
const JOBS_FILE: &str = "noctics-jobs.json";
const DAEMON_LOG: &str = "daemon.log";
const HOST: &str = "127.0.0.1";
const SKILL: &str = "frontend-design";
const AGENT: &str = "claude";
const HTML_EXTENSION: &str = "html";
const EXPORT_QUERY_KEY: &str = "inline";
const EXPORT_QUERY_VALUE: &str = "1";
const TERMINAL_RUN_STATES: [&str; 4] = ["succeeded", "failed", "canceled", "cancelled"];
const FAILED_RUN_STATE: &str = "failed";
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const DAEMON_BOOT: Duration = Duration::from_secs(60);
const BOOT_PROBE: Duration = Duration::from_millis(700);
const POLL_INTERVAL: Duration = Duration::from_secs(5);
const QUIET_POLLS: u32 = 3;
const MAX_ROUND: Duration = Duration::from_secs(180 * 60);
const STARTUP_DELAY: Duration = Duration::from_secs(8);
const REFERENCE_WAIT: Duration = Duration::from_secs(6 * 60);
const REFERENCE_PROBE: Duration = Duration::from_secs(2);
const IMAGE_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 6 * 1024 * 1024;
const MAX_TITLE_SCAN_BYTES: usize = 64 * 1024;
const MAX_SCREENS: usize = 8;
const MAX_VARIANTS: usize = 256;
const MAX_IMAGES_PER_SOURCE: usize = 12;
const MAX_IMAGE_ATTEMPTS: usize = 24;
const SCAN_DEPTH: usize = 3;
const REFERENCES_FOLDER: &str = "noctics-references";
const USER_AGENT: &str = "Mozilla/5.0 (compatible; NocticsFactory/0.1)";
const IMAGE_TYPES: [(&str, &str); 4] = [
    ("image/jpeg", "jpg"),
    ("image/png", "png"),
    ("image/webp", "webp"),
    ("image/gif", "gif"),
];
const SCREEN_TOPICS: [ReferenceTopic; 5] = [
    ReferenceTopic::Interface,
    ReferenceTopic::MainMenu,
    ReferenceTopic::Panels,
    ReferenceTopic::Buttons,
    ReferenceTopic::Icons,
];
const FALLBACK_DIRECTION: &str = "dramatic";
const DIRECTIONS: [(&str, &[&str]); 8] = [
    ("neon", &["неон", "neon", "кіберпанк", "cyberpunk", "synthwave"]),
    ("cosmic", &["космо", "космос", "space", "галакт", "galax", "планет", "planet"]),
    ("retro", &["піксел", "pixel", "8-біт", "16-біт", "8-bit", "16-bit", "ретро", "retro", "аркад", "arcade"]),
    ("futuristic", &["sci-fi", "футур", "futur", "робот", "robot", "меха", "mech"]),
    ("hud", &["шутер", "shooter", "тактич", "tactical", "військ", "military"]),
    (
        "fantasy",
        &["фентез", "fantasy", "магі", "magic", "казк", "fairy", "дракон", "dragon", "лицар", "knight", "підземел", "dungeon"],
    ),
    ("doodle", &["затишн", "cozy", "мультяш", "cartoon", "cute"]),
    ("vintage", &["нуар", "noir", "вінтаж", "vintage"]),
];
const ALTERNATE_DIRECTIONS: [&str; 6] = ["dramatic", "bold", "skeumorphism", "glassmorphism", "vibrant", "retro"];

#[derive(Clone)]
struct Install {
    root: PathBuf,
    exe: PathBuf,
    cli: PathBuf,
    resources: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Job {
    od_project: Option<String>,
    conversation: Option<String>,
    design_system: Option<String>,
    status: DesignRunStatus,
    rounds: u32,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    error: Option<String>,
    #[serde(default)]
    skipped: Vec<DesignSource>,
}

enum RunsState {
    Active,
    Settled { failed: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    url: String,
    topic: ReferenceTopic,
    title: String,
    author: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShelfImage {
    file: String,
    topic: ReferenceTopic,
    title: String,
    author: String,
}

#[derive(Debug, Default)]
struct Shelf {
    pinterest: Vec<ShelfImage>,
    artstation: Vec<ShelfImage>,
}

impl Shelf {
    fn sources(&self) -> Vec<DesignSource> {
        DesignSource::ALL
            .into_iter()
            .filter(|source| match source {
                DesignSource::Prompt => true,
                DesignSource::Pinterest => !self.pinterest.is_empty(),
                DesignSource::Artstation => !self.artstation.is_empty(),
                DesignSource::Mixed => !self.pinterest.is_empty() && !self.artstation.is_empty(),
            })
            .collect()
    }

    fn skipped(&self) -> Vec<DesignSource> {
        let sources = self.sources();
        DesignSource::ALL
            .into_iter()
            .filter(|source| !sources.contains(source))
            .collect()
    }
}

pub struct OpenDesign {
    app: AppHandle,
    data_dir: PathBuf,
    agent: ureq::Agent,
    probe: ureq::Agent,
    images: ureq::Agent,
    jobs: Mutex<BTreeMap<i64, Job>>,
    daemon: Mutex<()>,
}

impl OpenDesign {
    pub fn new(app: AppHandle, data_dir: PathBuf) -> Self {
        let jobs = fs::read_to_string(data_dir.join(JOBS_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            app,
            data_dir,
            agent: http_agent(REQUEST_TIMEOUT),
            probe: http_agent(HEALTH_TIMEOUT),
            images: http_agent(IMAGE_TIMEOUT),
            jobs: Mutex::new(jobs),
            daemon: Mutex::new(()),
        }
    }

    pub fn board(&self, project_id: i64) -> OpenDesignBoard {
        let job = self.job(project_id);
        let variants = job
            .od_project
            .as_deref()
            .map(|od_project| self.variants(od_project))
            .unwrap_or_default();
        OpenDesignBoard {
            project_id,
            installed: find_install().is_some(),
            status: job.status,
            design_system: job.design_system,
            rounds: job.rounds,
            started_at: job.started_at,
            finished_at: job.finished_at,
            error: job.error,
            skipped: job.skipped,
            variants,
        }
    }

    pub fn project_folder(&self, project_id: i64) -> Option<PathBuf> {
        let folder = self.project_root(&self.job(project_id).od_project?);
        folder.is_dir().then_some(folder)
    }

    pub fn variant_url(&self, project_id: i64, file: &str) -> Option<String> {
        let od_project = self.job(project_id).od_project?;
        self.variants(&od_project)
            .into_iter()
            .find(|variant| variant.file == file)
            .map(|variant| variant.preview_url)
    }

    pub fn start(self: &Arc<Self>, store: Arc<StudioStore>, project_id: i64) -> Result<(), OpenDesignError> {
        let install = find_install().ok_or(OpenDesignError::NotInstalled)?;
        let detail = store.detail(project_id).map_err(studio_error)?;
        if detail.screens.is_empty() {
            return Err(OpenDesignError::NotPlanned);
        }
        {
            let mut jobs = self.jobs();
            let job = jobs.entry(project_id).or_default();
            if job.status.is_busy() {
                return Ok(());
            }
            job.status = DesignRunStatus::Starting;
            job.error = None;
            job.started_at = Some(now_seconds());
            job.finished_at = None;
        }
        self.persist();
        self.notify();
        let open_design = Arc::clone(self);
        thread::spawn(move || {
            open_design.wait_for_references(project_id);
            let launched = store
                .detail(project_id)
                .map_err(studio_error)
                .and_then(|fresh| open_design.launch(&install, project_id, &fresh));
            match launched {
                Ok(()) => open_design.watch(project_id),
                Err(problem) => open_design.finish(project_id, Some(describe(&problem))),
            }
        });
        Ok(())
    }

    fn wait_for_references(&self, project_id: i64) {
        if !self.references_loading(project_id) {
            return;
        }
        self.update(project_id, |job| job.status = DesignRunStatus::WaitingReferences);
        let deadline = Instant::now() + REFERENCE_WAIT;
        while self.references_loading(project_id) && Instant::now() < deadline {
            thread::sleep(REFERENCE_PROBE);
        }
        self.update(project_id, |job| job.status = DesignRunStatus::Starting);
    }

    fn references_loading(&self, project_id: i64) -> bool {
        let pinterest = self
            .app
            .try_state::<Arc<Pinterest>>()
            .is_some_and(|pinterest| pinterest.is_loading(project_id));
        let artstation = self
            .app
            .try_state::<Arc<ArtStation>>()
            .is_some_and(|artstation| artstation.is_loading(project_id));
        pinterest || artstation
    }

    fn launch(&self, install: &Install, project_id: i64, detail: &ProjectDetail) -> Result<(), OpenDesignError> {
        self.ensure_daemon(install)?;
        let job = self.job(project_id);
        let design_system = pick_design_system(detail, job.rounds);
        let existing = match (job.od_project, job.conversation) {
            (Some(od_project), Some(conversation)) if self.project_root(&od_project).is_dir() => {
                Some((od_project, conversation))
            }
            _ => None,
        };
        let (od_project, conversation) = match existing {
            Some(pair) => pair,
            None => self.create_project(project_id, &detail.project.name, design_system)?,
        };
        let shelf = self.stock_shelf(&od_project, &detail.project.references);
        self.post_json(
            "/api/runs",
            &json!({
                "projectId": od_project,
                "conversationId": conversation,
                "message": brief(detail, job.rounds, design_system, &shelf),
                "skillId": SKILL,
                "designSystemId": design_system,
                "agentId": AGENT,
            }),
        )?;
        let skipped = shelf.skipped();
        self.update(project_id, |job| {
            job.od_project = Some(od_project.clone());
            job.conversation = Some(conversation.clone());
            job.design_system = Some(design_system.to_owned());
            job.status = DesignRunStatus::Running;
            job.skipped = skipped;
            job.rounds += 1;
        });
        Ok(())
    }

    fn stock_shelf(&self, od_project: &str, references: &[Reference]) -> Shelf {
        let folder = self.project_root(od_project).join(REFERENCES_FOLDER);
        let _ = fs::remove_dir_all(&folder);
        Shelf {
            pinterest: self.stock(&folder, DesignSource::Pinterest, references),
            artstation: self.stock(&folder, DesignSource::Artstation, references),
        }
    }

    fn stock(&self, folder: &Path, source: DesignSource, references: &[Reference]) -> Vec<ShelfImage> {
        let target = folder.join(source.code());
        let mut shelf = Vec::new();
        let wanted = candidates(references, source);
        if wanted.is_empty() || fs::create_dir_all(&target).is_err() {
            return shelf;
        }
        for candidate in wanted.into_iter().take(MAX_IMAGE_ATTEMPTS) {
            if shelf.len() >= MAX_IMAGES_PER_SOURCE {
                break;
            }
            let Some((bytes, extension)) = self.fetch_image(&candidate.url) else {
                continue;
            };
            let name = format!("{:02}-{}.{extension}", shelf.len() + 1, candidate.topic.code().replace('_', "-"));
            if fs::write(target.join(&name), bytes).is_ok() {
                shelf.push(ShelfImage {
                    file: format!("{REFERENCES_FOLDER}/{}/{name}", source.code()),
                    topic: candidate.topic,
                    title: candidate.title,
                    author: candidate.author,
                });
            }
        }
        shelf
    }

    fn fetch_image(&self, address: &str) -> Option<(Vec<u8>, &'static str)> {
        let mut response = self
            .images
            .get(address)
            .header("User-Agent", USER_AGENT)
            .call()
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let extension = IMAGE_TYPES
            .iter()
            .find(|(mime, _)| content_type.starts_with(mime))
            .map(|(_, extension)| *extension)?;
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_IMAGE_BYTES)
            .read_to_vec()
            .ok()?;
        (!bytes.is_empty()).then_some((bytes, extension))
    }

    fn create_project(&self, project_id: i64, game_name: &str, design_system: &str) -> Result<(String, String), OpenDesignError> {
        let reply = self.post_json(
            "/api/projects",
            &json!({
                "id": format!("noctics-{project_id}-{}", now_seconds()),
                "name": format!("{game_name} · Noctics"),
                "skillId": SKILL,
                "designSystemId": design_system,
                "skipDiscoveryBrief": true,
            }),
        )?;
        let od_project = reply["project"]["id"].as_str().map(str::to_owned);
        let conversation = reply["conversationId"].as_str().map(str::to_owned);
        od_project
            .zip(conversation)
            .ok_or_else(|| OpenDesignError::Daemon("Open Design не повернув проєкт".into()))
    }

    fn watch(&self, project_id: i64) {
        let deadline = Instant::now() + MAX_ROUND;
        let mut quiet = 0;
        let mut seen = self.board(project_id).variants.len();
        loop {
            thread::sleep(POLL_INTERVAL);
            let Some(od_project) = self.job(project_id).od_project else {
                return;
            };
            let found = self.variants(&od_project).len();
            if found != seen {
                seen = found;
                self.notify();
            }
            match self.runs_state(&od_project) {
                Ok(RunsState::Active) => quiet = 0,
                Ok(RunsState::Settled { failed }) => {
                    quiet += 1;
                    if quiet >= QUIET_POLLS {
                        let problem = match (found, failed) {
                            (0, Some(reason)) => Some(reason),
                            (0, None) => Some("Open Design завершив роботу, але не зробив жодного HTML-екрана.".to_owned()),
                            _ => None,
                        };
                        self.finish(project_id, problem);
                        return;
                    }
                }
                Err(_) if !self.healthy() => {
                    self.finish(project_id, Some("Open Design зупинився посеред роботи — запусти ще раз.".into()));
                    return;
                }
                Err(_) => {}
            }
            if Instant::now() >= deadline {
                self.finish(project_id, Some("Open Design працює задовго — перевір папку проєкту й запусти ще раз.".into()));
                return;
            }
        }
    }

    fn runs_state(&self, od_project: &str) -> Result<RunsState, OpenDesignError> {
        let mut address = self.endpoint("/api/runs")?;
        address.query_pairs_mut().append_pair("projectId", od_project);
        let reply = self.get_json(&address)?;
        let runs = reply
            .get("runs")
            .or(Some(&reply))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let state_of = |run: &Value| run["status"].as_str().unwrap_or_default().to_owned();
        if runs.iter().any(|run| !TERMINAL_RUN_STATES.contains(&state_of(run).as_str())) {
            return Ok(RunsState::Active);
        }
        let latest = runs.iter().max_by_key(|run| run["createdAt"].as_i64().unwrap_or_default());
        let failed = latest
            .filter(|run| state_of(run) == FAILED_RUN_STATE)
            .map(|run| {
                run["error"]["message"]
                    .as_str()
                    .or_else(|| run["error"].as_str())
                    .unwrap_or("Open Design не впорався з екранами.")
                    .to_owned()
            });
        Ok(RunsState::Settled { failed })
    }

    fn finish(&self, project_id: i64, problem: Option<String>) {
        self.update(project_id, |job| {
            job.status = if problem.is_some() { DesignRunStatus::Failed } else { DesignRunStatus::Done };
            job.error = problem.clone();
            job.finished_at = Some(now_seconds());
        });
    }

    fn ensure_daemon(&self, install: &Install) -> Result<(), OpenDesignError> {
        let _turn = self.daemon.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.healthy() {
            return Ok(());
        }
        fs::create_dir_all(&self.data_dir).map_err(storage)?;
        let log = File::create(self.data_dir.join(DAEMON_LOG)).map_err(storage)?;
        let errors = log.try_clone().map_err(storage)?;
        let mut command = Command::new(&install.exe);
        process::hidden(&mut command);
        command
            .arg(&install.cli)
            .args(["--no-open", "--port", &OPEN_DESIGN_PORT.to_string()])
            .env("ELECTRON_RUN_AS_NODE", "1")
            .env("OD_DATA_DIR", &self.data_dir)
            .env("OD_RESOURCE_ROOT", &install.resources)
            .env("OD_INSTALLATION_DIR", &install.root)
            .current_dir(&self.data_dir)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(errors);
        command
            .spawn()
            .map_err(|error| OpenDesignError::Daemon(format!("could not start Open Design: {error}")))?;
        let deadline = Instant::now() + DAEMON_BOOT;
        while Instant::now() < deadline {
            thread::sleep(BOOT_PROBE);
            if self.healthy() {
                return Ok(());
            }
        }
        Err(OpenDesignError::Daemon("Open Design не запустився за хвилину".into()))
    }

    fn healthy(&self) -> bool {
        self.endpoint("/api/health")
            .ok()
            .and_then(|address| self.probe.get(address.as_str()).call().ok())
            .is_some_and(|response| response.status().is_success())
    }

    fn variants(&self, od_project: &str) -> Vec<DesignVariant> {
        let root = self.project_root(od_project);
        let mut files = Vec::new();
        collect_html(&root, &root, 0, &mut files);
        files.sort_by_cached_key(|file| {
            (
                variant_screen(file).unwrap_or(u32::MAX),
                DesignSource::from_file(file),
                file.clone(),
            )
        });
        files
            .into_iter()
            .take(MAX_VARIANTS)
            .filter_map(|file| {
                let preview_url = self.export_url(od_project, &file)?;
                Some(DesignVariant {
                    title: read_title(&root.join(&file)).unwrap_or_else(|| humanize(&file)),
                    screen: variant_screen(&file),
                    source: DesignSource::from_file(&file),
                    preview_url,
                    file,
                })
            })
            .collect()
    }

    fn export_url(&self, od_project: &str, file: &str) -> Option<String> {
        let mut address = self.endpoint("/").ok()?;
        address
            .path_segments_mut()
            .ok()?
            .clear()
            .extend(["api", "projects", od_project, "export"])
            .extend(file.split('/'));
        address
            .query_pairs_mut()
            .append_pair(EXPORT_QUERY_KEY, EXPORT_QUERY_VALUE);
        Some(address.into())
    }

    fn project_root(&self, od_project: &str) -> PathBuf {
        self.data_dir.join(PROJECTS_FOLDER).join(od_project)
    }

    fn endpoint(&self, path: &str) -> Result<Url, OpenDesignError> {
        Url::parse(&format!("http://{HOST}:{OPEN_DESIGN_PORT}{path}"))
            .map_err(|error| OpenDesignError::Daemon(error.to_string()))
    }

    fn get_json(&self, address: &Url) -> Result<Value, OpenDesignError> {
        let response = self.agent.get(address.as_str()).call().map_err(daemon)?;
        read_reply(response)
    }

    fn post_json(&self, path: &str, body: &Value) -> Result<Value, OpenDesignError> {
        let response = self
            .agent
            .post(self.endpoint(path)?.as_str())
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .map_err(daemon)?;
        read_reply(response)
    }

    fn job(&self, project_id: i64) -> Job {
        self.jobs().get(&project_id).cloned().unwrap_or_default()
    }

    fn update(&self, project_id: i64, change: impl FnOnce(&mut Job)) {
        change(self.jobs().entry(project_id).or_default());
        self.persist();
        self.notify();
    }

    fn persist(&self) {
        let snapshot = self.jobs().clone();
        if fs::create_dir_all(&self.data_dir).is_ok()
            && let Ok(text) = serde_json::to_string_pretty(&snapshot)
        {
            let _ = fs::write(self.data_dir.join(JOBS_FILE), text);
        }
    }

    fn notify(&self) {
        let _ = self.app.emit(CHANGED_EVENT, ());
    }

    fn jobs(&self) -> MutexGuard<'_, BTreeMap<i64, Job>> {
        self.jobs.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub fn resume_after_start(app: AppHandle) {
    thread::spawn(move || {
        thread::sleep(STARTUP_DELAY);
        let Some(open_design) = app.try_state::<Arc<OpenDesign>>().map(|state| Arc::clone(&state)) else {
            return;
        };
        let busy: Vec<(i64, DesignRunStatus)> = open_design
            .jobs()
            .iter()
            .filter(|(_, job)| job.status.is_busy())
            .map(|(project_id, job)| (*project_id, job.status))
            .collect();
        if busy.is_empty() {
            return;
        }
        let Some(install) = find_install() else {
            for (project_id, _) in busy {
                open_design.finish(project_id, Some(describe(&OpenDesignError::NotInstalled)));
            }
            return;
        };
        if let Err(problem) = open_design.ensure_daemon(&install) {
            for (project_id, _) in busy {
                open_design.finish(project_id, Some(describe(&problem)));
            }
            return;
        }
        let studio = app.try_state::<Arc<Studio>>().map(|state| Arc::clone(&state));
        for (project_id, status) in busy {
            match (status, &studio) {
                (DesignRunStatus::Running, _) => {
                    let watcher = Arc::clone(&open_design);
                    thread::spawn(move || watcher.watch(project_id));
                }
                (_, Some(studio)) => {
                    open_design.update(project_id, |job| job.status = DesignRunStatus::Idle);
                    if let Err(problem) = open_design.start(Arc::clone(&studio.store), project_id) {
                        open_design.finish(project_id, Some(describe(&problem)));
                    }
                }
                (_, None) => open_design.finish(project_id, Some("Студія ще не готова — запусти Open Design ще раз.".to_owned())),
            }
        }
    });
}

pub fn start_for_plan(app: &AppHandle, project_id: i64) {
    let (Some(open_design), Some(studio)) = (app.try_state::<Arc<OpenDesign>>(), app.try_state::<Arc<Studio>>()) else {
        return;
    };
    if find_install().is_some() {
        let _ = open_design.start(Arc::clone(&studio.store), project_id);
    }
}

fn find_install() -> Option<Install> {
    let root = PathBuf::from(std::env::var_os(LOCAL_APP_DATA)?)
        .join(INSTALL_PARENT)
        .join(INSTALL_FOLDER);
    let resources_folder = root.join(RESOURCES_FOLDER);
    let config: Value = serde_json::from_str(&fs::read_to_string(resources_folder.join(CONFIG_FILE)).ok()?).ok()?;
    let cli = resources_folder.join(config.get(CLI_ENTRY_KEY)?.as_str()?);
    let exe = root.join(APP_EXE);
    let resources = resources_folder.join(RESOURCE_ROOT_FOLDER);
    (exe.is_file() && cli.is_file() && resources.is_dir()).then_some(Install {
        root,
        exe,
        cli,
        resources,
    })
}

fn pick_design_system(detail: &ProjectDetail, round: u32) -> &'static str {
    let project = &detail.project;
    let text = format!("{} {} {} {}", project.prompt, project.pitch, project.style.mood, project.name).to_lowercase();
    let primary = DIRECTIONS
        .iter()
        .find(|(_, words)| words.iter().any(|word| text.contains(word)))
        .map_or(FALLBACK_DIRECTION, |(id, _)| *id);
    let Some(offset) = (round as usize).checked_sub(1) else {
        return primary;
    };
    ALTERNATE_DIRECTIONS
        .iter()
        .copied()
        .filter(|id| *id != primary)
        .cycle()
        .nth(offset)
        .unwrap_or(FALLBACK_DIRECTION)
}

fn candidates(references: &[Reference], source: DesignSource) -> Vec<Candidate> {
    let mut visual: Vec<&Reference> = references
        .iter()
        .filter(|reference| !reference.topic.is_audio())
        .collect();
    visual.sort_by_key(|reference| !SCREEN_TOPICS.contains(&reference.topic));
    let pools: Vec<Vec<Candidate>> = visual
        .into_iter()
        .map(|reference| match source {
            DesignSource::Pinterest => reference
                .pins
                .iter()
                .map(|pin| Candidate {
                    url: pin.sized(DETAIL_PIN_SIZE),
                    topic: reference.topic,
                    title: pin.title.clone(),
                    author: String::new(),
                })
                .collect(),
            DesignSource::Artstation => reference
                .artworks
                .iter()
                .map(|artwork| Candidate {
                    url: artwork.sized(DETAIL_ARTWORK_SIZE),
                    topic: reference.topic,
                    title: artwork.title.clone(),
                    author: artwork.author.clone(),
                })
                .collect(),
            DesignSource::Prompt | DesignSource::Mixed => Vec::new(),
        })
        .collect();
    let deepest = pools.iter().map(Vec::len).max().unwrap_or(0);
    (0..deepest)
        .flat_map(|depth| pools.iter().filter_map(move |pool| pool.get(depth).cloned()))
        .collect()
}

fn variant_count(count: usize) -> String {
    match count {
        1 => "1 варіацію".to_owned(),
        2..=4 => format!("{count} варіації"),
        _ => format!("{count} варіацій"),
    }
}

fn shelf_lines(images: &[ShelfImage]) -> String {
    images
        .iter()
        .map(|image| {
            let title = if image.title.is_empty() { "без назви" } else { image.title.as_str() };
            let author = if image.author.is_empty() { String::new() } else { format!(" — {}", image.author) };
            format!("  - {} — [{}] {title}{author}", image.file, image.topic.label())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn source_line(source: DesignSource, shelf: &Shelf) -> String {
    match source {
        DesignSource::Prompt => format!(
            "- {} — лише промпт, план і стиль гри, без картинок-референсів: власна композиція в дизайн-системі.",
            source.code()
        ),
        DesignSource::Pinterest => format!(
            "- {} — дошка Pinterest. Перш ніж малювати, переглянь інструментом Read кожну картинку нижче й бери з неї форму панелей, рамок і кнопок, іконки, композицію меню, світло, фактуру й кольори:\n{}",
            source.code(),
            shelf_lines(&shelf.pinterest)
        ),
        DesignSource::Artstation => format!(
            "- {} — дошка ArtStation. Перш ніж малювати, переглянь інструментом Read кожну картинку нижче й бери з неї рівень арту, світло, матеріали, силуети, VFX і колір:\n{}",
            source.code(),
            shelf_lines(&shelf.artstation)
        ),
        DesignSource::Mixed => format!(
            "- {} — обидві дошки разом: поєднай форму інтерфейсу з Pinterest і світло, фактуру й арт з ArtStation в одну цілісну мову оформлення.",
            source.code()
        ),
    }
}

fn brief(detail: &ProjectDetail, round: u32, design_system: &str, shelf: &Shelf) -> String {
    let project = &detail.project;
    let style = &project.style;
    let screens = detail
        .screens
        .iter()
        .take(MAX_SCREENS)
        .map(|screen| format!("{:02}. {} — {}", screen.position, screen.title, screen.summary))
        .collect::<Vec<_>>()
        .join("\n");
    let sources = shelf.sources();
    let tag = round + 1;
    let first = sources.first().copied().unwrap_or_default().code();
    let codes = sources.iter().map(|source| source.code()).collect::<Vec<_>>().join(", ");
    let lines = sources
        .iter()
        .map(|source| source_line(*source, shelf))
        .collect::<Vec<_>>()
        .join("\n");
    let task = if round == 0 {
        "зроби інші дизайни тих самих екранів — сміливіші композиції, іншу ієрархію, інакшу подачу меню, HUD і панелей, щоб було з чого вибрати."
    } else {
        "зроби ще варіації тих самих екранів у новому напрямі, не схожі на файли, які вже є в цьому проєкті."
    };
    format!(
        "Гра «{name}» — {pitch}\nРушій: {engine}.\nПромпт гравця: {prompt}\n\n\
Стиль гри: палітра {palette}; шрифти {fonts}; радіус кутів {radius} px; настрій: {mood}. \
Дизайн-система Open Design: {design_system} — бери з неї мову форм і деталей, але тримай палітру й настрій гри.\n\n\
Claude Design паралельно робить основну версію кожного екрана. Твоє завдання інше: {task}\n\n\
Екрани гри:\n{screens}\n\n\
Для кожного екрана зроби {count} — по одній на кожне джерело нижче. Кожна варіація — окремий самодостатній HTML-файл у корені проєкту з назвою \
screen-<номер екрана двома цифрами>-<код джерела>-{tag}.html, коди джерел {codes} (наприклад screen-01-{first}-{tag}.html). \
Не змінюй і не видаляй файли, які вже є.\n\n\
Джерела:\n{lines}\n\n\
Варіації одного екрана мають помітно відрізнятися між собою — кожна тримається свого джерела. \
Картинки з папки {REFERENCES_FOLDER}/ — лише орієнтир стилю: не вставляй їх в екрани й не посилайся на них у HTML.\n\
Вимоги: рівно 1280×720 px, html і body без відступів, overflow hidden; у <title> — назва екрана й джерело варіації; \
тексти інтерфейсу мовою гри, з назвами й числами з опису екрана; без JavaScript. \
Персонажів, істот, предмети й ілюстрації ніколи не малюй SVG чи CSS-фігурами — бери справжній арт або став слот арту \
<figure class=\"art-slot\" data-prompt=\"<промпт англійською>\">.\n\
Не став запитань — одразу роби всі файли.",
        name = project.name,
        pitch = project.pitch,
        engine = project.engine.label(),
        prompt = project.prompt,
        palette = style.palette.join(", "),
        fonts = style.fonts,
        radius = style.corner_radius,
        mood = style.mood,
        count = variant_count(sources.len()),
    )
}

fn collect_html(root: &Path, folder: &Path, depth: usize, files: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            if depth < SCAN_DEPTH {
                collect_html(root, &path, depth + 1, files);
            }
            continue;
        }
        let is_html = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(HTML_EXTENSION));
        if let (true, Ok(relative)) = (is_html, path.strip_prefix(root)) {
            files.push(
                relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/"),
            );
        }
    }
}

fn read_title(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_TITLE_SCAN_BYTES)]).into_owned();
    let lower = head.to_lowercase();
    let open = lower.find("<title")?;
    let start = open + lower[open..].find('>')? + 1;
    let end = start + lower[start..].find("</title>")?;
    let title = decode_entities(head.get(start..end)?.trim());
    (!title.is_empty()).then_some(title)
}

fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
}

fn humanize(file: &str) -> String {
    let name = file.rsplit('/').next().unwrap_or(file);
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    stem.replace(['-', '_'], " ")
}

fn read_reply(mut response: ureq::http::Response<ureq::Body>) -> Result<Value, OpenDesignError> {
    let status = response.status();
    let text = response
        .body_mut()
        .with_config()
        .limit(MAX_JSON_BYTES)
        .read_to_string()
        .map_err(daemon)?;
    let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(value);
    }
    let message = value["error"]["message"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
    Err(OpenDesignError::Daemon(message))
}

fn http_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

fn describe(problem: &OpenDesignError) -> String {
    match problem {
        OpenDesignError::NotInstalled => "Open Design не встановлено на цьому комп’ютері.".to_owned(),
        OpenDesignError::NotPlanned => "У гри ще немає списку екранів.".to_owned(),
        OpenDesignError::Daemon(detail) => detail.clone(),
        other => other.to_string(),
    }
}

fn daemon(error: impl std::fmt::Display) -> OpenDesignError {
    OpenDesignError::Daemon(error.to_string())
}

fn storage(error: impl std::fmt::Display) -> OpenDesignError {
    OpenDesignError::Storage(error.to_string())
}

fn studio_error(problem: StudioError) -> OpenDesignError {
    match problem {
        StudioError::NotFound => OpenDesignError::NotFound,
        other => OpenDesignError::Storage(other.to_string()),
    }
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use noctics_shared::{Artwork, GameEngine, Pin, Project, ProjectStatus, PromptAnalysis, Screen, ScreenStatus, StyleGuide};

    use super::*;

    fn detail(prompt: &str, mood: &str) -> ProjectDetail {
        ProjectDetail {
            project: Project {
                id: 7,
                name: "Ліхтарник".into(),
                prompt: prompt.into(),
                pitch: "Гра про маяк".into(),
                engine: GameEngine::Godot,
                status: ProjectStatus::Designing,
                style: StyleGuide {
                    palette: vec!["#101820".into(), "#f2aa4c".into()],
                    fonts: "Unbounded".into(),
                    corner_radius: 8,
                    mood: mood.into(),
                    ..StyleGuide::default()
                },
                references: Vec::new(),
                analysis: PromptAnalysis::default(),
                cover_screen_id: None,
                cover_html: None,
                screen_count: 1,
                approved_count: 0,
                pending_count: 0,
                error: None,
                created_at: 0,
            },
            design_document: String::new(),
            screens: vec![Screen {
                id: 1,
                project_id: 7,
                position: 1,
                title: "Головне меню".into(),
                summary: "Старт, продовження, налаштування".into(),
                status: ScreenStatus::Draft,
                html: None,
                revision: 1,
                error: None,
            }],
            outputs: Vec::new(),
        }
    }

    #[test]
    fn picks_a_design_direction_from_the_game_and_rotates_it() {
        let neon = detail("Кіберпанк-стелс у неоновому місті", "дощ");
        assert_eq!(pick_design_system(&neon, 0), "neon");
        assert_eq!(pick_design_system(&neon, 1), "dramatic");
        assert_eq!(pick_design_system(&neon, 2), "bold");
        let plain = detail("Головоломка з кубиками", "спокій");
        assert_eq!(pick_design_system(&plain, 0), FALLBACK_DIRECTION);
        assert_eq!(pick_design_system(&plain, 1), "bold");
    }

    fn reference(topic: ReferenceTopic, pins: &[&str], artworks: &[&str]) -> Reference {
        Reference {
            topic,
            title: topic.label().into(),
            note: String::new(),
            query: String::new(),
            palette: Vec::new(),
            pins: pins
                .iter()
                .map(|name| Pin {
                    id: String::new(),
                    image: format!("https://i.pinimg.com/474x/aa/{name}.jpg"),
                    title: (*name).into(),
                })
                .collect(),
            artworks: artworks
                .iter()
                .map(|name| Artwork {
                    id: (*name).into(),
                    image: format!("https://cdna.artstation.com/p/assets/images/images/000/000/001/smaller_square/{name}.jpg"),
                    title: (*name).into(),
                    author: "Ада".into(),
                })
                .collect(),
        }
    }

    fn image(file: &str, topic: ReferenceTopic, author: &str) -> ShelfImage {
        ShelfImage {
            file: file.into(),
            topic,
            title: "Кам’яне меню".into(),
            author: author.into(),
        }
    }

    #[test]
    fn picks_screen_topics_first_and_rotates_through_the_board() {
        let board = vec![
            reference(ReferenceTopic::World, &["world-1", "world-2"], &["w"]),
            reference(ReferenceTopic::Sounds, &["sound"], &[]),
            reference(ReferenceTopic::MainMenu, &["menu-1", "menu-2", "menu-3"], &[]),
        ];
        let pins: Vec<String> = candidates(&board, DesignSource::Pinterest)
            .into_iter()
            .map(|candidate| candidate.title)
            .collect();
        assert_eq!(pins, ["menu-1", "world-1", "menu-2", "world-2", "menu-3"]);
        let artworks = candidates(&board, DesignSource::Artstation);
        assert_eq!(artworks.len(), 1);
        assert!(artworks[0].url.contains("/medium/"));
        assert_eq!(artworks[0].author, "Ада");
        assert!(candidates(&board, DesignSource::Pinterest)[0].url.contains("/736x/"));
        assert!(candidates(&board, DesignSource::Mixed).is_empty());
    }

    #[test]
    fn offers_only_the_sources_that_have_pictures() {
        let empty = Shelf::default();
        assert_eq!(empty.sources(), [DesignSource::Prompt]);
        assert_eq!(empty.skipped(), [DesignSource::Pinterest, DesignSource::Artstation, DesignSource::Mixed]);
        let pinterest_only = Shelf {
            pinterest: vec![image("a.jpg", ReferenceTopic::MainMenu, "")],
            artstation: Vec::new(),
        };
        assert_eq!(pinterest_only.sources(), [DesignSource::Prompt, DesignSource::Pinterest]);
        let both = Shelf {
            pinterest: vec![image("a.jpg", ReferenceTopic::MainMenu, "")],
            artstation: vec![image("b.jpg", ReferenceTopic::World, "Ада")],
        };
        assert_eq!(both.sources(), DesignSource::ALL);
        assert!(both.skipped().is_empty());
    }

    #[test]
    fn writes_a_brief_with_one_variant_per_source() {
        let shelf = Shelf {
            pinterest: vec![image("noctics-references/pinterest/01-main-menu.jpg", ReferenceTopic::MainMenu, "")],
            artstation: vec![image("noctics-references/artstation/01-world.jpg", ReferenceTopic::World, "Ада")],
        };
        let message = brief(&detail("Фентезі про маяк", "тепло"), 1, "fantasy", &shelf);
        assert!(message.contains("01. Головне меню — Старт, продовження, налаштування"));
        assert!(message.contains("зроби 4 варіації — по одній на кожне джерело"));
        assert!(message.contains("коди джерел prompt, pinterest, artstation, mix (наприклад screen-01-prompt-2.html)"));
        assert!(message.contains("  - noctics-references/pinterest/01-main-menu.jpg — [Головні меню] Кам’яне меню"));
        assert!(message.contains("  - noctics-references/artstation/01-world.jpg — [Світ] Кам’яне меню — Ада"));
        assert!(message.contains("Дизайн-система Open Design: fantasy"));
        assert!(message.contains("Рушій: Godot"));
        let plain = brief(&detail("Фентезі про маяк", "тепло"), 0, "fantasy", &Shelf::default());
        assert!(plain.contains("зроби 1 варіацію"));
        assert!(plain.contains("screen-01-prompt-1.html"));
        assert!(!plain.contains("- pinterest —"));
    }

    #[test]
    fn reads_titles_and_names_of_variant_files() {
        let folder = std::env::temp_dir().join(format!("noctics-od-test-{}", std::process::id()));
        fs::create_dir_all(folder.join(".od-frames")).unwrap();
        fs::create_dir_all(folder.join("extra")).unwrap();
        fs::write(folder.join("screen-02-a.html"), "<html><head><TITLE> Меню &amp; HUD — A </TITLE></head></html>").unwrap();
        fs::write(folder.join("extra").join("screen-01-b.html"), "<html></html>").unwrap();
        fs::write(folder.join(".od-frames").join("hidden.html"), "<html></html>").unwrap();
        fs::write(folder.join("notes.md"), "#").unwrap();
        let mut files = Vec::new();
        collect_html(&folder, &folder, 0, &mut files);
        files.sort();
        assert_eq!(files, ["extra/screen-01-b.html", "screen-02-a.html"]);
        assert_eq!(read_title(&folder.join("screen-02-a.html")).as_deref(), Some("Меню & HUD — A"));
        assert_eq!(read_title(&folder.join("extra").join("screen-01-b.html")), None);
        assert_eq!(humanize("extra/screen-01-b.html"), "screen 01 b");
        let _ = fs::remove_dir_all(&folder);
    }
}
