use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use noctics_shared::{
    Build, BuildError, BuildStatus, BuildTarget, GameEngine, SourceOrigin, SourceStatus,
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
";
const REQUEUE_INTERRUPTED: &str =
    "UPDATE builds SET status = 'queued', started_at = NULL WHERE status = 'running'";
const INTERRUPTED_GENERATION: &str =
    "Генерацію перервано: застосунок закрили. Запусти її ще раз.";
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
        Ok(Self {
            connection: Mutex::new(connection),
        })
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
                    activity: activity
                        .lines()
                        .filter(|line| !line.is_empty())
                        .map(str::to_owned)
                        .collect(),
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
