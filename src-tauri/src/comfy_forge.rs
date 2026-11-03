use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use noctics_shared::{
    AssetDraft, AssetKind, AssetTask, COMFY_CHANGED_EVENT, ComfyError, ComfyModel, ComfyOverview,
    ComfyStatus, Medium, ModelRun, RosterSource, RunFile, RunStatus, StudioError, TaskOrigin,
    TaskStatus,
};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::claude_code::{Completion, Effort};
use crate::comfy::{ComfyClient, Install, Inventory, OutputRef, Progress, ServerInfo, locate_install};
use crate::comfy_prompts::{self, ProjectStyle};
use crate::comfy_recipes::{self, Aspect, Brief, Candidate};
use crate::comfy_store::{ComfyStore, NewRun, NewTask, Plan, StoredFile, StoredRun, StoredTask, WritingJob};
use crate::studio::Studio;

const WATCH_INTERVAL: Duration = Duration::from_secs(4);
const INVENTORY_INTERVAL: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const RUN_TIMEOUT: Duration = Duration::from_secs(45 * 60);
const LAUNCH_GRACE: Duration = Duration::from_secs(4 * 60);
const OFFLINE_POLLS: u32 = 20;
const LOST_POLLS: u32 = 6;
const TASK_LIMIT: usize = 40;
const OUTPUT_ROOT: &str = "noctics";
const SEED_RANGE: u64 = 1 << 31;
const SAFE_NAME_CHARS: &str = "._-";

#[derive(Clone, Default)]
struct Watch {
    online: bool,
    info: Option<ServerInfo>,
    source: Option<RosterSource>,
    roster: Vec<Candidate>,
    matting: Option<String>,
    install: Option<Install>,
    launching_since: Option<Instant>,
}

enum Outcome {
    Finished(Vec<OutputRef>),
    Failed(String),
    Offline,
    Deleted,
}

enum Flow {
    Settled,
    Offline,
    Deleted,
}

pub struct Forge {
    store: Arc<ComfyStore>,
    studio: Arc<Studio>,
    app: AppHandle,
    client: ComfyClient,
    gallery: PathBuf,
    launch_log: PathBuf,
    watch: Mutex<Watch>,
    media_base: Mutex<String>,
    writer: AtomicBool,
    renderer: AtomicBool,
    active: Mutex<Option<(i64, String)>>,
}

impl Forge {
    pub fn new(
        store: Arc<ComfyStore>,
        studio: Arc<Studio>,
        app: AppHandle,
        comfy_url: &str,
        gallery: PathBuf,
        launch_log: PathBuf,
    ) -> Self {
        Self {
            store,
            studio,
            app,
            client: ComfyClient::new(comfy_url),
            gallery,
            launch_log,
            watch: Mutex::new(Watch::default()),
            media_base: Mutex::new(String::new()),
            writer: AtomicBool::new(false),
            renderer: AtomicBool::new(false),
            active: Mutex::new(None),
        }
    }

    pub fn start(self: &Arc<Self>) {
        let _ = self.store.recover();
        let forge = Arc::clone(self);
        thread::spawn(move || forge.watch_forever());
        self.kick();
    }

    pub fn notify(&self) {
        let _ = self.app.emit(COMFY_CHANGED_EVENT, ());
    }

    pub fn set_media_base(&self, base: String) {
        if let Ok(mut slot) = self.media_base.lock() {
            *slot = base;
        }
        self.notify();
    }

    fn media_base(&self) -> String {
        self.media_base.lock().map(|base| base.clone()).unwrap_or_default()
    }

    fn snapshot(&self) -> Watch {
        self.watch.lock().map(|watch| watch.clone()).unwrap_or_default()
    }

    fn online(&self) -> bool {
        self.watch.lock().is_ok_and(|watch| watch.online)
    }

    fn watch_forever(self: Arc<Self>) {
        let mut refreshed: Option<Instant> = None;
        loop {
            let info = self.client.probe();
            let online = info.is_some();
            let before = self.public_view();
            let was_online = self.online();
            let stale = refreshed.is_none_or(|at| at.elapsed() >= INVENTORY_INTERVAL) || online != was_online;
            let fresh = stale.then(|| self.take_inventory(online));
            if let Ok(mut watch) = self.watch.lock() {
                watch.online = online;
                watch.info = info;
                if let Some((inventory, source, install)) = fresh {
                    watch.roster = comfy_recipes::roster(&inventory);
                    watch.matting = comfy_recipes::matting_model(&inventory);
                    watch.source = Some(source);
                    watch.install = install;
                    refreshed = Some(Instant::now());
                }
                if online || watch.launching_since.is_some_and(|since| since.elapsed() > LAUNCH_GRACE) {
                    watch.launching_since = None;
                }
            }
            if self.public_view() != before {
                self.notify();
            }
            if online && !was_online {
                self.kick();
            }
            thread::sleep(WATCH_INTERVAL);
        }
    }

    fn take_inventory(&self, online: bool) -> (Inventory, RosterSource, Option<Install>) {
        let install = locate_install();
        if online && let Some(inventory) = self.client.inventory() {
            return (inventory, RosterSource::Server, install);
        }
        match &install {
            Some(found) => (found.scan(), RosterSource::Disk, install),
            None => (Inventory::default(), RosterSource::Unknown, install),
        }
    }

    fn public_view(&self) -> (ComfyStatus, Vec<ComfyModel>) {
        let disabled = self.store.disabled().unwrap_or_default();
        let watch = self.snapshot();
        (self.status_of(&watch), models_of(&watch.roster, &disabled))
    }

    fn status_of(&self, watch: &Watch) -> ComfyStatus {
        ComfyStatus {
            online: watch.online,
            url: self.client.url(),
            version: watch.info.as_ref().and_then(|info| info.version.clone()),
            gpu: watch.info.as_ref().and_then(|info| info.gpu.clone()),
            vram_mb: watch.info.as_ref().and_then(|info| info.vram_mb),
            source: watch.source.unwrap_or(RosterSource::Unknown),
            launchable: !watch.online
                && watch
                    .install
                    .as_ref()
                    .is_some_and(|install| install.python.is_some()),
            launching: !watch.online && watch.launching_since.is_some(),
            media_base: self.media_base(),
        }
    }

    pub fn overview(&self) -> Result<ComfyOverview, ComfyError> {
        let (status, models) = self.public_view();
        let base = status.media_base.clone();
        let tasks = self
            .store
            .list(TASK_LIMIT)?
            .into_iter()
            .map(|task| public_task(task, &base))
            .collect();
        Ok(ComfyOverview {
            status,
            models,
            tasks,
        })
    }

    pub fn task(&self, task_id: i64) -> Result<AssetTask, ComfyError> {
        let base = self.media_base();
        self.store
            .task(task_id)?
            .map(|task| public_task(task, &base))
            .ok_or(ComfyError::NotFound)
    }

    pub fn models(&self) -> Vec<ComfyModel> {
        self.public_view().1
    }

    pub fn create(
        self: &Arc<Self>,
        draft: &AssetDraft,
        origin: TaskOrigin,
        plan: Option<Plan>,
    ) -> Result<Vec<i64>, ComfyError> {
        let draft = draft.normalized()?;
        let project = draft.project_id.and_then(|id| {
            self.studio
                .store
                .reference_context(id)
                .ok()
                .map(|context| (id, context.game_name))
        });
        let disabled = self.store.disabled()?;
        let roster = self.snapshot().roster;
        let seed = fresh_seed();
        let mut created = Vec::new();
        for (index, kind) in draft.kinds.iter().copied().enumerate() {
            let runs = plan_runs(&roster, &disabled, kind);
            if runs.is_empty() {
                continue;
            }
            let settled = plan
                .clone()
                .map(|plan| comfy_prompts::settle(plan, kind, &draft.brief));
            let task_id = self.store.create(&NewTask {
                kind,
                brief: &draft.brief,
                origin,
                project: project.clone(),
                plan: settled,
                seed: (seed + index as u64 * 7919) % SEED_RANGE,
                runs,
            })?;
            created.push(task_id);
        }
        if created.is_empty() {
            return Err(ComfyError::NoModels);
        }
        self.notify();
        self.kick();
        Ok(created)
    }

    pub fn retry(self: &Arc<Self>, run_id: i64) -> Result<(), ComfyError> {
        self.store.retry_run(run_id)?;
        self.notify();
        self.kick();
        Ok(())
    }

    pub fn delete(&self, task_id: i64) -> Result<(), ComfyError> {
        self.store.delete_task(task_id)?;
        let running = self
            .active
            .lock()
            .ok()
            .and_then(|active| active.clone())
            .filter(|(active_task, _)| *active_task == task_id);
        if let Some((_, prompt_id)) = running {
            self.client.interrupt(&prompt_id);
        }
        let _ = std::fs::remove_dir_all(self.gallery.join(task_id.to_string()));
        self.notify();
        Ok(())
    }

    pub fn set_enabled(&self, model_id: &str, enabled: bool) -> Result<(), ComfyError> {
        self.store.set_enabled(model_id, enabled)?;
        self.notify();
        Ok(())
    }

    pub fn launch(&self) -> Result<(), ComfyError> {
        let watch = self.snapshot();
        if watch.online {
            return Ok(());
        }
        if watch
            .launching_since
            .is_some_and(|since| since.elapsed() < LAUNCH_GRACE)
        {
            return Ok(());
        }
        let install = locate_install().ok_or(ComfyError::NotInstalled)?;
        if install.python.is_none() {
            return Err(ComfyError::NotInstalled);
        }
        install.launch(&self.launch_log).map_err(ComfyError::Launch)?;
        if let Ok(mut watch) = self.watch.lock() {
            watch.launching_since = Some(Instant::now());
        }
        self.notify();
        Ok(())
    }

    pub fn file_path(&self, run_id: i64, index: usize) -> Result<PathBuf, ComfyError> {
        let run = self.store.run(run_id)?;
        let file = run.files.get(index).ok_or(ComfyError::NotFound)?;
        let path = self.gallery.join(run.task_id.to_string()).join(&file.name);
        if path.is_file() {
            Ok(path)
        } else {
            Err(ComfyError::NotFound)
        }
    }

    pub fn served_file(&self, task_id: &str, name: &str) -> Option<PathBuf> {
        let valid_task = !task_id.is_empty() && task_id.bytes().all(|byte| byte.is_ascii_digit());
        let valid_name = !name.is_empty()
            && !name.starts_with('.')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || SAFE_NAME_CHARS.contains(c));
        if !(valid_task && valid_name) {
            return None;
        }
        let path = self.gallery.join(task_id).join(name);
        path.is_file().then_some(path)
    }

    pub fn kick(self: &Arc<Self>) {
        if self.store.has_status(TaskStatus::Writing).unwrap_or(false)
            && !self.writer.swap(true, Ordering::SeqCst)
        {
            let forge = Arc::clone(self);
            thread::spawn(move || {
                forge.write_prompts();
                forge.writer.store(false, Ordering::SeqCst);
                forge.kick();
            });
        }
        if self.online()
            && self.store.has_status(TaskStatus::Queued).unwrap_or(false)
            && !self.renderer.swap(true, Ordering::SeqCst)
        {
            let forge = Arc::clone(self);
            thread::spawn(move || {
                forge.render();
                forge.renderer.store(false, Ordering::SeqCst);
                if forge.online() {
                    forge.kick();
                }
            });
        }
    }

    fn write_prompts(self: &Arc<Self>) {
        while let Ok(Some(job)) = self.store.next_writing() {
            let (plan, note) = self.compose(&job);
            if self.store.set_plan(job.task_id, &plan, note.as_deref()).is_err() {
                break;
            }
            self.notify();
            self.kick();
        }
    }

    fn compose(&self, job: &WritingJob) -> (Plan, Option<String>) {
        let fallback = || comfy_prompts::fallback(job.kind, &job.brief);
        if let Err(problem) = self.studio.require_connection() {
            return (
                fallback(),
                Some(format!("{} — промпт складено з опису без Claude.", claude_problem(&problem))),
            );
        }
        let context = job
            .project_id
            .and_then(|id| self.studio.store.reference_context(id).ok());
        let style = context.as_ref().map(|context| ProjectStyle {
            name: &context.game_name,
            pitch: &context.pitch,
            style: &context.style,
        });
        let completion = Completion {
            system: comfy_prompts::system_prompt(),
            user: comfy_prompts::message(job.kind, &job.brief, style.as_ref()),
            effort: Effort::Medium,
            schema: Some(comfy_prompts::schema()),
        };
        match self.studio.claude().complete(&completion) {
            Ok(answer) => match comfy_prompts::parse(&answer, job.kind, &job.brief) {
                Some(plan) => (plan, None),
                None => (
                    fallback(),
                    Some("Claude відповів незрозуміло — промпт складено з опису.".to_owned()),
                ),
            },
            Err(problem) => (
                fallback(),
                Some(format!("{} — промпт складено з опису без Claude.", claude_problem(&problem))),
            ),
        }
    }

    fn render(self: &Arc<Self>) {
        let mut family: Option<&'static str> = None;
        while self.online() {
            let task = match self.store.claim_queued() {
                Ok(Some(task)) => task,
                _ => break,
            };
            self.notify();
            match self.render_task(&task, &mut family) {
                Flow::Settled => {
                    let _ = self.store.settle_task(task.id);
                }
                Flow::Offline => {
                    let _ = self.store.requeue_task(task.id);
                    if let Ok(mut watch) = self.watch.lock() {
                        watch.online = false;
                    }
                    self.notify();
                    break;
                }
                Flow::Deleted => {}
            }
            self.notify();
        }
    }

    fn render_task(&self, task: &StoredTask, family: &mut Option<&'static str>) -> Flow {
        let plan = comfy_prompts::settle(
            task.plan
                .clone()
                .unwrap_or_else(|| comfy_prompts::fallback(task.kind, &task.brief)),
            task.kind,
            &task.brief,
        );
        let watch = self.snapshot();
        let mut reference = task
            .runs
            .iter()
            .filter(|run| run.reference && run.status == RunStatus::Done)
            .find_map(|run| run.files.first())
            .map(|file| file.source.annotated());
        for run in task.runs.iter().filter(|run| run.status == RunStatus::Queued) {
            if !self.store.exists(task.id).unwrap_or(true) {
                return Flow::Deleted;
            }
            if !self.online() {
                return Flow::Offline;
            }
            let Some(candidate) = watch.roster.iter().find(|candidate| candidate.id == run.model_id) else {
                let _ = self.store.skip_run(run.id, "Модель більше не знайдено в ComfyUI.");
                self.notify();
                continue;
            };
            let Some(recipe) = candidate.recipe.as_ref() else {
                let _ = self
                    .store
                    .skip_run(run.id, &format!("Бракує: {}.", candidate.missing.join(", ")));
                self.notify();
                continue;
            };
            if candidate.medium == Medium::Mesh && reference.is_none() {
                let _ = self
                    .store
                    .skip_run(run.id, "Немає референсу — картинка для 3D не згенерувалась.");
                self.notify();
                continue;
            }
            if family.is_some_and(|previous| previous != recipe.family()) {
                self.client.free_memory();
            }
            *family = Some(recipe.family());
            let _ = self.store.start_run(run.id);
            self.notify();
            let prefix = output_prefix(task.id, run);
            let brief = Brief {
                kind: if run.reference { AssetKind::Model } else { task.kind },
                prompt: &plan.prompt,
                tags: &plan.tags,
                negative: &plan.negative,
                aspect: if run.reference {
                    Aspect::Square
                } else {
                    Aspect::from_code(&plan.aspect)
                },
                seconds: plan.seconds,
                bpm: plan.bpm,
                keyscale: &plan.keyscale,
                lyrics: &plan.lyrics,
                seed: task.seed,
                prefix: &prefix,
                matting: watch.matting.as_deref(),
                reference: reference.as_deref(),
            };
            let graph = comfy_recipes::graph(recipe, &brief);
            match self.execute(task.id, &graph) {
                Outcome::Finished(outputs) => {
                    let files = self.collect(task.id, run.id, &outputs);
                    match files {
                        Ok(files) if !files.is_empty() => {
                            if run.reference {
                                reference = files.first().map(|file| file.source.annotated());
                            }
                            let _ = self.store.finish_run(run.id, &files);
                        }
                        Ok(_) => {
                            let _ = self.store.fail_run(run.id, "ComfyUI нічого не зберіг.");
                        }
                        Err(problem) => {
                            let _ = self
                                .store
                                .fail_run(run.id, &format!("Не вдалося забрати результат: {problem}"));
                        }
                    }
                }
                Outcome::Failed(message) => {
                    let _ = self.store.fail_run(run.id, &message);
                }
                Outcome::Offline => {
                    let _ = self.store.requeue_run(run.id);
                    return Flow::Offline;
                }
                Outcome::Deleted => return Flow::Deleted,
            }
            self.notify();
        }
        Flow::Settled
    }

    fn execute(&self, task_id: i64, graph: &Value) -> Outcome {
        let prompt_id = match self.client.submit(graph) {
            Ok(prompt_id) => prompt_id,
            Err(message) => {
                return if self.client.probe().is_some() {
                    Outcome::Failed(message)
                } else {
                    Outcome::Offline
                };
            }
        };
        if let Ok(mut active) = self.active.lock() {
            *active = Some((task_id, prompt_id.clone()));
        }
        let outcome = self.follow(task_id, &prompt_id);
        if let Ok(mut active) = self.active.lock() {
            *active = None;
        }
        outcome
    }

    fn follow(&self, task_id: i64, prompt_id: &str) -> Outcome {
        let started = Instant::now();
        let mut unreachable = 0;
        let mut lost = 0;
        loop {
            thread::sleep(POLL_INTERVAL);
            if !self.store.exists(task_id).unwrap_or(true) {
                self.client.interrupt(prompt_id);
                return Outcome::Deleted;
            }
            if started.elapsed() > RUN_TIMEOUT {
                self.client.interrupt(prompt_id);
                return Outcome::Failed("ComfyUI не впорався за 45 хвилин.".to_owned());
            }
            match self.client.progress(prompt_id) {
                Ok(Progress::Finished(outputs)) => return Outcome::Finished(outputs),
                Ok(Progress::Failed(message)) => return Outcome::Failed(message),
                Ok(Progress::Waiting) => {
                    unreachable = 0;
                    lost = if self.client.queued(prompt_id) == Some(false) {
                        lost + 1
                    } else {
                        0
                    };
                    if lost >= LOST_POLLS {
                        return Outcome::Failed("ComfyUI загубив задачу — спробуй ще раз.".to_owned());
                    }
                }
                Err(_) => {
                    unreachable += 1;
                    if unreachable >= OFFLINE_POLLS {
                        return Outcome::Offline;
                    }
                }
            }
        }
    }

    fn collect(&self, task_id: i64, run_id: i64, outputs: &[OutputRef]) -> Result<Vec<StoredFile>, String> {
        let folder = self.gallery.join(task_id.to_string());
        outputs
            .iter()
            .enumerate()
            .filter_map(|(index, output)| {
                let extension = output.extension();
                let medium = medium_of(&extension)?;
                Some((index, output, extension, medium))
            })
            .map(|(index, output, extension, medium)| {
                let name = format!("{run_id}-{index}.{extension}");
                self.client.download(output, &folder.join(&name))?;
                Ok(StoredFile {
                    name,
                    medium,
                    source: output.clone(),
                })
            })
            .collect()
    }
}

fn plan_runs(roster: &[Candidate], disabled: &HashSet<String>, kind: AssetKind) -> Vec<NewRun> {
    let usable = |medium: Medium| {
        roster
            .iter()
            .filter(move |candidate| candidate.medium == medium && candidate.ready() && !disabled.contains(&candidate.id))
    };
    let entry = |candidate: &Candidate, reference: bool| NewRun {
        model_id: candidate.id.clone(),
        model_name: candidate.name.clone(),
        reference,
    };
    match kind.medium() {
        Medium::Mesh => {
            let meshes: Vec<NewRun> = usable(Medium::Mesh).map(|candidate| entry(candidate, false)).collect();
            let painter = usable(Medium::Image).next().or_else(|| {
                roster
                    .iter()
                    .find(|candidate| candidate.medium == Medium::Image && candidate.ready())
            });
            match painter {
                Some(painter) if !meshes.is_empty() => std::iter::once(entry(painter, true)).chain(meshes).collect(),
                _ => Vec::new(),
            }
        }
        medium => usable(medium).map(|candidate| entry(candidate, false)).collect(),
    }
}

fn models_of(roster: &[Candidate], disabled: &HashSet<String>) -> Vec<ComfyModel> {
    roster
        .iter()
        .map(|candidate| ComfyModel {
            id: candidate.id.clone(),
            name: candidate.name.clone(),
            note: candidate.note.clone(),
            medium: candidate.medium,
            ready: candidate.ready(),
            missing: candidate.missing.clone(),
            enabled: !disabled.contains(&candidate.id),
        })
        .collect()
}

fn public_task(task: StoredTask, base: &str) -> AssetTask {
    let prompt = task
        .plan
        .as_ref()
        .map(|plan| match task.kind.medium() {
            Medium::Audio if !plan.tags.is_empty() => plan.tags.clone(),
            _ => plan.prompt.clone(),
        })
        .unwrap_or_default();
    let title = if task.title.is_empty() {
        task.brief.clone()
    } else {
        task.title.clone()
    };
    AssetTask {
        id: task.id,
        kind: task.kind,
        brief: task.brief,
        title,
        prompt,
        origin: task.origin,
        project_name: task.project_name,
        status: task.status,
        note: task.note,
        created_at: task.created_at,
        runs: task
            .runs
            .into_iter()
            .map(|run| public_run(run, base))
            .collect(),
    }
}

fn public_run(run: StoredRun, base: &str) -> ModelRun {
    let files = run
        .files
        .iter()
        .map(|file| RunFile {
            name: file.name.clone(),
            medium: file.medium,
            url: format!(
                "{base}/files/{}/{}?v={}",
                run.task_id,
                file.name,
                run.finished_at.unwrap_or_default()
            ),
        })
        .collect();
    ModelRun {
        id: run.id,
        model_id: run.model_id,
        model_name: run.model_name,
        reference: run.reference,
        status: run.status,
        files,
        error: run.error,
        started_at: run.started_at,
        finished_at: run.finished_at,
    }
}

fn output_prefix(task_id: i64, run: &StoredRun) -> String {
    let slug: String = run
        .model_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    let role = if run.reference { "_ref" } else { "" };
    format!("{OUTPUT_ROOT}/{task_id}/{}{role}", slug.trim_matches('_'))
}

fn medium_of(extension: &str) -> Option<Medium> {
    match extension {
        "png" | "jpg" | "jpeg" | "webp" => Some(Medium::Image),
        "glb" | "gltf" => Some(Medium::Mesh),
        "flac" | "mp3" | "wav" | "ogg" | "opus" | "m4a" => Some(Medium::Audio),
        _ => None,
    }
}

fn fresh_seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    u64::try_from(nanos % u128::from(SEED_RANGE)).unwrap_or_default()
}

fn claude_problem(problem: &StudioError) -> &'static str {
    match problem {
        StudioError::ClaudeCodeMissing => "Claude Code не встановлено",
        StudioError::ClaudeSignedOut => "AI-ядро не підключене",
        StudioError::UsageLimit => "Ліміт підписки Claude вичерпано",
        StudioError::RateLimited => "Забагато запитів до Claude",
        StudioError::Overloaded => "Claude перевантажений",
        _ => "Claude не відповів",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, medium: Medium, ready: bool) -> Candidate {
        Candidate {
            id: id.into(),
            name: id.to_uppercase(),
            note: String::new(),
            medium,
            rank: 0,
            recipe: ready.then(|| comfy_recipes::Recipe::Checkpoint {
                file: format!("{id}.safetensors"),
                xl: true,
            }),
            missing: Vec::new(),
        }
    }

    #[test]
    fn three_d_tasks_start_with_a_reference_image() {
        let roster = vec![
            candidate("zimage", Medium::Image, true),
            candidate("flux", Medium::Image, true),
            candidate("trellis", Medium::Mesh, true),
            candidate("pixal", Medium::Mesh, false),
            candidate("ace", Medium::Audio, true),
        ];
        let mut disabled = HashSet::new();
        let ids = |runs: Vec<NewRun>| -> Vec<(String, bool)> {
            runs.into_iter().map(|run| (run.model_id, run.reference)).collect()
        };
        assert_eq!(
            ids(plan_runs(&roster, &disabled, AssetKind::Model)),
            vec![("zimage".into(), true), ("trellis".into(), false)]
        );
        assert_eq!(
            ids(plan_runs(&roster, &disabled, AssetKind::Sprite)),
            vec![("zimage".into(), false), ("flux".into(), false)]
        );
        disabled.insert("zimage".to_owned());
        assert_eq!(
            ids(plan_runs(&roster, &disabled, AssetKind::Model)),
            vec![("flux".into(), true), ("trellis".into(), false)]
        );
        disabled.insert("trellis".to_owned());
        assert!(plan_runs(&roster, &disabled, AssetKind::Model).is_empty());
        assert_eq!(ids(plan_runs(&roster, &disabled, AssetKind::Sound)).len(), 1);
    }

    fn wait_online(client: &ComfyClient) -> bool {
        let deadline = Instant::now() + Duration::from_secs(240);
        while Instant::now() < deadline {
            if client.probe().is_some() {
                return true;
            }
            thread::sleep(Duration::from_secs(2));
        }
        false
    }

    fn finish(client: &ComfyClient, prompt_id: &str) -> Vec<OutputRef> {
        let deadline = Instant::now() + Duration::from_secs(900);
        while Instant::now() < deadline {
            match client.progress(prompt_id).unwrap() {
                Progress::Finished(outputs) => return outputs,
                Progress::Failed(message) => panic!("ComfyUI failed: {message}"),
                Progress::Waiting => thread::sleep(Duration::from_secs(1)),
            }
        }
        panic!("ComfyUI did not finish {prompt_id}");
    }

    fn live_brief<'a>(kind: AssetKind, prefix: &'a str, matting: Option<&'a str>, reference: Option<&'a str>) -> Brief<'a> {
        Brief {
            kind,
            prompt: "Game sprite of a glowing blue mana crystal, faceted, centered, plain light background, no shadow",
            tags: "magic crystal chime, sparkling shimmer, sound effect, one shot, no music",
            negative: "",
            aspect: Aspect::Square,
            seconds: 3.0,
            bpm: 120,
            keyscale: "C minor",
            lyrics: "[Instrumental]",
            seed: 7,
            prefix,
            matting,
            reference,
        }
    }

    #[test]
    #[ignore = "launches the local ComfyUI and spends GPU time"]
    fn live_graphs_run_on_the_local_comfyui() {
        let client = ComfyClient::new(crate::comfy::COMFY_URL);
        if client.probe().is_none() {
            let install = locate_install().expect("a local ComfyUI install");
            install
                .launch(&std::env::temp_dir().join("noctics-comfy-live.log"))
                .expect("ComfyUI launches");
        }
        assert!(wait_online(&client), "ComfyUI did not come online");
        let inventory = client.inventory().expect("object_info");
        let roster = comfy_recipes::roster(&inventory);
        for candidate in &roster {
            println!("{} ready={} missing={:?}", candidate.name, candidate.ready(), candidate.missing);
        }
        let matting = comfy_recipes::matting_model(&inventory);
        assert!(matting.is_some(), "background removal is available");
        let reference = "vidpusty/ghost/front_s1_00001_.png [output]";

        for candidate in roster.iter().filter(|candidate| candidate.ready()) {
            let kind = match candidate.medium {
                Medium::Image => AssetKind::Sprite,
                Medium::Mesh => AssetKind::Model,
                Medium::Audio => AssetKind::Sound,
            };
            let prefix = format!("noctics/live/{}", candidate.rank);
            let graph = comfy_recipes::graph(
                candidate.recipe.as_ref().unwrap(),
                &live_brief(kind, &prefix, matting.as_deref(), Some(reference)),
            );
            let prompt_id = client
                .submit(&graph)
                .unwrap_or_else(|message| panic!("{} graph rejected: {message}", candidate.name));
            client.interrupt(&prompt_id);
            println!("{} graph accepted", candidate.name);
            thread::sleep(Duration::from_secs(2));
        }

        let folder = std::env::temp_dir().join("noctics-comfy-live");
        let runs = [
            ("z-image:", AssetKind::Sprite),
            ("flux2:", AssetKind::Concept),
            ("ace-aio:", AssetKind::Sound),
        ];
        for (prefix_id, kind) in runs {
            let candidate = roster
                .iter()
                .find(|candidate| candidate.id.starts_with(prefix_id) && candidate.ready())
                .unwrap_or_else(|| panic!("{prefix_id} model is ready"));
            let prefix = format!("noctics/live/{}", kind.code());
            let graph = comfy_recipes::graph(
                candidate.recipe.as_ref().unwrap(),
                &live_brief(kind, &prefix, matting.as_deref(), None),
            );
            let started = Instant::now();
            let prompt_id = client.submit(&graph).unwrap();
            let outputs = finish(&client, &prompt_id);
            assert!(!outputs.is_empty(), "{} saved nothing", candidate.name);
            let target = folder.join(format!("{}.{}", kind.code(), outputs[0].extension()));
            let size = client.download(&outputs[0], &target).unwrap();
            println!("{} → {} ({size} bytes, {:?})", candidate.name, target.display(), started.elapsed());
            if kind == AssetKind::Sprite {
                let bytes = std::fs::read(&target).unwrap();
                assert_eq!(bytes[25], 6, "sprite PNG carries an alpha channel");
            }
        }
    }

    #[test]
    fn names_outputs_and_media() {
        let run = StoredRun {
            id: 9,
            task_id: 4,
            model_id: "flux2:x".into(),
            model_name: "FLUX.2 Klein 4B".into(),
            reference: true,
            status: RunStatus::Queued,
            files: Vec::new(),
            error: None,
            started_at: None,
            finished_at: None,
        };
        assert_eq!(output_prefix(4, &run), "noctics/4/flux_2_klein_4b_ref");
        assert_eq!(medium_of("glb"), Some(Medium::Mesh));
        assert_eq!(medium_of("flac"), Some(Medium::Audio));
        assert_eq!(medium_of("txt"), None);
        assert!(fresh_seed() < SEED_RANGE);
    }
}
