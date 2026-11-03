use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use noctics_shared::{AssetKind, ComfyError, Medium, RunStatus, TaskOrigin, TaskStatus};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::accounts::unix_now;
use crate::comfy::OutputRef;

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS asset_tasks (
        id INTEGER PRIMARY KEY,
        kind TEXT NOT NULL,
        brief TEXT NOT NULL,
        title TEXT NOT NULL DEFAULT '',
        plan TEXT,
        origin TEXT NOT NULL,
        project_id INTEGER,
        project_name TEXT,
        status TEXT NOT NULL,
        note TEXT,
        seed INTEGER NOT NULL,
        created_at INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS model_runs (
        id INTEGER PRIMARY KEY,
        task_id INTEGER NOT NULL,
        position INTEGER NOT NULL,
        model_id TEXT NOT NULL,
        model_name TEXT NOT NULL,
        reference INTEGER NOT NULL DEFAULT 0,
        status TEXT NOT NULL,
        files TEXT NOT NULL DEFAULT '[]',
        error TEXT,
        started_at INTEGER,
        finished_at INTEGER
    );
    CREATE INDEX IF NOT EXISTS model_runs_by_task ON model_runs (task_id, position);
    CREATE TABLE IF NOT EXISTS disabled_models (
        model_id TEXT PRIMARY KEY
    );
";
const TASK_COLUMNS: &str =
    "id, kind, brief, title, plan, origin, project_name, status, note, seed, created_at";
const RUN_COLUMNS: &str =
    "id, task_id, model_id, model_name, reference, status, files, error, started_at, finished_at";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Plan {
    pub title: String,
    pub prompt: String,
    pub tags: String,
    pub negative: String,
    pub aspect: String,
    pub seconds: f64,
    pub bpm: u32,
    pub keyscale: String,
    pub lyrics: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredFile {
    pub name: String,
    pub medium: Medium,
    pub source: OutputRef,
}

pub struct NewRun {
    pub model_id: String,
    pub model_name: String,
    pub reference: bool,
}

pub struct NewTask<'a> {
    pub kind: AssetKind,
    pub brief: &'a str,
    pub origin: TaskOrigin,
    pub project: Option<(i64, String)>,
    pub plan: Option<Plan>,
    pub seed: u64,
    pub runs: Vec<NewRun>,
}

pub struct WritingJob {
    pub task_id: i64,
    pub kind: AssetKind,
    pub brief: String,
    pub project_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredRun {
    pub id: i64,
    pub task_id: i64,
    pub model_id: String,
    pub model_name: String,
    pub reference: bool,
    pub status: RunStatus,
    pub files: Vec<StoredFile>,
    pub error: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredTask {
    pub id: i64,
    pub kind: AssetKind,
    pub brief: String,
    pub title: String,
    pub plan: Option<Plan>,
    pub origin: TaskOrigin,
    pub project_name: Option<String>,
    pub status: TaskStatus,
    pub note: Option<String>,
    pub seed: u64,
    pub created_at: i64,
    pub runs: Vec<StoredRun>,
}

pub struct ComfyStore {
    connection: Mutex<Connection>,
}

impl ComfyStore {
    pub fn open(path: &Path) -> Result<Self, ComfyError> {
        Self::prepare(Connection::open(path).map_err(storage)?)
    }

    fn prepare(connection: Connection) -> Result<Self, ComfyError> {
        connection.execute_batch(SCHEMA).map_err(storage)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, ComfyError> {
        self.connection
            .lock()
            .map_err(|_| ComfyError::Storage("gallery lock is poisoned".into()))
    }

    pub fn recover(&self) -> Result<(), ComfyError> {
        let connection = self.lock()?;
        connection
            .execute_batch(
                "UPDATE asset_tasks SET status = 'queued' WHERE status = 'running';
                 UPDATE model_runs SET status = 'queued', started_at = NULL WHERE status = 'running';",
            )
            .map_err(storage)
    }

    pub fn create(&self, task: &NewTask<'_>) -> Result<i64, ComfyError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(storage)?;
        let status = if task.plan.is_some() {
            TaskStatus::Queued
        } else {
            TaskStatus::Writing
        };
        let plan = task.plan.as_ref().map(encode_plan).transpose()?;
        let title = task.plan.as_ref().map(|plan| plan.title.clone()).unwrap_or_default();
        let (project_id, project_name) = task
            .project
            .as_ref()
            .map_or((None, None), |(id, name)| (Some(*id), Some(name.as_str())));
        transaction
            .execute(
                "INSERT INTO asset_tasks (kind, brief, title, plan, origin, project_id, project_name, status, seed, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    task.kind.code(),
                    task.brief,
                    title,
                    plan,
                    task.origin.code(),
                    project_id,
                    project_name,
                    status.code(),
                    i64::try_from(task.seed).unwrap_or(i64::MAX),
                    unix_now()
                ],
            )
            .map_err(storage)?;
        let task_id = transaction.last_insert_rowid();
        for (position, run) in task.runs.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO model_runs (task_id, position, model_id, model_name, reference, status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        task_id,
                        i64::try_from(position).unwrap_or(i64::MAX),
                        run.model_id,
                        run.model_name,
                        run.reference,
                        RunStatus::Queued.code()
                    ],
                )
                .map_err(storage)?;
        }
        transaction.commit().map_err(storage)?;
        Ok(task_id)
    }

    pub fn next_writing(&self) -> Result<Option<WritingJob>, ComfyError> {
        let connection = self.lock()?;
        connection
            .query_row(
                "SELECT id, kind, brief, project_id FROM asset_tasks WHERE status = 'writing' ORDER BY id LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(storage)
            .map(|found| {
                found.map(|(task_id, kind, brief, project_id)| WritingJob {
                    task_id,
                    kind: AssetKind::from_code(&kind).unwrap_or(AssetKind::Concept),
                    brief,
                    project_id,
                })
            })
    }

    pub fn set_plan(&self, task_id: i64, plan: &Plan, note: Option<&str>) -> Result<(), ComfyError> {
        let connection = self.lock()?;
        connection
            .execute(
                "UPDATE asset_tasks SET plan = ?1, title = ?2, note = ?3, status = 'queued'
                 WHERE id = ?4 AND status = 'writing'",
                params![encode_plan(plan)?, plan.title, note, task_id],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn has_status(&self, status: TaskStatus) -> Result<bool, ComfyError> {
        let connection = self.lock()?;
        connection
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM asset_tasks WHERE status = ?1)",
                params![status.code()],
                |row| row.get::<_, bool>(0),
            )
            .map_err(storage)
    }

    pub fn claim_queued(&self) -> Result<Option<StoredTask>, ComfyError> {
        let connection = self.lock()?;
        let task_id = connection
            .query_row(
                "SELECT id FROM asset_tasks WHERE status = 'queued' ORDER BY id LIMIT 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(storage)?;
        let Some(task_id) = task_id else {
            return Ok(None);
        };
        connection
            .execute(
                "UPDATE asset_tasks SET status = 'running' WHERE id = ?1",
                params![task_id],
            )
            .map_err(storage)?;
        load_task(&connection, task_id)
    }

    pub fn task(&self, task_id: i64) -> Result<Option<StoredTask>, ComfyError> {
        load_task(&*self.lock()?, task_id)
    }

    pub fn exists(&self, task_id: i64) -> Result<bool, ComfyError> {
        let connection = self.lock()?;
        connection
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM asset_tasks WHERE id = ?1)",
                params![task_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(storage)
    }

    pub fn start_run(&self, run_id: i64) -> Result<(), ComfyError> {
        let connection = self.lock()?;
        connection
            .execute(
                "UPDATE model_runs SET status = 'running', started_at = ?2, finished_at = NULL, error = NULL, files = '[]' WHERE id = ?1",
                params![run_id, unix_now()],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn finish_run(&self, run_id: i64, files: &[StoredFile]) -> Result<(), ComfyError> {
        let encoded = serde_json::to_string(files).map_err(|error| ComfyError::Storage(error.to_string()))?;
        let connection = self.lock()?;
        connection
            .execute(
                "UPDATE model_runs SET status = 'done', files = ?2, finished_at = ?3 WHERE id = ?1",
                params![run_id, encoded, unix_now()],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn fail_run(&self, run_id: i64, error: &str) -> Result<(), ComfyError> {
        self.close_run(run_id, RunStatus::Failed, error)
    }

    pub fn skip_run(&self, run_id: i64, reason: &str) -> Result<(), ComfyError> {
        self.close_run(run_id, RunStatus::Skipped, reason)
    }

    pub fn requeue_run(&self, run_id: i64) -> Result<(), ComfyError> {
        let connection = self.lock()?;
        connection
            .execute(
                "UPDATE model_runs SET status = 'queued', started_at = NULL, finished_at = NULL WHERE id = ?1",
                params![run_id],
            )
            .map(drop)
            .map_err(storage)
    }

    fn close_run(&self, run_id: i64, status: RunStatus, message: &str) -> Result<(), ComfyError> {
        let connection = self.lock()?;
        connection
            .execute(
                "UPDATE model_runs SET status = ?2, error = ?3, finished_at = ?4 WHERE id = ?1",
                params![run_id, status.code(), message, unix_now()],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn requeue_task(&self, task_id: i64) -> Result<(), ComfyError> {
        let connection = self.lock()?;
        connection
            .execute(
                "UPDATE asset_tasks SET status = 'queued' WHERE id = ?1",
                params![task_id],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn settle_task(&self, task_id: i64) -> Result<(), ComfyError> {
        let connection = self.lock()?;
        let counts = |status: RunStatus| -> Result<i64, ComfyError> {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM model_runs WHERE task_id = ?1 AND status = ?2",
                    params![task_id, status.code()],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(storage)
        };
        let status = if counts(RunStatus::Queued)? > 0 {
            TaskStatus::Queued
        } else if counts(RunStatus::Done)? > 0 {
            TaskStatus::Done
        } else {
            TaskStatus::Failed
        };
        connection
            .execute(
                "UPDATE asset_tasks SET status = ?2 WHERE id = ?1",
                params![task_id, status.code()],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn retry_run(&self, run_id: i64) -> Result<i64, ComfyError> {
        let connection = self.lock()?;
        let task_id = connection
            .query_row(
                "SELECT task_id FROM model_runs WHERE id = ?1 AND status IN ('done', 'failed', 'skipped')",
                params![run_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(storage)?
            .ok_or(ComfyError::NotFound)?;
        connection
            .execute(
                "UPDATE model_runs SET status = 'queued', files = '[]', error = NULL, started_at = NULL, finished_at = NULL WHERE id = ?1",
                params![run_id],
            )
            .map_err(storage)?;
        connection
            .execute(
                "UPDATE asset_tasks SET status = 'queued' WHERE id = ?1 AND status IN ('done', 'failed')",
                params![task_id],
            )
            .map_err(storage)?;
        Ok(task_id)
    }

    pub fn delete_task(&self, task_id: i64) -> Result<(), ComfyError> {
        let connection = self.lock()?;
        let removed = connection
            .execute("DELETE FROM asset_tasks WHERE id = ?1", params![task_id])
            .map_err(storage)?;
        connection
            .execute("DELETE FROM model_runs WHERE task_id = ?1", params![task_id])
            .map_err(storage)?;
        if removed == 0 {
            Err(ComfyError::NotFound)
        } else {
            Ok(())
        }
    }

    pub fn list(&self, limit: usize) -> Result<Vec<StoredTask>, ComfyError> {
        let connection = self.lock()?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut statement = connection
            .prepare(&format!(
                "SELECT {TASK_COLUMNS} FROM asset_tasks ORDER BY id DESC LIMIT ?1"
            ))
            .map_err(storage)?;
        let mut tasks = statement
            .query_map(params![limit], read_task)
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?;
        let mut runs = connection
            .prepare(&format!(
                "SELECT {RUN_COLUMNS} FROM model_runs
                 WHERE task_id IN (SELECT id FROM asset_tasks ORDER BY id DESC LIMIT ?1)
                 ORDER BY task_id, position"
            ))
            .map_err(storage)?;
        let mut grouped: HashMap<i64, Vec<StoredRun>> = HashMap::new();
        for run in runs
            .query_map(params![limit], read_run)
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?
        {
            grouped.entry(run.task_id).or_default().push(run);
        }
        for task in &mut tasks {
            task.runs = grouped.remove(&task.id).unwrap_or_default();
        }
        Ok(tasks)
    }

    pub fn run(&self, run_id: i64) -> Result<StoredRun, ComfyError> {
        let connection = self.lock()?;
        connection
            .query_row(
                &format!("SELECT {RUN_COLUMNS} FROM model_runs WHERE id = ?1"),
                params![run_id],
                read_run,
            )
            .optional()
            .map_err(storage)?
            .ok_or(ComfyError::NotFound)
    }

    pub fn disabled(&self) -> Result<HashSet<String>, ComfyError> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare("SELECT model_id FROM disabled_models")
            .map_err(storage)?;
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage)?
            .collect::<Result<HashSet<_>, _>>()
            .map_err(storage)
    }

    pub fn set_enabled(&self, model_id: &str, enabled: bool) -> Result<(), ComfyError> {
        let connection = self.lock()?;
        let sql = if enabled {
            "DELETE FROM disabled_models WHERE model_id = ?1"
        } else {
            "INSERT OR IGNORE INTO disabled_models (model_id) VALUES (?1)"
        };
        connection
            .execute(sql, params![model_id])
            .map(drop)
            .map_err(storage)
    }
}

fn load_task(connection: &Connection, task_id: i64) -> Result<Option<StoredTask>, ComfyError> {
    let task = connection
        .query_row(
            &format!("SELECT {TASK_COLUMNS} FROM asset_tasks WHERE id = ?1"),
            params![task_id],
            read_task,
        )
        .optional()
        .map_err(storage)?;
    let Some(mut task) = task else {
        return Ok(None);
    };
    let mut statement = connection
        .prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM model_runs WHERE task_id = ?1 ORDER BY position"
        ))
        .map_err(storage)?;
    task.runs = statement
        .query_map(params![task_id], read_run)
        .map_err(storage)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage)?;
    Ok(Some(task))
}

fn read_task(row: &Row<'_>) -> rusqlite::Result<StoredTask> {
    let kind: String = row.get(1)?;
    let plan: Option<String> = row.get(4)?;
    let origin: String = row.get(5)?;
    let status: String = row.get(7)?;
    let seed: i64 = row.get(9)?;
    Ok(StoredTask {
        id: row.get(0)?,
        kind: AssetKind::from_code(&kind).unwrap_or(AssetKind::Concept),
        brief: row.get(2)?,
        title: row.get(3)?,
        plan: plan.and_then(|text| serde_json::from_str(&text).ok()),
        origin: TaskOrigin::from_code(&origin),
        project_name: row.get(6)?,
        status: TaskStatus::from_code(&status).unwrap_or(TaskStatus::Failed),
        note: row.get(8)?,
        seed: u64::try_from(seed).unwrap_or_default(),
        created_at: row.get(10)?,
        runs: Vec::new(),
    })
}

fn read_run(row: &Row<'_>) -> rusqlite::Result<StoredRun> {
    let status: String = row.get(5)?;
    let files: String = row.get(6)?;
    Ok(StoredRun {
        id: row.get(0)?,
        task_id: row.get(1)?,
        model_id: row.get(2)?,
        model_name: row.get(3)?,
        reference: row.get(4)?,
        status: RunStatus::from_code(&status).unwrap_or(RunStatus::Failed),
        files: serde_json::from_str(&files).unwrap_or_default(),
        error: row.get(7)?,
        started_at: row.get(8)?,
        finished_at: row.get(9)?,
    })
}

fn encode_plan(plan: &Plan) -> Result<String, ComfyError> {
    serde_json::to_string(plan).map_err(|error| ComfyError::Storage(error.to_string()))
}

fn storage(error: rusqlite::Error) -> ComfyError {
    ComfyError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory() -> ComfyStore {
        ComfyStore::prepare(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn runs() -> Vec<NewRun> {
        ["z-image", "flux2"]
            .into_iter()
            .map(|id| NewRun {
                model_id: id.into(),
                model_name: id.to_uppercase(),
                reference: false,
            })
            .collect()
    }

    fn task(plan: Option<Plan>) -> NewTask<'static> {
        NewTask {
            kind: AssetKind::Sprite,
            brief: "вогняний меч",
            origin: TaskOrigin::Studio,
            project: Some((3, "Нічне місто".into())),
            plan,
            seed: 42,
            runs: runs(),
        }
    }

    #[test]
    fn tasks_wait_for_claude_then_run_every_model() {
        let store = in_memory();
        let id = store.create(&task(None)).unwrap();
        assert!(store.claim_queued().unwrap().is_none());
        let writing = store.next_writing().unwrap().unwrap();
        assert_eq!((writing.task_id, writing.brief.as_str()), (id, "вогняний меч"));

        let plan = Plan {
            title: "Вогняний меч".into(),
            prompt: "a fire sword".into(),
            ..Plan::default()
        };
        store.set_plan(id, &plan, None).unwrap();
        let claimed = store.claim_queued().unwrap().unwrap();
        assert_eq!(claimed.status, TaskStatus::Running);
        assert_eq!(claimed.plan.unwrap().prompt, "a fire sword");
        assert_eq!(claimed.runs.len(), 2);

        let first = claimed.runs[0].id;
        let second = claimed.runs[1].id;
        store.start_run(first).unwrap();
        let file = StoredFile {
            name: format!("{first}-0.png"),
            medium: Medium::Image,
            source: OutputRef { filename: "a.png".into(), subfolder: "noctics/1".into() },
        };
        store.finish_run(first, std::slice::from_ref(&file)).unwrap();
        store.fail_run(second, "OOM").unwrap();
        store.settle_task(id).unwrap();

        let listed = store.list(10).unwrap();
        assert_eq!(listed[0].status, TaskStatus::Done);
        assert_eq!(listed[0].title, "Вогняний меч");
        assert_eq!(listed[0].project_name.as_deref(), Some("Нічне місто"));
        assert_eq!(listed[0].runs[0].files, vec![file]);
        assert_eq!(listed[0].runs[1].error.as_deref(), Some("OOM"));

        assert_eq!(store.retry_run(second).unwrap(), id);
        assert_eq!(store.task(id).unwrap().unwrap().status, TaskStatus::Queued);
        assert_eq!(store.retry_run(second), Err(ComfyError::NotFound));
    }

    #[test]
    fn interrupted_work_is_resumed_and_deleted_tasks_vanish() {
        let store = in_memory();
        let id = store.create(&task(Some(Plan::default()))).unwrap();
        let claimed = store.claim_queued().unwrap().unwrap();
        store.start_run(claimed.runs[0].id).unwrap();
        store.recover().unwrap();
        let resumed = store.task(id).unwrap().unwrap();
        assert_eq!(resumed.status, TaskStatus::Queued);
        assert_eq!(resumed.runs[0].status, RunStatus::Queued);

        store.delete_task(id).unwrap();
        assert!(!store.exists(id).unwrap());
        assert!(store.list(10).unwrap().is_empty());
        assert_eq!(store.delete_task(id), Err(ComfyError::NotFound));
    }

    #[test]
    fn remembers_switched_off_models() {
        let store = in_memory();
        store.set_enabled("flux2:x", false).unwrap();
        store.set_enabled("flux2:x", false).unwrap();
        assert!(store.disabled().unwrap().contains("flux2:x"));
        store.set_enabled("flux2:x", true).unwrap();
        assert!(store.disabled().unwrap().is_empty());
    }
}
