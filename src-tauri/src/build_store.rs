use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use noctics_shared::{
    Build, BuildError, BuildStatus, BuildTarget, GameEngine, PlaytestReport, PlaytestStatus, SourceOrigin,
    SourceStatus,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::accounts::unix_now;

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS game_sources (
        project_id INTEGER PRIMARY KEY,
        path TEXT NOT NULL,
        origin TEXT NOT NULL,
        engine TEXT NOT NULL,
        status TEXT NOT NULL,
        activity TEXT NOT NULL DEFAULT '',
        error TEXT,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS builds (
        id INTEGER PRIMARY KEY,
        project_id INTEGER NOT NULL,
        target TEXT NOT NULL,
        status TEXT NOT NULL,
        output_dir TEXT NOT NULL DEFAULT '',
        log_path TEXT NOT NULL DEFAULT '',
        artifact TEXT,
        size_bytes INTEGER,
        log_tail TEXT NOT NULL DEFAULT '',
        error TEXT,
        created_at INTEGER NOT NULL,
        started_at INTEGER,
        finished_at INTEGER
    );
    CREATE INDEX IF NOT EXISTS builds_by_project ON builds (project_id, id);
    CREATE TABLE IF NOT EXISTS build_plans (
        project_id INTEGER PRIMARY KEY,
        targets TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS playtests (
        project_id INTEGER PRIMARY KEY,
        status TEXT NOT NULL,
        run_dir TEXT NOT NULL,
        round_dir TEXT NOT NULL DEFAULT '',
        errors TEXT NOT NULL DEFAULT '',
        activity TEXT NOT NULL DEFAULT '',
        report TEXT,
        error TEXT,
        started_at INTEGER NOT NULL,
        finished_at INTEGER
    );
";
const PLAN_SEPARATOR: char = ',';
const REQUEUE_INTERRUPTED: &str =
    "UPDATE builds SET status = 'queued', started_at = NULL WHERE status = 'running'";
const INTERRUPTED_GENERATION: &str =
    "Генерацію перервано: застосунок закрили. Запусти її ще раз.";
const INTERRUPTED_PLAYTEST: &str = "Плейтест перервано: застосунок закрили. Запусти його ще раз.";
const LINE_BREAK: &str = "\n";
const SELECT_BUILD: &str = "SELECT id, project_id, target, status, artifact, size_bytes, log_tail, error, \
created_at, started_at, finished_at, output_dir, log_path FROM builds";
const HISTORY_LIMIT: i64 = 30;
const LOGS_DIR: &str = "logs";
pub const MAX_ACTIVITY: usize = 40;

pub struct StoredSource {
    pub path: PathBuf,
    pub origin: SourceOrigin,
    pub engine: GameEngine,
    pub status: SourceStatus,
    pub activity: Vec<String>,
    pub error: Option<String>,
}

pub struct StoredPlaytest {
    pub status: PlaytestStatus,
    pub run_dir: PathBuf,
    pub round_dir: Option<PathBuf>,
    pub errors: Vec<String>,
    pub activity: Vec<String>,
    pub report: Option<PlaytestReport>,
    pub error: Option<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

pub struct StoredBuild {
    pub build: Build,
    pub output_dir: PathBuf,
    pub log_path: PathBuf,
}

pub struct BuildStore {
    connection: Mutex<Connection>,
}

impl BuildStore {
    pub fn open(path: &Path) -> Result<Self, BuildError> {
        Self::prepare(Connection::open(path).map_err(storage)?)
    }

    fn prepare(connection: Connection) -> Result<Self, BuildError> {
        connection.execute_batch(SCHEMA).map_err(storage)?;
        connection.execute(REQUEUE_INTERRUPTED, []).map_err(storage)?;
        connection
            .execute(
                "UPDATE game_sources SET status = ?1, error = ?2 WHERE status = ?3",
                params![
                    SourceStatus::Failed.code(),
                    INTERRUPTED_GENERATION,
                    SourceStatus::Generating.code()
                ],
            )
            .map_err(storage)?;
        connection
            .execute(
                "UPDATE playtests SET status = ?1, error = ?2, finished_at = ?3 WHERE status IN (?4, ?5)",
                params![
                    PlaytestStatus::Failed.code(),
                    INTERRUPTED_PLAYTEST,
                    unix_now(),
                    PlaytestStatus::Capturing.code(),
                    PlaytestStatus::Reviewing.code()
                ],
            )
            .map_err(storage)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn begin_playtest(&self, project_id: i64, run_dir: &Path) -> Result<(), BuildError> {
        let connection = self.lock()?;
        if playtest_active(&connection, project_id)? {
            return Err(BuildError::PlaytestBusy);
        }
        connection
            .execute(
                "INSERT INTO playtests (project_id, status, run_dir, round_dir, errors, activity, report, error, started_at, finished_at)
                 VALUES (?1, ?2, ?3, '', '', '', NULL, NULL, ?4, NULL)
                 ON CONFLICT(project_id) DO UPDATE SET
                    status = excluded.status,
                    run_dir = excluded.run_dir,
                    round_dir = '',
                    errors = '',
                    activity = '',
                    report = NULL,
                    error = NULL,
                    started_at = excluded.started_at,
                    finished_at = NULL",
                params![
                    project_id,
                    PlaytestStatus::Capturing.code(),
                    run_dir.to_string_lossy(),
                    unix_now()
                ],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn playtest_active(&self, project_id: i64) -> Result<bool, BuildError> {
        playtest_active(&*self.lock()?, project_id)
    }

    pub fn set_playtest_status(&self, project_id: i64, status: PlaytestStatus) -> Result<(), BuildError> {
        self.lock()?
            .execute(
                "UPDATE playtests SET status = ?1 WHERE project_id = ?2",
                params![status.code(), project_id],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn set_playtest_round(&self, project_id: i64, round_dir: &Path, errors: &[String]) -> Result<(), BuildError> {
        self.lock()?
            .execute(
                "UPDATE playtests SET round_dir = ?1, errors = ?2 WHERE project_id = ?3",
                params![round_dir.to_string_lossy(), errors.join(LINE_BREAK), project_id],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn set_playtest_activity(&self, project_id: i64, activity: &[String]) -> Result<(), BuildError> {
        let start = activity.len().saturating_sub(MAX_ACTIVITY);
        self.lock()?
            .execute(
                "UPDATE playtests SET activity = ?1 WHERE project_id = ?2",
                params![activity[start..].join(LINE_BREAK), project_id],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn finish_playtest(
        &self,
        project_id: i64,
        report: Option<&PlaytestReport>,
        error: Option<&str>,
    ) -> Result<(), BuildError> {
        let status = if error.is_some() {
            PlaytestStatus::Failed
        } else {
            PlaytestStatus::Done
        };
        let report = report
            .map(serde_json::to_string)
            .transpose()
            .map_err(|problem| BuildError::Storage(problem.to_string()))?;
        self.lock()?
            .execute(
                "UPDATE playtests SET status = ?1, report = ?2, error = ?3, finished_at = ?4 WHERE project_id = ?5",
                params![status.code(), report, error, unix_now(), project_id],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn playtest(&self, project_id: i64) -> Result<Option<StoredPlaytest>, BuildError> {
        self.lock()?
            .query_row(
                "SELECT status, run_dir, round_dir, errors, activity, report, error, started_at, finished_at
                 FROM playtests WHERE project_id = ?1",
                params![project_id],
                |row| {
                    let status: String = row.get(0)?;
                    let round_dir: String = row.get(2)?;
                    let report: Option<String> = row.get(5)?;
                    Ok(StoredPlaytest {
                        status: PlaytestStatus::from_code(&status).unwrap_or(PlaytestStatus::Failed),
                        run_dir: PathBuf::from(row.get::<_, String>(1)?),
                        round_dir: Some(round_dir).filter(|dir| !dir.is_empty()).map(PathBuf::from),
                        errors: lines_of(&row.get::<_, String>(3)?),
                        activity: lines_of(&row.get::<_, String>(4)?),
                        report: report.and_then(|text| serde_json::from_str(&text).ok()),
                        error: row.get(6)?,
                        started_at: row.get(7)?,
                        finished_at: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(storage)
    }

    pub fn has_active_builds(&self, project_id: i64) -> Result<bool, BuildError> {
        self.lock()?
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM builds WHERE project_id = ?1 AND status IN (?2, ?3))",
                params![project_id, BuildStatus::Queued.code(), BuildStatus::Running.code()],
                |row| row.get(0),
            )
            .map_err(storage)
    }

    pub fn set_plan(&self, project_id: i64, targets: &[BuildTarget]) -> Result<(), BuildError> {
        let codes: Vec<&str> = BuildTarget::ALL
            .into_iter()
            .filter(|target| targets.contains(target))
            .map(BuildTarget::code)
            .collect();
        self.lock()?
            .execute(
                "INSERT INTO build_plans (project_id, targets) VALUES (?1, ?2)
                 ON CONFLICT(project_id) DO UPDATE SET targets = excluded.targets",
                params![project_id, codes.join(&PLAN_SEPARATOR.to_string())],
            )
            .map_err(storage)?;
        Ok(())
    }

    pub fn plan(&self, project_id: i64) -> Result<Vec<BuildTarget>, BuildError> {
        let stored: Option<String> = self
            .lock()?
            .query_row(
                "SELECT targets FROM build_plans WHERE project_id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)?;
        Ok(stored
            .unwrap_or_default()
            .split(PLAN_SEPARATOR)
            .filter_map(BuildTarget::from_code)
            .collect())
    }

    pub fn source(&self, project_id: i64) -> Result<Option<StoredSource>, BuildError> {
        self.lock()?
            .query_row(
                "SELECT path, origin, engine, status, activity, error FROM game_sources WHERE project_id = ?1",
                params![project_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(storage)
            .map(|found| {
                found.map(|(path, origin, engine, status, activity, error)| StoredSource {
                    path: PathBuf::from(path),
                    origin: SourceOrigin::from_code(&origin).unwrap_or(SourceOrigin::Linked),
                    engine: GameEngine::from_code(&engine).unwrap_or_default(),
                    status: SourceStatus::from_code(&status).unwrap_or(SourceStatus::Failed),
                    activity: lines_of(&activity),
                    error,
                })
            })
    }

    pub fn link_source(&self, project_id: i64, path: &Path, engine: GameEngine) -> Result<(), BuildError> {
        let connection = self.lock()?;
        if is_generating(&connection, project_id)? {
            return Err(BuildError::SourceBusy);
        }
        upsert_source(
            &connection,
            project_id,
            path,
            SourceOrigin::Linked,
            engine,
            SourceStatus::Ready,
        )
    }

    pub fn begin_generation(
        &self,
        project_id: i64,
        path: &Path,
        origin: SourceOrigin,
        engine: GameEngine,
    ) -> Result<(), BuildError> {
        let connection = self.lock()?;
        if is_generating(&connection, project_id)? {
            return Err(BuildError::SourceBusy);
        }
        upsert_source(&connection, project_id, path, origin, engine, SourceStatus::Generating)
    }

    pub fn set_activity(&self, project_id: i64, activity: &[String]) -> Result<(), BuildError> {
        let start = activity.len().saturating_sub(MAX_ACTIVITY);
        let text = activity[start..].join("\n");
        self.lock()?
            .execute(
                "UPDATE game_sources SET activity = ?1, updated_at = ?2 WHERE project_id = ?3",
                params![text, unix_now(), project_id],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn finish_generation(&self, project_id: i64, error: Option<&str>) -> Result<(), BuildError> {
        let status = if error.is_some() {
            SourceStatus::Failed
        } else {
            SourceStatus::Ready
        };
        self.lock()?
            .execute(
                "UPDATE game_sources SET status = ?1, error = ?2, updated_at = ?3 WHERE project_id = ?4",
                params![status.code(), error, unix_now(), project_id],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn queue_build(&self, project_id: i64, target: BuildTarget, builds_root: &Path) -> Result<i64, BuildError> {
        let connection = self.lock()?;
        connection
            .execute(
                "INSERT INTO builds (project_id, target, status, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![project_id, target.code(), BuildStatus::Queued.code(), unix_now()],
            )
            .map_err(storage)?;
        let id = connection.last_insert_rowid();
        let folder = format!("{}-{id}", target.code());
        let output_dir = builds_root.join(&folder);
        let log_path = builds_root.join(LOGS_DIR).join(format!("{folder}.log"));
        connection
            .execute(
                "UPDATE builds SET output_dir = ?1, log_path = ?2 WHERE id = ?3",
                params![
                    output_dir.to_string_lossy(),
                    log_path.to_string_lossy(),
                    id
                ],
            )
            .map_err(storage)?;
        Ok(id)
    }

    pub fn has_queued(&self) -> Result<bool, BuildError> {
        self.lock()?
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM builds WHERE status = ?1)",
                params![BuildStatus::Queued.code()],
                |row| row.get(0),
            )
            .map_err(storage)
    }

    pub fn claim_next(&self) -> Result<Option<StoredBuild>, BuildError> {
        let connection = self.lock()?;
        let next = connection
            .query_row(
                &format!("{SELECT_BUILD} WHERE status = ?1 ORDER BY id LIMIT 1"),
                params![BuildStatus::Queued.code()],
                read_stored,
            )
            .optional()
            .map_err(storage)?;
        let Some(mut claimed) = next else {
            return Ok(None);
        };
        let now = unix_now();
        connection
            .execute(
                "UPDATE builds SET status = ?1, started_at = ?2, error = NULL, log_tail = '' WHERE id = ?3",
                params![BuildStatus::Running.code(), now, claimed.build.id],
            )
            .map_err(storage)?;
        claimed.build.status = BuildStatus::Running;
        claimed.build.started_at = Some(now);
        Ok(Some(claimed))
    }

    pub fn update_tail(&self, id: i64, tail: &str) -> Result<(), BuildError> {
        self.lock()?
            .execute(
                "UPDATE builds SET log_tail = ?1 WHERE id = ?2 AND status = ?3",
                params![tail, id, BuildStatus::Running.code()],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn finish_build(&self, id: i64, artifact: &Path, size: u64, tail: &str) -> Result<(), BuildError> {
        self.lock()?
            .execute(
                "UPDATE builds SET status = ?1, artifact = ?2, size_bytes = ?3, log_tail = ?4, finished_at = ?5 WHERE id = ?6",
                params![
                    BuildStatus::Done.code(),
                    artifact.to_string_lossy(),
                    i64::try_from(size).unwrap_or(i64::MAX),
                    tail,
                    unix_now(),
                    id
                ],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn close_build(&self, id: i64, status: BuildStatus, error: Option<&str>, tail: &str) -> Result<(), BuildError> {
        self.lock()?
            .execute(
                "UPDATE builds SET status = ?1, error = ?2, log_tail = ?3, finished_at = ?4 WHERE id = ?5",
                params![status.code(), error, tail, unix_now(), id],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn cancel_queued(&self, id: i64) -> Result<BuildStatus, BuildError> {
        let connection = self.lock()?;
        let status: String = connection
            .query_row("SELECT status FROM builds WHERE id = ?1", params![id], |row| row.get(0))
            .optional()
            .map_err(storage)?
            .ok_or(BuildError::NotFound)?;
        let status = BuildStatus::from_code(&status).ok_or(BuildError::NotFound)?;
        if status == BuildStatus::Queued {
            connection
                .execute(
                    "UPDATE builds SET status = ?1, finished_at = ?2 WHERE id = ?3",
                    params![BuildStatus::Cancelled.code(), unix_now(), id],
                )
                .map_err(storage)?;
        }
        Ok(status)
    }

    pub fn builds(&self, project_id: i64) -> Result<Vec<Build>, BuildError> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(&format!("{SELECT_BUILD} WHERE project_id = ?1 ORDER BY id DESC LIMIT ?2"))
            .map_err(storage)?;
        statement
            .query_map(params![project_id, HISTORY_LIMIT], read_stored)
            .map_err(storage)?
            .map(|row| row.map(|stored| stored.build))
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)
    }

    pub fn build(&self, id: i64) -> Result<StoredBuild, BuildError> {
        self.lock()?
            .query_row(&format!("{SELECT_BUILD} WHERE id = ?1"), params![id], read_stored)
            .optional()
            .map_err(storage)?
            .ok_or(BuildError::NotFound)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, BuildError> {
        self.connection
            .lock()
            .map_err(|_| BuildError::Storage("build store lock is poisoned".into()))
    }
}

fn lines_of(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn playtest_active(connection: &Connection, project_id: i64) -> Result<bool, BuildError> {
    connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM playtests WHERE project_id = ?1 AND status IN (?2, ?3))",
            params![
                project_id,
                PlaytestStatus::Capturing.code(),
                PlaytestStatus::Reviewing.code()
            ],
            |row| row.get(0),
        )
        .map_err(storage)
}

fn is_generating(connection: &Connection, project_id: i64) -> Result<bool, BuildError> {
    connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM game_sources WHERE project_id = ?1 AND status = ?2)",
            params![project_id, SourceStatus::Generating.code()],
            |row| row.get(0),
        )
        .map_err(storage)
}

fn upsert_source(
    connection: &Connection,
    project_id: i64,
    path: &Path,
    origin: SourceOrigin,
    engine: GameEngine,
    status: SourceStatus,
) -> Result<(), BuildError> {
    connection
        .execute(
            "INSERT INTO game_sources (project_id, path, origin, engine, status, activity, error, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, '', NULL, ?6)
             ON CONFLICT(project_id) DO UPDATE SET
                path = excluded.path,
                origin = excluded.origin,
                engine = excluded.engine,
                status = excluded.status,
                activity = '',
                error = NULL,
                updated_at = excluded.updated_at",
            params![
                project_id,
                path.to_string_lossy(),
                origin.code(),
                engine.code(),
                status.code(),
                unix_now()
            ],
        )
        .map(drop)
        .map_err(storage)
}

fn read_stored(row: &Row<'_>) -> rusqlite::Result<StoredBuild> {
    let target: String = row.get(2)?;
    let status: String = row.get(3)?;
    let size: Option<i64> = row.get(5)?;
    Ok(StoredBuild {
        build: Build {
            id: row.get(0)?,
            project_id: row.get(1)?,
            target: BuildTarget::from_code(&target).unwrap_or(BuildTarget::Windows),
            status: BuildStatus::from_code(&status).unwrap_or(BuildStatus::Failed),
            artifact: row.get(4)?,
            size_bytes: size.and_then(|size| u64::try_from(size).ok()),
            log_tail: row.get(6)?,
            error: row.get(7)?,
            created_at: row.get(8)?,
            started_at: row.get(9)?,
            finished_at: row.get(10)?,
        },
        output_dir: PathBuf::from(row.get::<_, String>(11)?),
        log_path: PathBuf::from(row.get::<_, String>(12)?),
    })
}

fn storage(error: rusqlite::Error) -> BuildError {
    BuildError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory() -> BuildStore {
        BuildStore::prepare(Connection::open_in_memory().unwrap()).unwrap()
    }

    #[test]
    fn remembers_the_build_plan_of_each_project() {
        let store = in_memory();
        assert!(store.plan(3).unwrap().is_empty());
        store
            .set_plan(3, &[BuildTarget::Web, BuildTarget::Windows, BuildTarget::Web])
            .unwrap();
        assert_eq!(store.plan(3).unwrap(), [BuildTarget::Windows, BuildTarget::Web]);
        store.set_plan(3, &[]).unwrap();
        assert!(store.plan(3).unwrap().is_empty());
        assert!(store.plan(4).unwrap().is_empty());
    }

    #[test]
    fn queues_claims_and_finishes_builds_in_order() {
        let store = in_memory();
        let root = Path::new("builds");
        let first = store.queue_build(7, BuildTarget::Windows, root).unwrap();
        let second = store.queue_build(7, BuildTarget::Web, root).unwrap();
        assert!(store.has_queued().unwrap());

        let claimed = store.claim_next().unwrap().unwrap();
        assert_eq!(claimed.build.id, first);
        assert_eq!(claimed.build.status, BuildStatus::Running);
        assert_eq!(claimed.output_dir, root.join(format!("windows-{first}")));
        assert_eq!(claimed.log_path, root.join("logs").join(format!("windows-{first}.log")));

        store.update_tail(first, "▸ Godot").unwrap();
        store
            .finish_build(first, Path::new("builds/windows-1/game.exe"), 2048, "done")
            .unwrap();
        assert_eq!(store.cancel_queued(second).unwrap(), BuildStatus::Queued);
        assert!(!store.has_queued().unwrap());

        let history = store.builds(7).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].status, BuildStatus::Cancelled);
        assert_eq!(history[1].status, BuildStatus::Done);
        assert_eq!(history[1].size_bytes, Some(2048));
        assert_eq!(history[1].log_tail, "done");
        assert!(store.builds(8).unwrap().is_empty());
        assert_eq!(store.cancel_queued(99).err(), Some(BuildError::NotFound));
    }

    #[test]
    fn keeps_one_playtest_per_game_and_its_report() {
        let store = in_memory();
        let run = Path::new("games/heist/.noctics/playtest/run-1");
        assert!(store.playtest(5).unwrap().is_none());
        store.begin_playtest(5, run).unwrap();
        assert!(store.playtest_active(5).unwrap());
        assert_eq!(store.begin_playtest(5, run), Err(BuildError::PlaytestBusy));
        store
            .set_playtest_round(5, &run.join("round-1"), &["SCRIPT ERROR: boom".into()])
            .unwrap();
        store.set_playtest_status(5, PlaytestStatus::Reviewing).unwrap();
        store
            .set_playtest_activity(5, &["Читає 01.png".into(), "Редагує player.gd".into()])
            .unwrap();
        let busy = store.playtest(5).unwrap().unwrap();
        assert_eq!(busy.status, PlaytestStatus::Reviewing);
        assert_eq!(busy.round_dir.as_deref(), Some(run.join("round-1").as_path()));
        assert_eq!(busy.errors, ["SCRIPT ERROR: boom"]);
        assert_eq!(busy.activity.len(), 2);

        let report = PlaytestReport {
            verdict: noctics_shared::PlaytestVerdict::Fixed,
            summary: "Меню знову стартує гру".into(),
            issues: Vec::new(),
        };
        store.finish_playtest(5, Some(&report), None).unwrap();
        let done = store.playtest(5).unwrap().unwrap();
        assert_eq!(done.status, PlaytestStatus::Done);
        assert_eq!(done.report, Some(report));
        assert!(done.finished_at.is_some() && !store.playtest_active(5).unwrap());

        store.begin_playtest(5, run).unwrap();
        let fresh = store.playtest(5).unwrap().unwrap();
        assert!(fresh.report.is_none() && fresh.errors.is_empty() && fresh.round_dir.is_none());
        store.finish_playtest(5, None, Some("Godot не знайдено")).unwrap();
        assert_eq!(store.playtest(5).unwrap().unwrap().status, PlaytestStatus::Failed);

        store.queue_build(5, BuildTarget::Web, Path::new("builds")).unwrap();
        assert!(store.has_active_builds(5).unwrap());
        assert!(!store.has_active_builds(6).unwrap());
    }

    #[test]
    fn guards_the_source_while_claude_writes_it() {
        let store = in_memory();
        let folder = Path::new("games/heist/source");
        store
            .begin_generation(3, folder, SourceOrigin::Generated, GameEngine::Godot)
            .unwrap();
        assert_eq!(
            store.begin_generation(3, folder, SourceOrigin::Generated, GameEngine::Godot),
            Err(BuildError::SourceBusy)
        );
        assert_eq!(
            store.link_source(3, Path::new("C:/games/heist"), GameEngine::Godot),
            Err(BuildError::SourceBusy)
        );
        let lines: Vec<String> = (0..MAX_ACTIVITY + 5).map(|index| format!("крок {index}")).collect();
        store.set_activity(3, &lines).unwrap();
        let busy = store.source(3).unwrap().unwrap();
        assert_eq!(busy.status, SourceStatus::Generating);
        assert_eq!(busy.activity.len(), MAX_ACTIVITY);
        assert_eq!(busy.activity.last().unwrap(), &format!("крок {}", MAX_ACTIVITY + 4));

        store.finish_generation(3, Some("ліміт")).unwrap();
        let failed = store.source(3).unwrap().unwrap();
        assert_eq!(failed.status, SourceStatus::Failed);
        assert_eq!(failed.error.as_deref(), Some("ліміт"));

        store.link_source(3, Path::new("C:/games/heist"), GameEngine::Unity).unwrap();
        let linked = store.source(3).unwrap().unwrap();
        assert_eq!(linked.origin, SourceOrigin::Linked);
        assert_eq!(linked.engine, GameEngine::Unity);
        assert!(linked.activity.is_empty() && linked.error.is_none());
        assert!(store.source(4).unwrap().is_none());
    }
}
