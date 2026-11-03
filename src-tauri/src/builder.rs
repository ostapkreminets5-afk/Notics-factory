use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctics_shared::{
    BuildError, BuildOverview, BuildStatus, BuildTarget, GameSource, JournalEvent,
    MAX_BUILD_REQUEST_CHARS, MAX_SOURCE_PATH_CHARS, Project, SourceOrigin, SourceStatus, StudioError,
    TargetReport, format_size, project_marker,
};
use tauri::{AppHandle, Emitter};

use crate::build_store::{BuildStore, MAX_ACTIVITY, StoredBuild};
use crate::claude_code;
use crate::codegen::{self, Assignment, Event};
use crate::games_root::GamesRoot;
use crate::process::{self, LineSink, RunFailure};
use crate::recipes::{self, Context, GameNames, StepFailure};
use crate::studio::Studio;
use crate::toolchains::Toolchains;

pub const BUILDS_CHANGED: &str = "builds-changed";
const BUILD_TIMEOUT: Duration = Duration::from_secs(3 * 60 * 60);
const GENERATION_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);
const FLUSH_INTERVAL: Duration = Duration::from_millis(800);
const TOOLCHAIN_TTL: Duration = Duration::from_secs(30);
const TAIL_LINES: usize = 60;
const FIX_LOG_LINES: usize = 40;
const MAX_LINE_CHARS: usize = 400;
const MAX_CLUE_CHARS: usize = 200;
const ERROR_MARKER: &str = "error";
const STEP_MARKER: char = '▸';
const SOURCE_DIR: &str = "source";
const BUILDS_DIR: &str = "builds";
const WORK_DIR: &str = ".noctics";
const BRIEF_DIR: &str = "brief";
const GENERATION_LOG: &str = "generation.log";

pub struct Builder {
    store: Arc<BuildStore>,
    studio: Arc<Studio>,
    app: AppHandle,
    games_root: Arc<GamesRoot>,
    cache_root: PathBuf,
    worker: AtomicBool,
    cancelling: Mutex<HashSet<i64>>,
    stopping: Mutex<HashSet<i64>>,
    toolchains: Mutex<Option<(Instant, Arc<Toolchains>)>>,
}

struct ProjectDirs {
    source: PathBuf,
    builds: PathBuf,
    work: PathBuf,
    brief: PathBuf,
}

enum Failure {
    Cancelled,
    Error(String),
}

pub enum Folder {
    Source,
    Builds,
}

impl Builder {
    pub fn new(
        store: Arc<BuildStore>,
        studio: Arc<Studio>,
        app: AppHandle,
        games_root: Arc<GamesRoot>,
        cache_root: PathBuf,
    ) -> Self {
        Self {
            store,
            studio,
            app,
            games_root,
            cache_root,
            worker: AtomicBool::new(false),
            cancelling: Mutex::new(HashSet::new()),
            stopping: Mutex::new(HashSet::new()),
            toolchains: Mutex::new(None),
        }
    }

    pub fn notify(&self) {
        let _ = self.app.emit(BUILDS_CHANGED, ());
    }

    pub fn overview(&self, project_id: i64, refresh: bool) -> Result<BuildOverview, BuildError> {
        let project = self.project(project_id)?;
        let tools = self.toolchains(refresh);
        let dirs = self.dirs(&project);
        let stored = self.store.source(project_id)?;
        let valid = stored
            .as_ref()
            .is_some_and(|source| source.path.is_dir() && recipes::marker_present(project.engine, &source.path));
        let usable = stored.as_ref().filter(|_| valid).map(|source| source.path.as_path());
        let targets = BuildTarget::ALL
            .into_iter()
            .map(|target| TargetReport {
                target,
                readiness: recipes::readiness(project.engine, target, usable, &tools),
            })
            .collect();
        let source = stored.map(|source| GameSource {
            path: source.path.to_string_lossy().into_owned(),
            origin: source.origin,
            status: source.status,
            engine: source.engine,
            valid,
            activity: source.activity,
            error: source.error,
        });
        Ok(BuildOverview {
            project_id,
            engine: project.engine,
            source,
            targets,
            builds: self.store.builds(project_id)?,
            output_dir: dirs.builds.to_string_lossy().into_owned(),
        })
    }

    pub fn link_source(&self, project_id: i64, raw: &str) -> Result<(), BuildError> {
        let cleaned = raw.trim().trim_matches('"').trim();
        if cleaned.is_empty() || cleaned.chars().count() > MAX_SOURCE_PATH_CHARS {
            return Err(BuildError::PathInvalid);
        }
        let path = PathBuf::from(cleaned);
        if !path.is_dir() {
            return Err(BuildError::PathInvalid);
        }
        let project = self.project(project_id)?;
        if !recipes::marker_present(project.engine, &path) {
            return Err(BuildError::SourceInvalid);
        }
        self.store.link_source(project_id, &path, project.engine)?;
        self.notify();
        Ok(())
    }

    pub fn generate(self: &Arc<Self>, project_id: i64, request: Option<String>) -> Result<(), BuildError> {
        let request = request
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty());
        if request
            .as_ref()
            .is_some_and(|text| text.chars().count() > MAX_BUILD_REQUEST_CHARS)
        {
            return Err(BuildError::RequestTooLong);
        }
        self.start_generation(project_id, request)
    }

    pub fn fix(self: &Arc<Self>, build_id: i64) -> Result<(), BuildError> {
        let stored = self.store.build(build_id)?;
        let build = &stored.build;
        let lines: Vec<&str> = build.log_tail.lines().collect();
        let recent = lines[lines.len().saturating_sub(FIX_LOG_LINES)..].join("\n");
        let request = format!(
            "Збірка під {} впала: {}.\nОстанні рядки логу збірки студії:\n```\n{recent}\n```\n\
Знайди причину в проєкті й виправ її, щоб ця збірка проходила. Кроки збірки позначені в логу рядками «▸».",
            build.target.label(),
            build.error.as_deref().unwrap_or("невідома помилка")
        );
        self.start_generation(build.project_id, Some(request))
    }

    pub fn stop_generation(&self, project_id: i64) {
        if let Ok(mut stopping) = self.stopping.lock() {
            stopping.insert(project_id);
        }
    }

    pub fn queue(self: &Arc<Self>, project_id: i64, targets: &[BuildTarget]) -> Result<(), BuildError> {
        let mut chosen: Vec<BuildTarget> = Vec::new();
        for target in targets {
            if !chosen.contains(target) {
                chosen.push(*target);
            }
        }
        if chosen.is_empty() {
            return Err(BuildError::NothingSelected);
        }
        let project = self.project(project_id)?;
        let source = self.store.source(project_id)?.ok_or(BuildError::NoSource)?;
        if source.status == SourceStatus::Generating {
            return Err(BuildError::SourceBusy);
        }
        if !source.path.is_dir() || !recipes::marker_present(project.engine, &source.path) {
            return Err(BuildError::SourceInvalid);
        }
        let tools = self.toolchains(false);
        if let Some(blocked) = chosen
            .iter()
            .find(|target| !recipes::readiness(project.engine, **target, Some(&source.path), &tools).is_ready())
        {
            return Err(BuildError::TargetUnavailable(*blocked));
        }
        let dirs = self.dirs(&project);
        for target in chosen {
            self.store.queue_build(project_id, target, &dirs.builds)?;
        }
        self.notify();
        self.kick();
        Ok(())
    }

    pub fn cancel(&self, build_id: i64) -> Result<(), BuildError> {
        if self.store.cancel_queued(build_id)? == BuildStatus::Running
            && let Ok(mut cancelling) = self.cancelling.lock()
        {
            cancelling.insert(build_id);
        }
        self.notify();
        Ok(())
    }

    pub fn artifact_path(&self, build_id: i64) -> Result<PathBuf, BuildError> {
        let stored = self.store.build(build_id)?;
        stored
            .build
            .artifact
            .map(PathBuf::from)
            .filter(|path| path.exists())
            .or_else(|| Some(stored.output_dir).filter(|dir| dir.is_dir()))
            .ok_or(BuildError::NotFound)
    }

    pub fn log_path(&self, build_id: i64) -> Result<PathBuf, BuildError> {
        let stored = self.store.build(build_id)?;
        Some(stored.log_path)
            .filter(|path| path.is_file())
            .ok_or(BuildError::NotFound)
    }

    pub fn folder(&self, project_id: i64, folder: Folder) -> Result<PathBuf, BuildError> {
        let project = self.project(project_id)?;
        let path = match folder {
            Folder::Source => self.store.source(project_id)?.ok_or(BuildError::NoSource)?.path,
            Folder::Builds => self.dirs(&project).builds,
        };
        std::fs::create_dir_all(&path).map_err(|error| BuildError::Process(error.to_string()))?;
        Ok(path)
    }

    pub fn kick(self: &Arc<Self>) {
        if self.worker.swap(true, Ordering::SeqCst) {
            return;
        }
        let builder = Arc::clone(self);
        std::thread::spawn(move || {
            while let Ok(Some(job)) = builder.store.claim_next() {
                builder.run_build(job);
            }
            builder.worker.store(false, Ordering::SeqCst);
            if builder.store.has_queued().unwrap_or(false) {
                builder.kick();
            }
        });
    }

    fn start_generation(self: &Arc<Self>, project_id: i64, request: Option<String>) -> Result<(), BuildError> {
        self.studio.require_connection().map_err(BuildError::Claude)?;
        let detail = self.studio.store.detail(project_id).map_err(studio_failure)?;
        let project = detail.project.clone();
        let dirs = self.dirs(&project);
        let (path, origin) = self
            .store
            .source(project_id)?
            .filter(|source| source.path.is_dir())
            .map_or((dirs.source.clone(), SourceOrigin::Generated), |source| {
                (source.path, source.origin)
            });
        std::fs::create_dir_all(&path).map_err(|error| BuildError::Process(error.to_string()))?;
        codegen::write_brief(&dirs.brief, &detail).map_err(BuildError::Process)?;
        let fresh = !recipes::marker_present(project.engine, &path);
        self.store
            .begin_generation(project_id, &path, origin, project.engine)?;
        if let Ok(mut stopping) = self.stopping.lock() {
            stopping.remove(&project_id);
        }
        self.journal(project_id, JournalEvent::CodeStarted { fresh });
        self.notify();
        let builder = Arc::clone(self);
        std::thread::spawn(move || {
            let error = builder.run_generation(&project, &path, &dirs, request.as_deref()).err();
            let _ = builder.store.finish_generation(project_id, error.as_deref());
            builder.journal(
                project_id,
                match &error {
                    Some(reason) => JournalEvent::CodeFailed { reason: reason.clone() },
                    None => JournalEvent::CodeReady {
                        files: recipes::count_files(&path),
                    },
                },
            );
            if let Ok(mut stopping) = builder.stopping.lock() {
                stopping.remove(&project_id);
            }
            builder.notify();
        });
        Ok(())
    }

    fn run_generation(
        self: &Arc<Self>,
        project: &Project,
        root: &Path,
        dirs: &ProjectDirs,
        request: Option<&str>,
    ) -> Result<(), String> {
        let executable = claude_code::locate().ok_or("Claude Code не знайдено — встанови його в Налаштуваннях.")?;
        let tools = self.toolchains(false);
        let names = GameNames::new(&project.name);
        let assignment = Assignment {
            engine: project.engine,
            names: &names,
            brief: &dirs.brief,
            tools: &tools,
            request,
            fresh: !recipes::marker_present(project.engine, root),
        };
        let mut command = Command::new(executable);
        command
            .args(codegen::arguments(&dirs.brief, &codegen::system_prompt(&assignment)))
            .current_dir(root);
        for variable in claude_code::SHADOWING_CREDENTIALS {
            command.env_remove(variable);
        }
        let journal = Arc::new(Journal::open(
            &dirs.work.join(GENERATION_LOG),
            project.id,
            Arc::clone(&self.store),
            self.app.clone(),
        ));
        journal.note(if assignment.fresh {
            format!("Claude пише гру на {} з нуля", project.engine.label())
        } else {
            "Claude доопрацьовує код гри".to_owned()
        });
        let reader = Arc::clone(&journal);
        let base = root.to_path_buf();
        let sink: LineSink = Arc::new(move |line: String| reader.consume(&line, &base));
        let project_id = project.id;
        let outcome = process::stream(
            command,
            Some(codegen::task_message(&assignment)),
            sink,
            drop,
            || self.stopping.lock().is_ok_and(|stopping| stopping.contains(&project_id)),
            GENERATION_TIMEOUT,
        );
        let verdict = journal.verdict();
        let result = match outcome {
            Err(RunFailure::Cancelled) => Err("Генерацію зупинено.".to_owned()),
            Err(RunFailure::TimedOut) => Err("Claude працював задовго — генерацію зупинено.".to_owned()),
            Err(failure) => Err(format!("Claude Code: {}", recipes::failure_message(&failure))),
            Ok(status) => match verdict {
                Some(Err(message)) => Err(message),
                Some(Ok(())) => Ok(()),
                None if status.success() => Err("Claude Code завершився без результату.".to_owned()),
                None => Err(format!("Claude Code: {}", recipes::exit_message(status.code()))),
            },
        };
        let result = result.and_then(|()| {
            if recipes::marker_present(project.engine, root) {
                Ok(())
            } else {
                Err(format!(
                    "Claude завершив роботу, але в папці немає {} — проєкту {} не видно.",
                    project_marker(project.engine),
                    project.engine.label()
                ))
            }
        });
        journal.note(match &result {
            Ok(()) => "✓ Код гри готовий — можна збирати".to_owned(),
            Err(message) => format!("✕ {message}"),
        });
        journal.flush();
        result
    }

    fn run_build(self: &Arc<Self>, job: StoredBuild) {
        let id = job.build.id;
        self.notify();
        let log = Arc::new(BuildLog::open(&job.log_path, id, Arc::clone(&self.store), self.app.clone()));
        let outcome = self.execute(&job, &log);
        let tail = log.tail();
        let target = job.build.target;
        let (_, event) = match outcome {
            Ok((artifact, size)) => (
                self.store.finish_build(id, &artifact, size, &tail),
                JournalEvent::BuildDone {
                    target,
                    size_bytes: size,
                },
            ),
            Err(Failure::Cancelled) => (
                self.store.close_build(id, BuildStatus::Cancelled, None, &tail),
                JournalEvent::BuildCancelled { target },
            ),
            Err(Failure::Error(message)) => (
                self.store.close_build(id, BuildStatus::Failed, Some(&message), &tail),
                JournalEvent::BuildFailed {
                    target,
                    reason: error_clue(&tail).unwrap_or(message),
                },
            ),
        };
        self.journal(job.build.project_id, event);
        if let Ok(mut cancelling) = self.cancelling.lock() {
            cancelling.remove(&id);
        }
        self.notify();
    }

    fn execute(&self, job: &StoredBuild, log: &Arc<BuildLog>) -> Result<(PathBuf, u64), Failure> {
        let id = job.build.id;
        let target = job.build.target;
        let project = self
            .project(job.build.project_id)
            .map_err(|_| Failure::Error("Проєкт видалено".into()))?;
        let source = self
            .store
            .source(project.id)
            .ok()
            .flatten()
            .ok_or_else(|| Failure::Error("У проєкту немає коду гри".into()))?;
        let tools = self.toolchains(true);
        let names = GameNames::new(&project.name);
        let dirs = self.dirs(&project);
        for dir in [&job.output_dir, &dirs.work, &self.cache_root] {
            std::fs::create_dir_all(dir)
                .map_err(|error| Failure::Error(format!("Не вдалося створити {}: {error}", dir.display())))?;
        }
        log.line(format!(
            "Noctics Factory · {} · {} → {}",
            names.title,
            project.engine.label(),
            target.label()
        ));
        log.line(format!("Код гри: {}", source.path.display()));
        log.line(format!("Збірка: {}", job.output_dir.display()));
        let plan = recipes::plan(&Context {
            engine: project.engine,
            target,
            source: &source.path,
            output: &job.output_dir,
            work: &dirs.work,
            cache: &self.cache_root,
            names: &names,
            tools: &tools,
        })
        .map_err(Failure::Error)?;
        log.line(format!("Інструменти: {}", plan.toolchain));
        self.journal(
            project.id,
            JournalEvent::BuildStarted {
                target,
                toolchain: plan.toolchain.clone(),
            },
        );
        let cancelled = || self.cancelling.lock().is_ok_and(|cancelling| cancelling.contains(&id));
        recipes::execute(plan.steps, &log.sink(), cancelled, BUILD_TIMEOUT).map_err(|failure| match failure {
            StepFailure::Cancelled => Failure::Cancelled,
            StepFailure::Failed(message) => Failure::Error(message),
        })?;
        let artifact = recipes::locate(&plan.artifact)
            .ok_or_else(|| Failure::Error("Збірка завершилась, але готового файлу гри не знайдено".into()))?;
        let size = recipes::size_of(&artifact);
        log.line(format!("✓ Готово: {} · {}", artifact.display(), format_size(size)));
        Ok((artifact, size))
    }

    fn journal(&self, project_id: i64, event: JournalEvent) {
        let _ = self.studio.store.note(project_id, &event);
        self.studio.notify();
    }

    fn project(&self, project_id: i64) -> Result<Project, BuildError> {
        self.studio
            .store
            .list_projects()
            .map_err(studio_failure)?
            .into_iter()
            .find(|project| project.id == project_id)
            .ok_or(BuildError::NotFound)
    }

    fn dirs(&self, project: &Project) -> ProjectDirs {
        let root = self.generated_root(project.id).unwrap_or_else(|| {
            self.games_root
                .path()
                .join(format!("{}-{}", recipes::slugify(&project.name), project.id))
        });
        let work = root.join(WORK_DIR);
        ProjectDirs {
            source: root.join(SOURCE_DIR),
            builds: root.join(BUILDS_DIR),
            brief: work.join(BRIEF_DIR),
            work,
        }
    }

    fn generated_root(&self, project_id: i64) -> Option<PathBuf> {
        self.store
            .source(project_id)
            .ok()
            .flatten()
            .filter(|source| source.origin == SourceOrigin::Generated)
            .filter(|source| source.path.file_name().is_some_and(|name| name == SOURCE_DIR))
            .and_then(|source| source.path.parent().map(Path::to_path_buf))
    }

    fn toolchains(&self, refresh: bool) -> Arc<Toolchains> {
        if !refresh
            && let Some((checked, tools)) = self.toolchains.lock().ok().and_then(|slot| slot.clone())
            && checked.elapsed() < TOOLCHAIN_TTL
        {
            return tools;
        }
        let fresh = Arc::new(Toolchains::detect());
        if let Ok(mut slot) = self.toolchains.lock() {
            *slot = Some((Instant::now(), Arc::clone(&fresh)));
        }
        fresh
    }
}

fn studio_failure(problem: StudioError) -> BuildError {
    match problem {
        StudioError::NotFound => BuildError::NotFound,
        StudioError::Storage(detail) => BuildError::Storage(detail),
        other => BuildError::Claude(other),
    }
}

fn error_clue(tail: &str) -> Option<String> {
    tail.lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.starts_with(STEP_MARKER))
        .find(|line| line.to_lowercase().contains(ERROR_MARKER))
        .map(|line| {
            if line.chars().count() <= MAX_CLUE_CHARS {
                line.to_owned()
            } else {
                let mut clipped: String = line.chars().take(MAX_CLUE_CHARS - 1).collect();
                clipped.push('…');
                clipped
            }
        })
}

fn clip_line(line: &str) -> String {
    if line.chars().count() <= MAX_LINE_CHARS {
        return line.to_owned();
    }
    let mut clipped: String = line.chars().take(MAX_LINE_CHARS - 1).collect();
    clipped.push('…');
    clipped
}

fn open_log(path: &Path) -> Option<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    File::create(path).ok()
}

struct LogState {
    file: Option<File>,
    tail: VecDeque<String>,
    flushed: Instant,
}

struct BuildLog {
    state: Mutex<LogState>,
    store: Arc<BuildStore>,
    app: AppHandle,
    id: i64,
}

impl BuildLog {
    fn open(path: &Path, id: i64, store: Arc<BuildStore>, app: AppHandle) -> Self {
        Self {
            state: Mutex::new(LogState {
                file: open_log(path),
                tail: VecDeque::with_capacity(TAIL_LINES),
                flushed: Instant::now(),
            }),
            store,
            app,
            id,
        }
    }

    fn line(&self, text: String) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(file) = state.file.as_mut() {
            let _ = writeln!(file, "{text}");
        }
        if state.tail.len() == TAIL_LINES {
            state.tail.pop_front();
        }
        state.tail.push_back(clip_line(&text));
        if state.flushed.elapsed() >= FLUSH_INTERVAL {
            state.flushed = Instant::now();
            let tail = state.tail.iter().cloned().collect::<Vec<_>>().join("\n");
            drop(state);
            let _ = self.store.update_tail(self.id, &tail);
            let _ = self.app.emit(BUILDS_CHANGED, ());
        }
    }

    fn tail(&self) -> String {
        self.state
            .lock()
            .map(|state| state.tail.iter().cloned().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    }

    fn sink(self: &Arc<Self>) -> LineSink {
        let log = Arc::clone(self);
        Arc::new(move |line: String| log.line(line))
    }
}

struct JournalState {
    file: Option<File>,
    activity: Vec<String>,
    verdict: Option<Result<(), String>>,
    flushed: Instant,
    dirty: bool,
}

struct Journal {
    state: Mutex<JournalState>,
    store: Arc<BuildStore>,
    app: AppHandle,
    project_id: i64,
}

impl Journal {
    fn open(path: &Path, project_id: i64, store: Arc<BuildStore>, app: AppHandle) -> Self {
        Self {
            state: Mutex::new(JournalState {
                file: open_log(path),
                activity: Vec::new(),
                verdict: None,
                flushed: Instant::now(),
                dirty: false,
            }),
            store,
            app,
            project_id,
        }
    }

    fn consume(&self, line: &str, root: &Path) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(file) = state.file.as_mut() {
            let _ = writeln!(file, "{line}");
        }
        match codegen::read_event(line, root) {
            Event::Activity(lines) => {
                state.dirty |= !lines.is_empty();
                state.activity.extend(lines);
                let overflow = state.activity.len().saturating_sub(MAX_ACTIVITY);
                state.activity.drain(..overflow);
            }
            Event::Finished(verdict) => state.verdict = Some(verdict),
            Event::Quiet => {}
        }
        if state.dirty && state.flushed.elapsed() >= FLUSH_INTERVAL {
            state.dirty = false;
            state.flushed = Instant::now();
            let activity = state.activity.clone();
            drop(state);
            let _ = self.store.set_activity(self.project_id, &activity);
            let _ = self.app.emit(BUILDS_CHANGED, ());
        }
    }

    fn note(&self, text: String) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(file) = state.file.as_mut() {
                let _ = writeln!(file, "{text}");
            }
            state.activity.push(text);
            state.dirty = true;
        }
        self.flush();
    }

    fn verdict(&self) -> Option<Result<(), String>> {
        self.state.lock().ok().and_then(|state| state.verdict.clone())
    }

    fn flush(&self) {
        let activity = match self.state.lock() {
            Ok(mut state) => {
                state.dirty = false;
                state.flushed = Instant::now();
                state.activity.clone()
            }
            Err(_) => return,
        };
        let _ = self.store.set_activity(self.project_id, &activity);
        let _ = self.app.emit(BUILDS_CHANGED, ());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_overlong_log_lines() {
        let long = "x".repeat(MAX_LINE_CHARS + 10);
        let clipped = clip_line(&long);
        assert_eq!(clipped.chars().count(), MAX_LINE_CHARS);
        assert!(clipped.ends_with('…'));
        assert_eq!(clip_line("short"), "short");
    }

    #[test]
    fn maps_studio_failures() {
        assert_eq!(studio_failure(StudioError::NotFound), BuildError::NotFound);
        assert_eq!(
            studio_failure(StudioError::UsageLimit),
            BuildError::Claude(StudioError::UsageLimit)
        );
    }
}
