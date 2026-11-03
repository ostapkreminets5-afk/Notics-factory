use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use noctics_shared::{
    ARTIFACTS_CHANGED_EVENT, ArtifactDraft, ArtifactError, ArtifactOrigin, DesignArtifact,
    artifact_code, artifact_link, title_from_page,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use url::Url;

use crate::accounts::unix_now;

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS design_artifacts (
        id INTEGER PRIMARY KEY,
        title TEXT NOT NULL,
        link TEXT NOT NULL UNIQUE,
        custom_title INTEGER NOT NULL,
        added_at INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS dismissed_artifacts (
        link TEXT PRIMARY KEY
    );
    CREATE TABLE IF NOT EXISTS transcript_offsets (
        path TEXT PRIMARY KEY,
        offset INTEGER NOT NULL
    );
";
const ORIGIN_COLUMN: &str = "origin";
const ADD_ORIGIN_COLUMN: &str =
    "ALTER TABLE design_artifacts ADD COLUMN origin TEXT NOT NULL DEFAULT 'manual'";
const SELECT_ARTIFACT: &str =
    "SELECT id, title, link, custom_title, origin, added_at FROM design_artifacts";
const DEFAULT_TITLE_PREFIX: &str = "Артефакт";
const WINDOW_PREFIX: &str = "claude-artifact-";
const WINDOW_TITLE_SUFFIX: &str = "Claude Design · Noctics";
const WINDOW_WIDTH: f64 = 1320.0;
const WINDOW_HEIGHT: f64 = 880.0;
const WINDOW_MIN_WIDTH: f64 = 720.0;
const WINDOW_MIN_HEIGHT: f64 = 520.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundArtifact {
    pub link: String,
    pub title: Option<String>,
    pub created_at: Option<i64>,
}

pub struct ArtifactStore {
    connection: Mutex<Connection>,
}

impl ArtifactStore {
    pub fn open(path: &Path) -> Result<Self, ArtifactError> {
        Self::prepare(Connection::open(path).map_err(storage)?)
    }

    fn prepare(connection: Connection) -> Result<Self, ArtifactError> {
        connection.execute_batch(SCHEMA).map_err(storage)?;
        if !has_origin_column(&connection)? {
            connection.execute_batch(ADD_ORIGIN_COLUMN).map_err(storage)?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn list(&self) -> Result<Vec<DesignArtifact>, ArtifactError> {
        list(&*self.lock()?)
    }

    pub fn get(&self, id: i64) -> Result<DesignArtifact, ArtifactError> {
        self.lock()?
            .query_row(&format!("{SELECT_ARTIFACT} WHERE id = ?1"), params![id], read_artifact)
            .optional()
            .map_err(storage)?
            .ok_or(ArtifactError::NotFound)
    }

    pub fn add(&self, draft: &ArtifactDraft) -> Result<Vec<DesignArtifact>, ArtifactError> {
        let draft = draft.normalized()?;
        let connection = self.lock()?;
        if id_of(&connection, &draft.link)?.is_some() {
            return Err(ArtifactError::Duplicate);
        }
        let custom_title = !draft.title.is_empty();
        let title = if custom_title {
            draft.title
        } else {
            default_title(&draft.link)
        };
        connection
            .execute(
                "INSERT INTO design_artifacts (title, link, custom_title, origin, added_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![title, draft.link, custom_title, ArtifactOrigin::Manual.code(), unix_now()],
            )
            .map_err(storage)?;
        connection
            .execute("DELETE FROM dismissed_artifacts WHERE link = ?1", params![draft.link])
            .map_err(storage)?;
        list(&connection)
    }

    pub fn remove(&self, id: i64) -> Result<Vec<DesignArtifact>, ArtifactError> {
        let connection = self.lock()?;
        let link: String = connection
            .query_row("SELECT link FROM design_artifacts WHERE id = ?1", params![id], |row| row.get(0))
            .optional()
            .map_err(storage)?
            .ok_or(ArtifactError::NotFound)?;
        connection
            .execute("DELETE FROM design_artifacts WHERE id = ?1", params![id])
            .map_err(storage)?;
        connection
            .execute("INSERT OR IGNORE INTO dismissed_artifacts (link) VALUES (?1)", params![link])
            .map_err(storage)?;
        list(&connection)
    }

    pub fn record_found(&self, found: &FoundArtifact) -> Result<bool, ArtifactError> {
        let Some(link) = artifact_link(&found.link) else {
            return Ok(false);
        };
        let title = found.title.as_deref().and_then(title_from_page);
        let connection = self.lock()?;
        if let Some(id) = id_of(&connection, &link)? {
            let Some(title) = title else {
                return Ok(false);
            };
            return adopt_title(&connection, id, &title);
        }
        let dismissed = connection
            .query_row("SELECT 1 FROM dismissed_artifacts WHERE link = ?1", params![link], |_| Ok(()))
            .optional()
            .map_err(storage)?
            .is_some();
        if dismissed {
            return Ok(false);
        }
        let title = title.unwrap_or_else(|| default_title(&link));
        connection
            .execute(
                "INSERT INTO design_artifacts (title, link, custom_title, origin, added_at) VALUES (?1, ?2, 0, ?3, ?4)",
                params![title, link, ArtifactOrigin::Session.code(), found.created_at.unwrap_or_else(unix_now)],
            )
            .map_err(storage)?;
        Ok(true)
    }

    pub fn adopt_page_title(&self, id: i64, title: &str) -> Result<bool, ArtifactError> {
        adopt_title(&*self.lock()?, id, title)
    }

    pub fn transcript_offset(&self, path: &str) -> Result<u64, ArtifactError> {
        let offset: Option<i64> = self
            .lock()?
            .query_row("SELECT offset FROM transcript_offsets WHERE path = ?1", params![path], |row| row.get(0))
            .optional()
            .map_err(storage)?;
        Ok(offset.and_then(|value| u64::try_from(value).ok()).unwrap_or(0))
    }

    pub fn set_transcript_offset(&self, path: &str, offset: u64) -> Result<(), ArtifactError> {
        let offset = i64::try_from(offset).map_err(|error| ArtifactError::Storage(error.to_string()))?;
        self.lock()?
            .execute(
                "INSERT INTO transcript_offsets (path, offset) VALUES (?1, ?2)
                 ON CONFLICT(path) DO UPDATE SET offset = excluded.offset",
                params![path, offset],
            )
            .map_err(storage)?;
        Ok(())
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, ArtifactError> {
        self.connection
            .lock()
            .map_err(|_| ArtifactError::Storage("artifact shelf lock is poisoned".into()))
    }
}

pub fn open_window(
    app: &AppHandle,
    store: Arc<ArtifactStore>,
    artifact: &DesignArtifact,
) -> Result<(), ArtifactError> {
    let label = format!("{WINDOW_PREFIX}{}", artifact.id);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    let address = Url::parse(&artifact.link).map_err(|error| ArtifactError::Window(error.to_string()))?;
    let id = artifact.id;
    let link = artifact.link.clone();
    let notifier = app.clone();
    WebviewWindowBuilder::new(app, label, WebviewUrl::External(address))
        .title(window_title(&artifact.title))
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .min_inner_size(WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT)
        .center()
        .on_document_title_changed(move |window, page_title| {
            let on_artifact = window
                .url()
                .ok()
                .and_then(|current| artifact_link(current.as_str()))
                .is_some_and(|current| current == link);
            let Some(title) = title_from_page(&page_title).filter(|_| on_artifact) else {
                return;
            };
            let _ = window.set_title(&window_title(&title));
            let store = Arc::clone(&store);
            let app = notifier.clone();
            thread::spawn(move || {
                if store.adopt_page_title(id, &title).unwrap_or(false) {
                    let _ = app.emit(ARTIFACTS_CHANGED_EVENT, ());
                }
            });
        })
        .build()
        .map(drop)
        .map_err(|error| ArtifactError::Window(error.to_string()))
}

fn window_title(title: &str) -> String {
    format!("{title} · {WINDOW_TITLE_SUFFIX}")
}

fn default_title(link: &str) -> String {
    format!("{DEFAULT_TITLE_PREFIX} {}", artifact_code(link))
}

fn has_origin_column(connection: &Connection) -> Result<bool, ArtifactError> {
    let mut statement = connection
        .prepare("SELECT name FROM pragma_table_info('design_artifacts')")
        .map_err(storage)?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(storage)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage)?;
    Ok(names.iter().any(|name| name == ORIGIN_COLUMN))
}

fn id_of(connection: &Connection, link: &str) -> Result<Option<i64>, ArtifactError> {
    connection
        .query_row("SELECT id FROM design_artifacts WHERE link = ?1", params![link], |row| row.get(0))
        .optional()
        .map_err(storage)
}

fn adopt_title(connection: &Connection, id: i64, title: &str) -> Result<bool, ArtifactError> {
    let changed = connection
        .execute(
            "UPDATE design_artifacts SET title = ?1 WHERE id = ?2 AND custom_title = 0 AND title <> ?1",
            params![title, id],
        )
        .map_err(storage)?;
    Ok(changed > 0)
}

fn list(connection: &Connection) -> Result<Vec<DesignArtifact>, ArtifactError> {
    let mut statement = connection
        .prepare(&format!("{SELECT_ARTIFACT} ORDER BY added_at DESC, id DESC"))
        .map_err(storage)?;
    statement
        .query_map([], read_artifact)
        .map_err(storage)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage)
}

fn read_artifact(row: &Row<'_>) -> rusqlite::Result<DesignArtifact> {
    let origin: String = row.get(4)?;
    Ok(DesignArtifact {
        id: row.get(0)?,
        title: row.get(1)?,
        link: row.get(2)?,
        custom_title: row.get(3)?,
        origin: ArtifactOrigin::from_code(&origin).unwrap_or(ArtifactOrigin::Manual),
        added_at: row.get(5)?,
    })
}

fn storage(error: rusqlite::Error) -> ArtifactError {
    ArtifactError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shelf() -> ArtifactStore {
        ArtifactStore::prepare(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn draft(title: &str, link: &str) -> ArtifactDraft {
        ArtifactDraft {
            title: title.into(),
            link: link.into(),
        }
    }

    fn found(link: &str, title: Option<&str>) -> FoundArtifact {
        FoundArtifact {
            link: link.into(),
            title: title.map(str::to_owned),
            created_at: Some(1_793_396_605),
        }
    }

    #[test]
    fn keeps_artifacts_newest_first_without_duplicates() {
        let shelf = shelf();
        shelf.add(&draft("Hopla", "claude.ai/artifact/UxuweBPZSnhor1MAJw3vte")).unwrap();
        let listed = shelf.add(&draft("", "https://claude.ai/artifact/GwYHvofqunFu6ZSHy3aJPY")).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].title, "Артефакт GwYHvofqunFu6ZSHy3aJPY");
        assert!(!listed[0].custom_title);
        assert_eq!(listed[0].origin, ArtifactOrigin::Manual);
        assert!(listed[1].custom_title);
        assert_eq!(
            shelf.add(&draft("Again", "https://claude.ai/artifact/UxuweBPZSnhor1MAJw3vte?x=1")),
            Err(ArtifactError::Duplicate)
        );
        assert_eq!(shelf.add(&draft("Bad", "https://figma.com/file/1")), Err(ArtifactError::InvalidLink));
    }

    #[test]
    fn adopts_page_titles_only_for_unnamed_artifacts() {
        let shelf = shelf();
        shelf.add(&draft("Мій дизайн", "claude.ai/artifact/named")).unwrap();
        shelf.add(&draft("", "claude.ai/artifact/unnamed")).unwrap();
        let listed = shelf.list().unwrap();
        let unnamed = listed.iter().find(|artifact| artifact.code() == "unnamed").unwrap().id;
        let named = listed.iter().find(|artifact| artifact.code() == "named").unwrap().id;
        assert!(shelf.adopt_page_title(unnamed, "ВІДПУСТИ — Арт-дизайн").unwrap());
        assert!(!shelf.adopt_page_title(unnamed, "ВІДПУСТИ — Арт-дизайн").unwrap());
        assert!(!shelf.adopt_page_title(named, "Інша назва").unwrap());
        assert_eq!(shelf.get(unnamed).unwrap().title, "ВІДПУСТИ — Арт-дизайн");
        assert_eq!(shelf.get(named).unwrap().title, "Мій дизайн");
    }

    #[test]
    fn records_artifacts_found_in_sessions() {
        let shelf = shelf();
        assert!(shelf.record_found(&found("https://claude.ai/artifact/abc", None)).unwrap());
        assert!(!shelf.record_found(&found("https://claude.ai/artifact/abc", None)).unwrap());
        assert!(shelf.record_found(&found("https://claude.ai/artifact/abc", Some("Babuk — redesign"))).unwrap());
        assert!(!shelf.record_found(&found("https://example.com/artifact/abc", Some("x"))).unwrap());
        let listed = shelf.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "Babuk — redesign");
        assert_eq!(listed[0].origin, ArtifactOrigin::Session);
        assert_eq!(listed[0].added_at, 1_793_396_605);
    }

    #[test]
    fn removed_artifacts_stay_removed_until_added_by_hand() {
        let shelf = shelf();
        shelf.record_found(&found("https://claude.ai/artifact/abc", Some("Hopla"))).unwrap();
        let id = shelf.list().unwrap()[0].id;
        assert!(shelf.remove(id).unwrap().is_empty());
        assert!(!shelf.record_found(&found("https://claude.ai/artifact/abc", Some("Hopla"))).unwrap());
        assert!(shelf.list().unwrap().is_empty());
        let restored = shelf.add(&draft("", "claude.ai/artifact/abc")).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].origin, ArtifactOrigin::Manual);
        let missing = restored[0].id + 1;
        assert_eq!(shelf.remove(missing), Err(ArtifactError::NotFound));
        assert_eq!(shelf.get(missing), Err(ArtifactError::NotFound));
    }

    #[test]
    fn remembers_transcript_offsets() {
        let shelf = shelf();
        assert_eq!(shelf.transcript_offset("a.jsonl").unwrap(), 0);
        shelf.set_transcript_offset("a.jsonl", 4096).unwrap();
        shelf.set_transcript_offset("a.jsonl", 8192).unwrap();
        assert_eq!(shelf.transcript_offset("a.jsonl").unwrap(), 8192);
    }

    #[test]
    fn upgrades_tables_without_an_origin_column() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE design_artifacts (
                    id INTEGER PRIMARY KEY, title TEXT NOT NULL, link TEXT NOT NULL UNIQUE,
                    custom_title INTEGER NOT NULL, added_at INTEGER NOT NULL
                );
                INSERT INTO design_artifacts (title, link, custom_title, added_at)
                VALUES ('Old', 'https://claude.ai/artifact/old', 1, 1);",
            )
            .unwrap();
        let shelf = ArtifactStore::prepare(connection).unwrap();
        let listed = shelf.list().unwrap();
        assert_eq!(listed[0].origin, ArtifactOrigin::Manual);
    }
}
