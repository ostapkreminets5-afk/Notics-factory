use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use noctics_shared::{MixamoError, MotionClip, MotionOptions, MotionOrder, MotionStatus};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::accounts::unix_now;

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS motion_orders (
        id INTEGER PRIMARY KEY,
        project_id INTEGER,
        project_name TEXT,
        name TEXT NOT NULL,
        query TEXT NOT NULL,
        clip_id TEXT,
        clip TEXT,
        options TEXT NOT NULL,
        status TEXT NOT NULL,
        file TEXT,
        size_bytes INTEGER,
        error TEXT,
        created_at INTEGER NOT NULL,
        finished_at INTEGER
    );
    CREATE INDEX IF NOT EXISTS motion_orders_by_status ON motion_orders (status, id);
";
const COLUMNS: &str =
    "id, project_id, project_name, name, query, clip_id, clip, options, status, file, size_bytes, error, created_at, finished_at";

pub struct NewOrder<'a> {
    pub project: Option<(i64, String)>,
    pub name: &'a str,
    pub query: &'a str,
    pub clip_id: Option<&'a str>,
    pub options: MotionOptions,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredOrder {
    pub order: MotionOrder,
    pub clip_id: Option<String>,
}

pub struct MixamoStore {
    connection: Mutex<Connection>,
}

impl MixamoStore {
    pub fn open(path: &Path) -> Result<Self, MixamoError> {
        Self::prepare(Connection::open(path).map_err(storage)?)
    }

    fn prepare(connection: Connection) -> Result<Self, MixamoError> {
        connection.execute_batch(SCHEMA).map_err(storage)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, MixamoError> {
        self.connection
            .lock()
            .map_err(|_| MixamoError::Storage("animation lock is poisoned".into()))
    }

    pub fn recover(&self) -> Result<(), MixamoError> {
        self.lock()?
            .execute(
                "UPDATE motion_orders SET status = 'queued' WHERE status IN ('searching', 'exporting', 'downloading')",
                [],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn create(&self, order: &NewOrder<'_>) -> Result<i64, MixamoError> {
        let connection = self.lock()?;
        let (project_id, project_name) = order
            .project
            .as_ref()
            .map_or((None, None), |(id, name)| (Some(*id), Some(name.as_str())));
        connection
            .execute(
                "INSERT INTO motion_orders (project_id, project_name, name, query, clip_id, options, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    project_id,
                    project_name,
                    order.name,
                    order.query,
                    order.clip_id,
                    encode(&order.options)?,
                    MotionStatus::Queued.code(),
                    unix_now()
                ],
            )
            .map_err(storage)?;
        Ok(connection.last_insert_rowid())
    }

    pub fn twin(&self, order: &NewOrder<'_>) -> Result<Option<i64>, MixamoError> {
        let project_id = order.project.as_ref().map(|(id, _)| *id);
        self.lock()?
            .query_row(
                "SELECT id FROM motion_orders
                 WHERE project_id IS ?1 AND name = ?2 AND query = ?3 AND clip_id IS ?4 AND options = ?5 AND status != 'failed'
                 ORDER BY id DESC LIMIT 1",
                params![project_id, order.name, order.query, order.clip_id, encode(&order.options)?],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)
    }

    pub fn list(&self, limit: usize) -> Result<Vec<MotionOrder>, MixamoError> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(&format!("SELECT {COLUMNS} FROM motion_orders ORDER BY id DESC LIMIT ?1"))
            .map_err(storage)?;
        let rows = statement
            .query_map(params![i64::try_from(limit).unwrap_or(i64::MAX)], read)
            .map_err(storage)?;
        rows.map(|row| row.map(|stored| stored.order).map_err(storage)).collect()
    }

    pub fn order(&self, id: i64) -> Result<Option<StoredOrder>, MixamoError> {
        self.lock()?
            .query_row(&format!("SELECT {COLUMNS} FROM motion_orders WHERE id = ?1"), params![id], read)
            .optional()
            .map_err(storage)
    }

    pub fn next_queued(&self) -> Result<Option<StoredOrder>, MixamoError> {
        self.lock()?
            .query_row(
                &format!("SELECT {COLUMNS} FROM motion_orders WHERE status = 'queued' ORDER BY id LIMIT 1"),
                [],
                read,
            )
            .optional()
            .map_err(storage)
    }

    pub fn set_status(&self, id: i64, status: MotionStatus) -> Result<(), MixamoError> {
        self.lock()?
            .execute(
                "UPDATE motion_orders SET status = ?2 WHERE id = ?1",
                params![id, status.code()],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn set_clip(&self, id: i64, clip: &MotionClip) -> Result<(), MixamoError> {
        let encoded = serde_json::to_string(clip).map_err(|error| MixamoError::Storage(error.to_string()))?;
        self.lock()?
            .execute(
                "UPDATE motion_orders SET clip = ?2, clip_id = ?3 WHERE id = ?1",
                params![id, encoded, clip.id],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn finish(&self, id: i64, file: &Path, size: u64) -> Result<(), MixamoError> {
        self.lock()?
            .execute(
                "UPDATE motion_orders SET status = 'done', file = ?2, size_bytes = ?3, error = NULL, finished_at = ?4 WHERE id = ?1",
                params![
                    id,
                    file.to_string_lossy(),
                    i64::try_from(size).unwrap_or(i64::MAX),
                    unix_now()
                ],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn fail(&self, id: i64, error: &str) -> Result<(), MixamoError> {
        self.lock()?
            .execute(
                "UPDATE motion_orders SET status = 'failed', error = ?2, finished_at = ?3 WHERE id = ?1",
                params![id, error, unix_now()],
            )
            .map(drop)
            .map_err(storage)
    }

    pub fn requeue(&self, id: i64) -> Result<(), MixamoError> {
        let changed = self
            .lock()?
            .execute(
                "UPDATE motion_orders SET status = 'queued', error = NULL, finished_at = NULL
                 WHERE id = ?1 AND status IN ('done', 'failed')",
                params![id],
            )
            .map_err(storage)?;
        if changed == 0 {
            return Err(MixamoError::NotFound);
        }
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<Option<String>, MixamoError> {
        let stored = self.order(id)?.ok_or(MixamoError::NotFound)?;
        if stored.order.status.is_active() {
            return Err(MixamoError::Busy);
        }
        self.lock()?
            .execute("DELETE FROM motion_orders WHERE id = ?1", params![id])
            .map_err(storage)?;
        Ok(stored.order.file)
    }
}

fn read(row: &Row<'_>) -> rusqlite::Result<StoredOrder> {
    let clip: Option<String> = row.get(6)?;
    let options: String = row.get(7)?;
    let status: String = row.get(8)?;
    let size: Option<i64> = row.get(10)?;
    Ok(StoredOrder {
        order: MotionOrder {
            id: row.get(0)?,
            project_id: row.get(1)?,
            project_name: row.get(2)?,
            name: row.get(3)?,
            query: row.get(4)?,
            clip: clip.and_then(|text| serde_json::from_str(&text).ok()),
            options: serde_json::from_str(&options).unwrap_or_default(),
            status: MotionStatus::from_code(&status).unwrap_or(MotionStatus::Failed),
            file: row.get(9)?,
            size_bytes: size.and_then(|bytes| u64::try_from(bytes).ok()),
            error: row.get(11)?,
            created_at: row.get(12)?,
            finished_at: row.get(13)?,
        },
        clip_id: row.get(5)?,
    })
}

fn encode(options: &MotionOptions) -> Result<String, MixamoError> {
    serde_json::to_string(options).map_err(|error| MixamoError::Storage(error.to_string()))
}

fn storage(error: rusqlite::Error) -> MixamoError {
    MixamoError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MixamoStore {
        MixamoStore::prepare(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn order<'a>(name: &'a str, query: &'a str) -> NewOrder<'a> {
        NewOrder {
            project: Some((3, "Нічний злодій".into())),
            name,
            query,
            clip_id: None,
            options: MotionOptions::default(),
        }
    }

    #[test]
    fn walks_an_order_through_the_queue() {
        let store = store();
        let first = store.create(&order("run", "running")).unwrap();
        let second = store.create(&order("idle", "idle")).unwrap();
        assert_eq!(store.next_queued().unwrap().unwrap().order.id, first);
        store.set_status(first, MotionStatus::Exporting).unwrap();
        assert_eq!(store.next_queued().unwrap().unwrap().order.id, second);
        let clip = MotionClip {
            id: "m-1".into(),
            name: "Running".into(),
            ..MotionClip::default()
        };
        store.set_clip(first, &clip).unwrap();
        store.recover().unwrap();
        assert_eq!(store.next_queued().unwrap().unwrap().order.id, first);
        store.finish(first, Path::new("lib/run.fbx"), 2048).unwrap();
        let done = store.order(first).unwrap().unwrap();
        assert_eq!(done.order.status, MotionStatus::Done);
        assert_eq!(done.order.clip.unwrap().name, "Running");
        assert_eq!(done.clip_id.as_deref(), Some("m-1"));
        assert_eq!(done.order.size_bytes, Some(2048));
        assert_eq!(done.order.project_name.as_deref(), Some("Нічний злодій"));
        store.fail(second, "немає").unwrap();
        store.requeue(second).unwrap();
        assert_eq!(store.order(second).unwrap().unwrap().order.error, None);
        assert_eq!(store.list(10).unwrap().len(), 2);
    }

    #[test]
    fn reuses_twin_orders_until_they_fail() {
        let store = store();
        let id = store.create(&order("run", "running")).unwrap();
        assert_eq!(store.twin(&order("run", "running")).unwrap(), Some(id));
        assert_eq!(store.twin(&order("run", "sprint")).unwrap(), None);
        let in_place = NewOrder {
            options: MotionOptions {
                in_place: true,
                ..MotionOptions::default()
            },
            ..order("run", "running")
        };
        assert_eq!(store.twin(&in_place).unwrap(), None);
        store.fail(id, "x").unwrap();
        assert_eq!(store.twin(&order("run", "running")).unwrap(), None);
    }

    #[test]
    fn refuses_to_delete_running_orders() {
        let store = store();
        let id = store.create(&order("run", "running")).unwrap();
        store.set_status(id, MotionStatus::Downloading).unwrap();
        assert_eq!(store.delete(id), Err(MixamoError::Busy));
        store.finish(id, Path::new("lib/run.fbx"), 1).unwrap();
        assert_eq!(store.delete(id).unwrap().as_deref(), Some("lib/run.fbx"));
        assert_eq!(store.delete(id), Err(MixamoError::NotFound));
    }
}
