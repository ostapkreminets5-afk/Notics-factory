use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use noctics_shared::{AssetFind, AssetHunt, HuntStatus, StudioError};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::accounts::unix_now;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS asset_hunts (
    project_id INTEGER PRIMARY KEY,
    status TEXT NOT NULL,
    request TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL DEFAULT '',
    finds TEXT NOT NULL DEFAULT '[]',
    error TEXT,
    queued_at INTEGER NOT NULL,
    started_at INTEGER,
    finished_at INTEGER
);
";
const RECOVER: &str = "UPDATE asset_hunts SET status = 'queued', started_at = NULL WHERE status = 'searching'";
const COLUMNS: &str = "project_id, status, request, summary, finds, error, started_at, finished_at";

pub struct Claimed {
    pub project_id: i64,
    pub request: String,
}

pub struct HuntStore {
    connection: Mutex<Connection>,
}

impl HuntStore {
    pub fn open(path: &Path) -> Result<Self, StudioError> {
        Self::prepare(Connection::open(path).map_err(storage)?)
    }

    fn prepare(connection: Connection) -> Result<Self, StudioError> {
        connection.execute_batch(SCHEMA).map_err(storage)?;
        connection.execute_batch(RECOVER).map_err(storage)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn hunt(&self, project_id: i64) -> Result<Option<AssetHunt>, StudioError> {
        let connection = self.lock()?;
        find(&connection, project_id)
    }

    pub fn queue(&self, project_id: i64, request: &str) -> Result<AssetHunt, StudioError> {
        let connection = self.lock()?;
        if let Some(current) = find(&connection, project_id)?.filter(|hunt| hunt.status.is_active()) {
            return Ok(current);
        }
        connection
            .execute(
                "INSERT INTO asset_hunts (project_id, status, request, queued_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(project_id) DO UPDATE SET status = excluded.status, request = excluded.request,
                 error = NULL, queued_at = excluded.queued_at, started_at = NULL, finished_at = NULL",
                params![project_id, HuntStatus::Queued.code(), request, unix_now()],
            )
            .map_err(storage)?;
        find(&connection, project_id)?.ok_or(StudioError::NotFound)
    }

    pub fn queue_if_new(&self, project_id: i64) -> Result<bool, StudioError> {
        let connection = self.lock()?;
        let added = connection
            .execute(
                "INSERT OR IGNORE INTO asset_hunts (project_id, status, queued_at) VALUES (?1, ?2, ?3)",
                params![project_id, HuntStatus::Queued.code(), unix_now()],
            )
            .map_err(storage)?;
        Ok(added > 0)
    }

    pub fn has_queued(&self) -> Result<bool, StudioError> {
        let connection = self.lock()?;
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM asset_hunts WHERE status = ?1)",
                params![HuntStatus::Queued.code()],
                |row| row.get(0),
            )
            .map_err(storage)
    }

    pub fn claim(&self) -> Result<Option<Claimed>, StudioError> {
        let connection = self.lock()?;
        let next = connection
            .query_row(
                "SELECT project_id, request FROM asset_hunts WHERE status = ?1 ORDER BY queued_at, project_id LIMIT 1",
                params![HuntStatus::Queued.code()],
                |row| {
                    Ok(Claimed {
                        project_id: row.get(0)?,
                        request: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(storage)?;
        if let Some(claimed) = &next {
            connection
                .execute(
                    "UPDATE asset_hunts SET status = ?1, started_at = ?2 WHERE project_id = ?3",
                    params![HuntStatus::Searching.code(), unix_now(), claimed.project_id],
                )
                .map_err(storage)?;
        }
        Ok(next)
    }

    pub fn finish(&self, project_id: i64, summary: &str, finds: &[AssetFind]) -> Result<(), StudioError> {
        let encoded = serde_json::to_string(finds).map_err(|error| StudioError::Storage(error.to_string()))?;
        let connection = self.lock()?;
        connection
            .execute(
                "UPDATE asset_hunts SET status = ?1, summary = ?2, finds = ?3, error = NULL, finished_at = ?4
                 WHERE project_id = ?5",
                params![HuntStatus::Done.code(), summary, encoded, unix_now(), project_id],
            )
            .map_err(storage)?;
        Ok(())
    }

    pub fn fail(&self, project_id: i64, problem: &StudioError) -> Result<(), StudioError> {
        let encoded = serde_json::to_string(problem).unwrap_or_else(|_| problem.to_string());
        let connection = self.lock()?;
        connection
            .execute(
                "UPDATE asset_hunts SET status = ?1, error = ?2, finished_at = ?3 WHERE project_id = ?4",
                params![HuntStatus::Failed.code(), encoded, unix_now(), project_id],
            )
            .map_err(storage)?;
        Ok(())
    }

    pub fn forget(&self, project_id: i64) -> Result<(), StudioError> {
        let connection = self.lock()?;
        connection
            .execute("DELETE FROM asset_hunts WHERE project_id = ?1", params![project_id])
            .map_err(storage)?;
        Ok(())
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StudioError> {
        self.connection
            .lock()
            .map_err(|_| StudioError::Storage("asset hunt storage lock was poisoned".into()))
    }
}

fn find(connection: &Connection, project_id: i64) -> Result<Option<AssetHunt>, StudioError> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM asset_hunts WHERE project_id = ?1"),
            params![project_id],
            read_hunt,
        )
        .optional()
        .map_err(storage)
}

fn read_hunt(row: &Row<'_>) -> rusqlite::Result<AssetHunt> {
    let status: String = row.get(1)?;
    let finds: String = row.get(4)?;
    let error: Option<String> = row.get(5)?;
    Ok(AssetHunt {
        project_id: row.get(0)?,
        status: HuntStatus::from_code(&status).unwrap_or(HuntStatus::Failed),
        request: row.get(2)?,
        summary: row.get(3)?,
        finds: serde_json::from_str(&finds).unwrap_or_default(),
        error: error.map(|text| serde_json::from_str(&text).unwrap_or(StudioError::Claude(text))),
        started_at: row.get(6)?,
        finished_at: row.get(7)?,
    })
}

fn storage(error: rusqlite::Error) -> StudioError {
    StudioError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use noctics_shared::{AssetCost, FindKind};

    use super::*;

    fn store() -> HuntStore {
        HuntStore::prepare(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn find_named(title: &str) -> AssetFind {
        AssetFind {
            title: title.into(),
            url: "https://pixel-boy.itch.io/ninja-adventure-asset-pack".into(),
            site: "itch.io".into(),
            kind: FindKind::Characters,
            author: "Pixel Boy".into(),
            license: "CC0".into(),
            cost: AssetCost::Free,
            price: String::new(),
            fit: "Той самий стиль".into(),
            image: None,
        }
    }

    #[test]
    fn walks_a_hunt_from_queue_to_finds() {
        let store = store();
        assert!(store.hunt(7).unwrap().is_none());
        let queued = store.queue(7, "ліс").unwrap();
        assert_eq!(queued.status, HuntStatus::Queued);
        assert_eq!(queued.request, "ліс");
        assert!(store.has_queued().unwrap());

        let claimed = store.claim().unwrap().unwrap();
        assert_eq!(claimed.project_id, 7);
        assert_eq!(claimed.request, "ліс");
        assert!(store.claim().unwrap().is_none());
        assert_eq!(store.queue(7, "інше").unwrap().status, HuntStatus::Searching);

        store.finish(7, "Знайшов піксельні паки", &[find_named("Ninja")]).unwrap();
        let done = store.hunt(7).unwrap().unwrap();
        assert_eq!(done.status, HuntStatus::Done);
        assert_eq!(done.finds, vec![find_named("Ninja")]);
        assert!(done.finished_at.is_some());

        store.queue(7, "").unwrap();
        store.claim().unwrap();
        store.fail(7, &StudioError::UsageLimit).unwrap();
        let failed = store.hunt(7).unwrap().unwrap();
        assert_eq!(failed.status, HuntStatus::Failed);
        assert_eq!(failed.error, Some(StudioError::UsageLimit));
        assert_eq!(failed.finds.len(), 1);
    }

    #[test]
    fn queues_automatic_hunts_only_once_and_recovers_after_restart() {
        let store = store();
        assert!(store.queue_if_new(3).unwrap());
        assert!(!store.queue_if_new(3).unwrap());
        store.claim().unwrap();
        store.lock().unwrap().execute_batch(RECOVER).unwrap();
        let recovered = store.hunt(3).unwrap().unwrap();
        assert_eq!(recovered.status, HuntStatus::Queued);
        assert_eq!(recovered.started_at, None);
        store.forget(3).unwrap();
        assert!(store.hunt(3).unwrap().is_none());
    }
}
