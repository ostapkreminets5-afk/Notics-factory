use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use noctics_shared::{SKILL_FILE_NAME, Skill, SkillCategory, SkillDraft, SkillError};
use rusqlite::types::Type;
use rusqlite::{Connection, ErrorCode, OptionalExtension, Row, params};

use crate::accounts::unix_now;
use crate::skill_seeds::{LATEST_SEED_VERSION, skills_added_in};

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS skills (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE,
        stage INTEGER NOT NULL,
        title TEXT NOT NULL,
        summary TEXT NOT NULL,
        category TEXT NOT NULL,
        instructions TEXT NOT NULL,
        enabled INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
";
const SELECT_SKILL: &str =
    "SELECT id, name, stage, title, summary, category, instructions, enabled FROM skills";
const CATEGORY_COLUMN: usize = 5;

pub struct SkillStore {
    connection: Mutex<Connection>,
    export_dir: Option<PathBuf>,
}

impl SkillStore {
    pub fn open(database: &Path, export_dir: PathBuf) -> Result<Self, SkillError> {
        let store = Self::prepare(
            Connection::open(database).map_err(storage)?,
            Some(export_dir),
        )?;
        store.export(&*store.lock()?)?;
        Ok(store)
    }

    fn prepare(connection: Connection, export_dir: Option<PathBuf>) -> Result<Self, SkillError> {
        connection.execute_batch(SCHEMA).map_err(storage)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(storage)?;
        if version < LATEST_SEED_VERSION {
            seed(&connection, version)?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
            export_dir,
        })
    }

    pub fn export_dir(&self) -> Option<&Path> {
        self.export_dir.as_deref()
    }

    pub fn list(&self) -> Result<Vec<Skill>, SkillError> {
        list_all(&*self.lock()?)
    }

    pub fn create(&self, draft: &SkillDraft) -> Result<Skill, SkillError> {
        draft.validate()?;
        let draft = draft.normalized();
        let connection = self.lock()?;
        insert(&connection, &draft)?;
        let id = connection.last_insert_rowid();
        self.settle(&connection, id)
    }

    pub fn update(&self, id: i64, draft: &SkillDraft) -> Result<Skill, SkillError> {
        draft.validate()?;
        let draft = draft.normalized();
        let connection = self.lock()?;
        let changed = connection
            .execute(
                "UPDATE skills SET name = ?1, stage = ?2, title = ?3, summary = ?4, category = ?5, instructions = ?6, enabled = ?7, updated_at = ?8 WHERE id = ?9",
                params![
                    draft.name,
                    draft.stage,
                    draft.title,
                    draft.summary,
                    draft.category.code(),
                    draft.instructions,
                    draft.enabled,
                    unix_now(),
                    id
                ],
            )
            .map_err(write_failure)?;
        if changed == 0 {
            return Err(SkillError::NotFound);
        }
        self.settle(&connection, id)
    }

    pub fn set_enabled(&self, id: i64, enabled: bool) -> Result<Skill, SkillError> {
        let connection = self.lock()?;
        let changed = connection
            .execute(
                "UPDATE skills SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![enabled, unix_now(), id],
            )
            .map_err(storage)?;
        if changed == 0 {
            return Err(SkillError::NotFound);
        }
        self.settle(&connection, id)
    }

    pub fn delete(&self, id: i64) -> Result<(), SkillError> {
        let connection = self.lock()?;
        let changed = connection
            .execute("DELETE FROM skills WHERE id = ?1", params![id])
            .map_err(storage)?;
        if changed == 0 {
            return Err(SkillError::NotFound);
        }
        self.export(&connection)
    }

    fn settle(&self, connection: &Connection, id: i64) -> Result<Skill, SkillError> {
        let skill = find(connection, id)?.ok_or(SkillError::NotFound)?;
        self.export(connection)?;
        Ok(skill)
    }

    fn export(&self, connection: &Connection) -> Result<(), SkillError> {
        match &self.export_dir {
            Some(dir) => sync_skill_files(dir, &list_all(connection)?),
            None => Ok(()),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, SkillError> {
        self.connection
            .lock()
            .map_err(|_| SkillError::Storage("skill store lock is poisoned".into()))
    }
}

fn seed(connection: &Connection, from_version: i64) -> Result<(), SkillError> {
    let transaction = connection.unchecked_transaction().map_err(storage)?;
    for version in (from_version + 1)..=LATEST_SEED_VERSION {
        for draft in skills_added_in(version) {
            match insert(&transaction, &draft) {
                Ok(()) | Err(SkillError::NameTaken) => {}
                Err(problem) => return Err(problem),
            }
        }
    }
    transaction
        .pragma_update(None, "user_version", LATEST_SEED_VERSION)
        .map_err(storage)?;
    transaction.commit().map_err(storage)
}

fn insert(connection: &Connection, draft: &SkillDraft) -> Result<(), SkillError> {
    let now = unix_now();
    connection
        .execute(
            "INSERT INTO skills (name, stage, title, summary, category, instructions, enabled, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                draft.name,
                draft.stage,
                draft.title,
                draft.summary,
                draft.category.code(),
                draft.instructions,
                draft.enabled,
                now
            ],
        )
        .map(drop)
        .map_err(write_failure)
}

fn list_all(connection: &Connection) -> Result<Vec<Skill>, SkillError> {
    let mut statement = connection
        .prepare(&format!("{SELECT_SKILL} ORDER BY stage, id"))
        .map_err(storage)?;
    statement
        .query_map([], read_skill)
        .map_err(storage)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage)
}

fn find(connection: &Connection, id: i64) -> Result<Option<Skill>, SkillError> {
    connection
        .query_row(
            &format!("{SELECT_SKILL} WHERE id = ?1"),
            params![id],
            read_skill,
        )
        .optional()
        .map_err(storage)
}

fn read_skill(row: &Row<'_>) -> rusqlite::Result<Skill> {
    let category_code: String = row.get(CATEGORY_COLUMN)?;
    let category = SkillCategory::from_code(&category_code).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            CATEGORY_COLUMN,
            Type::Text,
            Box::new(SkillError::Storage(format!(
                "unknown skill category {category_code}"
            ))),
        )
    })?;
    Ok(Skill {
        id: row.get(0)?,
        name: row.get(1)?,
        stage: row.get(2)?,
        title: row.get(3)?,
        summary: row.get(4)?,
        category,
        instructions: row.get(6)?,
        enabled: row.get(7)?,
    })
}

fn sync_skill_files(dir: &Path, skills: &[Skill]) -> Result<(), SkillError> {
    fs::create_dir_all(dir).map_err(io_failure)?;
    let active: Vec<&Skill> = skills.iter().filter(|skill| skill.enabled).collect();
    let keep: HashSet<&str> = active.iter().map(|skill| skill.name.as_str()).collect();
    for entry in fs::read_dir(dir).map_err(io_failure)? {
        let folder = entry.map_err(io_failure)?.path();
        let skill_file = folder.join(SKILL_FILE_NAME);
        let name = folder
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if skill_file.is_file() && !keep.contains(name) {
            fs::remove_file(&skill_file).map_err(io_failure)?;
            let _ = fs::remove_dir(&folder);
        }
    }
    for skill in active {
        let folder = dir.join(&skill.name);
        fs::create_dir_all(&folder).map_err(io_failure)?;
        fs::write(folder.join(SKILL_FILE_NAME), skill.to_skill_markdown()).map_err(io_failure)?;
    }
    Ok(())
}

fn write_failure(error: rusqlite::Error) -> SkillError {
    match error {
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == ErrorCode::ConstraintViolation =>
        {
            SkillError::NameTaken
        }
        other => storage(other),
    }
}

fn storage(error: rusqlite::Error) -> SkillError {
    SkillError::Storage(error.to_string())
}

fn io_failure(error: std::io::Error) -> SkillError {
    SkillError::Storage(format!("skill files could not be written: {error}"))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use noctics_shared::LAST_STAGE;

    use super::*;

    fn in_memory() -> SkillStore {
        SkillStore::prepare(Connection::open_in_memory().unwrap(), None).unwrap()
    }

    fn scratch(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("noctics-skills-{label}-{nanos}"))
    }

    fn draft(name: &str) -> SkillDraft {
        SkillDraft {
            name: name.into(),
            stage: 5,
            title: " Ритм анімацій ".into(),
            summary: "Темп і пози для циклів руху.".into(),
            category: SkillCategory::Art,
            instructions: "- 12 кадрів на цикл.".into(),
            enabled: true,
        }
    }

    #[test]
    fn starts_with_the_default_skills_in_stage_order() {
        let skills = in_memory().list().unwrap();
        let names: Vec<&str> = skills.iter().map(|skill| skill.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "idea-to-gdd",
                "lore-and-naming",
                "concept-prompts",
                "game-ui-screens",
                "real-art-not-vector",
                "model-spec",
                "material-style",
                "game-scripts",
                "vfx-forge"
            ]
        );
        let disabled: Vec<&str> = skills
            .iter()
            .filter(|skill| !skill.enabled)
            .map(|skill| skill.name.as_str())
            .collect();
        assert_eq!(disabled, ["material-style"]);
        assert!(skills.iter().all(|skill| skill.draft().validate().is_ok()));
    }

    #[test]
    fn creates_updates_toggles_and_deletes() {
        let store = in_memory();
        let created = store.create(&draft("animation-rhythm")).unwrap();
        assert_eq!(created.title, "Ритм анімацій");
        assert_eq!(store.list().unwrap().len(), 10);

        let mut changes = created.draft();
        changes.stage = LAST_STAGE;
        changes.title = "Ритм і пози".into();
        let updated = store.update(created.id, &changes).unwrap();
        assert_eq!(
            (updated.stage, updated.title.as_str()),
            (LAST_STAGE, "Ритм і пози")
        );

        let switched_off = store.set_enabled(created.id, false).unwrap();
        assert!(!switched_off.enabled);

        store.delete(created.id).unwrap();
        assert_eq!(store.delete(created.id), Err(SkillError::NotFound));
        assert_eq!(
            store.set_enabled(created.id, true),
            Err(SkillError::NotFound)
        );
        assert_eq!(store.list().unwrap().len(), 9);
    }

    #[test]
    fn refuses_duplicate_names_and_invalid_drafts() {
        let store = in_memory();
        assert_eq!(
            store.create(&draft("model-spec")),
            Err(SkillError::NameTaken)
        );
        assert_eq!(
            store.create(&draft("Не латиниця")),
            Err(SkillError::NameInvalid)
        );
        assert_eq!(store.list().unwrap().len(), 9);
    }

    #[test]
    fn keeps_deleted_defaults_deleted_after_reopening() {
        let database = scratch("db").with_extension("sqlite3");
        let exports = scratch("db-export");

        let store = SkillStore::open(&database, exports.clone()).unwrap();
        let first = store.list().unwrap()[0].id;
        store.delete(first).unwrap();
        drop(store);
        let reopened = SkillStore::open(&database, exports.clone()).unwrap();
        let count = reopened.list().unwrap().len();
        drop(reopened);
        std::fs::remove_file(&database).unwrap();
        std::fs::remove_dir_all(&exports).unwrap();

        assert_eq!(count, 8);
    }

    #[test]
    fn adds_newer_default_skills_to_an_existing_library() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(SCHEMA).unwrap();
        seed(&connection, 0).unwrap();
        connection
            .execute(
                "DELETE FROM skills WHERE name IN ('game-ui-screens', 'real-art-not-vector', 'vfx-forge')",
                [],
            )
            .unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();

        let store = SkillStore::prepare(connection, None).unwrap();
        let names: Vec<String> = store
            .list()
            .unwrap()
            .into_iter()
            .map(|skill| skill.name)
            .collect();
        assert!(names.iter().any(|name| name == "game-ui-screens"));
        assert!(names.iter().any(|name| name == "real-art-not-vector"));
        assert!(names.iter().any(|name| name == "vfx-forge"));
        assert_eq!(names.len(), 9);
    }

    #[test]
    fn mirrors_enabled_skills_into_skill_folders() {
        let exports = scratch("export");
        let store =
            SkillStore::prepare(Connection::open_in_memory().unwrap(), Some(exports.clone()))
                .unwrap();
        store.export(&store.lock().unwrap()).unwrap();
        let exported = |name: &str| exports.join(name).join(SKILL_FILE_NAME).is_file();

        assert!(exported("idea-to-gdd"));
        assert!(!exported("material-style"));

        let lore = store.list().unwrap()[1].clone();
        store.set_enabled(lore.id, false).unwrap();
        assert!(!exported("lore-and-naming"));
        assert!(!exports.join("lore-and-naming").exists());

        let mut renamed = store.list().unwrap()[0].draft();
        renamed.name = "idea-brief".into();
        let id = store.list().unwrap()[0].id;
        store.update(id, &renamed).unwrap();
        assert!(!exported("idea-to-gdd"));
        let markdown =
            std::fs::read_to_string(exports.join("idea-brief").join(SKILL_FILE_NAME)).unwrap();
        std::fs::remove_dir_all(&exports).unwrap();

        assert!(markdown.starts_with("---\nname: idea-brief\ndescription: \"Розкладає промпт"));
    }
}
