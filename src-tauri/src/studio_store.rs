use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use noctics_shared::{
    Artwork, BUILD_STAGE, DESIGN_STAGE, GameEngine, JOURNAL_LIMIT, JournalEntry, JournalEvent, MAX_PROJECT_NAME_CHARS,
    MAX_SCREEN_TITLE_CHARS, OutputStatus, Pin, Project, ProjectDetail, ProjectStatus,
    PromptAnalysis, Reference, Screen, ScreenStatus, SkillOutput, StudioError, StyleGuide,
    validate_feedback,
    validate_game_prompt, validate_label, validate_screen_summary,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::accounts::unix_now;
use crate::prompts::Plan;

const SCHEMA: &str = "
    PRAGMA foreign_keys = ON;
    CREATE TABLE IF NOT EXISTS projects (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        prompt TEXT NOT NULL,
        pitch TEXT NOT NULL DEFAULT '',
        design_document TEXT NOT NULL DEFAULT '',
        style TEXT NOT NULL DEFAULT '{}',
        status TEXT NOT NULL,
        cover_screen_id INTEGER,
        error TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS screens (
        id INTEGER PRIMARY KEY,
        project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        position INTEGER NOT NULL,
        title TEXT NOT NULL,
        summary TEXT NOT NULL,
        status TEXT NOT NULL,
        html TEXT,
        feedback TEXT,
        revision INTEGER NOT NULL DEFAULT 0,
        error TEXT,
        updated_at INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS screens_by_project ON screens(project_id, position);
    CREATE TABLE IF NOT EXISTS skill_outputs (
        id INTEGER PRIMARY KEY,
        project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        skill_name TEXT NOT NULL,
        title TEXT NOT NULL,
        stage INTEGER NOT NULL,
        status TEXT NOT NULL,
        content TEXT,
        error TEXT,
        updated_at INTEGER NOT NULL,
        UNIQUE(project_id, skill_name)
    );
    CREATE TABLE IF NOT EXISTS journal (
        id INTEGER PRIMARY KEY,
        project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        event TEXT NOT NULL,
        created_at INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS journal_by_project ON journal(project_id, id);
";
const MOODBOARD_COLUMN: &str = "moodboard";
const ADD_MOODBOARD: &str = "ALTER TABLE projects ADD COLUMN moodboard TEXT NOT NULL DEFAULT '[]'";
const ENGINE_COLUMN: &str = "engine";
const ADD_ENGINE: &str = "ALTER TABLE projects ADD COLUMN engine TEXT NOT NULL DEFAULT 'godot'";
const ANALYSIS_COLUMN: &str = "prompt_analysis";
const ADD_ANALYSIS: &str =
    "ALTER TABLE projects ADD COLUMN prompt_analysis TEXT NOT NULL DEFAULT '{}'";
const RECOVER: &str = "
    UPDATE screens SET status = 'queued' WHERE status = 'generating';
    UPDATE skill_outputs SET status = 'queued' WHERE status = 'generating';
    UPDATE projects SET status = 'queued' WHERE status = 'planning';
";
const SELECT_PROJECT: &str = "
    SELECT p.id, p.name, p.prompt, p.pitch, p.status, p.style, p.cover_screen_id,
        (SELECT s.html FROM screens s WHERE s.id = p.cover_screen_id),
        (SELECT COUNT(*) FROM screens s WHERE s.project_id = p.id),
        (SELECT COUNT(*) FROM screens s WHERE s.project_id = p.id AND s.status = 'approved'),
        (SELECT COUNT(*) FROM screens s WHERE s.project_id = p.id AND s.status IN ('queued', 'generating'))
            + (SELECT COUNT(*) FROM skill_outputs o WHERE o.project_id = p.id AND o.status IN ('queued', 'generating')),
        p.error, p.created_at, p.moodboard, p.engine, p.prompt_analysis
    FROM projects p";
const SELECT_SCREEN: &str =
    "SELECT id, project_id, position, title, summary, status, html, revision, error FROM screens";
const SELECT_OUTPUT: &str =
    "SELECT id, project_id, skill_name, title, stage, status, content, error FROM skill_outputs";
const PLACEHOLDER_NAME: &str = "Нова гра";

pub enum Job {
    Plan {
        project_id: i64,
        prompt: String,
        engine: GameEngine,
    },
    Output(Box<OutputJob>),
    Screen(Box<ScreenJob>),
}

pub struct SkillRef {
    pub name: String,
    pub title: String,
    pub stage: u8,
}

pub struct OutputJob {
    pub output_id: i64,
    pub project_id: i64,
    pub skill_name: String,
    pub title: String,
    pub prompt: String,
    pub engine: GameEngine,
    pub game_name: String,
    pub pitch: String,
    pub brief: String,
    pub style: StyleGuide,
    pub screen_titles: Vec<String>,
    pub previous: Vec<(String, String)>,
    pub references: Vec<Reference>,
}

pub struct ReferenceContext {
    pub prompt: String,
    pub game_name: String,
    pub pitch: String,
    pub design_document: String,
    pub style: StyleGuide,
    pub references: Vec<Reference>,
}

pub struct ScreenJob {
    pub screen_id: i64,
    pub project_id: i64,
    pub game_name: String,
    pub pitch: String,
    pub design_document: String,
    pub style: StyleGuide,
    pub sibling_titles: Vec<String>,
    pub title: String,
    pub summary: String,
    pub board: Vec<Reference>,
    pub reference: Option<(String, String)>,
    pub revision: Option<(String, String)>,
}

pub struct StudioStore {
    connection: Mutex<Connection>,
}

impl StudioStore {
    pub fn open(path: &Path) -> Result<Self, StudioError> {
        let store = Self::prepare(Connection::open(path).map_err(storage)?)?;
        store.lock()?.execute_batch(RECOVER).map_err(storage)?;
        Ok(store)
    }

    fn prepare(connection: Connection) -> Result<Self, StudioError> {
        connection.execute_batch(SCHEMA).map_err(storage)?;
        if !has_column(&connection, "projects", MOODBOARD_COLUMN)? {
            connection.execute_batch(ADD_MOODBOARD).map_err(storage)?;
        }
        if !has_column(&connection, "projects", ENGINE_COLUMN)? {
            connection.execute_batch(ADD_ENGINE).map_err(storage)?;
        }
        if !has_column(&connection, "projects", ANALYSIS_COLUMN)? {
            connection.execute_batch(ADD_ANALYSIS).map_err(storage)?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, StudioError> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(&format!("{SELECT_PROJECT} ORDER BY p.id DESC"))
            .map_err(storage)?;
        statement
            .query_map([], read_project)
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)
    }

    pub fn detail(&self, project_id: i64) -> Result<ProjectDetail, StudioError> {
        let connection = self.lock()?;
        let project = find_project(&connection, project_id)?;
        let design_document: String = connection
            .query_row(
                "SELECT design_document FROM projects WHERE id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .map_err(storage)?;
        let mut statement = connection
            .prepare(&format!(
                "{SELECT_SCREEN} WHERE project_id = ?1 ORDER BY position, id"
            ))
            .map_err(storage)?;
        let screens = statement
            .query_map(params![project_id], read_screen)
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?;
        let mut outputs_statement = connection
            .prepare(&format!(
                "{SELECT_OUTPUT} WHERE project_id = ?1 ORDER BY stage, id"
            ))
            .map_err(storage)?;
        let outputs = outputs_statement
            .query_map(params![project_id], read_output)
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?;
        Ok(ProjectDetail {
            project,
            design_document,
            screens,
            outputs,
        })
    }

    pub fn journal(&self, project_id: i64) -> Result<Vec<JournalEntry>, StudioError> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(
                "SELECT id, project_id, event, created_at FROM journal
                 WHERE project_id = ?1 ORDER BY id DESC LIMIT ?2",
            )
            .map_err(storage)?;
        let rows = statement
            .query_map(params![project_id, JOURNAL_LIMIT], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, project_id, event, created_at)| {
                let event = serde_json::from_str(&event).ok()?;
                Some(JournalEntry {
                    id,
                    project_id,
                    event,
                    created_at,
                })
            })
            .collect())
    }

    pub fn note(&self, project_id: i64, event: &JournalEvent) -> Result<(), StudioError> {
        let connection = self.lock()?;
        record(&connection, project_id, event)
    }

    pub fn create_project(&self, prompt: &str, engine: GameEngine) -> Result<Project, StudioError> {
        let prompt = validate_game_prompt(prompt)?;
        let connection = self.lock()?;
        let now = unix_now();
        connection
            .execute(
                "INSERT INTO projects (name, prompt, engine, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![PLACEHOLDER_NAME, prompt, engine.code(), ProjectStatus::Queued.code(), now],
            )
            .map_err(storage)?;
        let project_id = connection.last_insert_rowid();
        record(&connection, project_id, &JournalEvent::ProjectStarted { engine })?;
        find_project(&connection, project_id)
    }

    pub fn set_engine(&self, project_id: i64, engine: GameEngine) -> Result<Project, StudioError> {
        let connection = self.lock()?;
        let project = find_project(&connection, project_id)?;
        if project.engine == engine {
            return Ok(project);
        }
        connection
            .execute(
                "UPDATE projects SET engine = ?1, updated_at = ?2 WHERE id = ?3",
                params![engine.code(), unix_now(), project_id],
            )
            .map_err(storage)?;
        record(&connection, project_id, &JournalEvent::EngineChanged { engine })?;
        find_project(&connection, project_id)
    }

    pub fn ready_for_code(&self, project_id: i64) -> Result<bool, StudioError> {
        self.lock()?
            .query_row(
                "SELECT p.status IN ('designing', 'ready')
                    AND EXISTS(SELECT 1 FROM screens s WHERE s.project_id = p.id)
                    AND NOT EXISTS(SELECT 1 FROM screens s WHERE s.project_id = p.id AND s.status NOT IN ('draft', 'approved'))
                    AND NOT EXISTS(SELECT 1 FROM skill_outputs o WHERE o.project_id = p.id AND o.status != 'done')
                 FROM projects p WHERE p.id = ?1",
                params![project_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)
            .map(|ready| ready.unwrap_or(false))
    }

    pub fn has_pending(&self) -> Result<bool, StudioError> {
        self.lock()?
            .query_row(
                &format!(
                    "SELECT EXISTS(SELECT 1 FROM projects WHERE status = 'queued')
                        OR EXISTS(SELECT 1 FROM skill_outputs o JOIN projects p ON p.id = o.project_id WHERE {})
                        OR EXISTS(SELECT 1 FROM screens s JOIN projects p ON p.id = s.project_id WHERE {})",
                    output_ready(),
                    screen_ready()
                ),
                [],
                |row| row.get(0),
            )
            .map_err(storage)
    }

    pub fn claim_job(&self) -> Result<Option<Job>, StudioError> {
        let connection = self.lock()?;
        let now = unix_now();
        let plan = connection
            .query_row(
                "UPDATE projects SET status = 'planning', error = NULL, updated_at = ?1
                 WHERE id = (SELECT id FROM projects WHERE status = 'queued' ORDER BY id LIMIT 1)
                 RETURNING id, prompt, engine",
                params![now],
                |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, String>(2)?)),
            )
            .optional()
            .map_err(storage)?;
        if let Some((project_id, prompt, engine)) = plan {
            return Ok(Some(Job::Plan {
                project_id,
                prompt,
                engine: parse_engine(&engine),
            }));
        }
        if let Some(job) = claim_output(&connection, now)? {
            return Ok(Some(Job::Output(Box::new(job))));
        }
        let claimed = connection
            .query_row(
                &format!(
                    "UPDATE screens SET status = 'generating', error = NULL, updated_at = ?1
                     WHERE id = (
                         SELECT s.id FROM screens s JOIN projects p ON p.id = s.project_id
                         WHERE {}
                         ORDER BY s.project_id, s.position, s.id LIMIT 1
                     )
                     RETURNING id, project_id, title, summary, html, feedback",
                    screen_ready()
                ),
                params![now],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(storage)?;
        let Some((screen_id, project_id, title, summary, html, feedback)) = claimed else {
            return Ok(None);
        };
        let revision = html.zip(feedback);
        record(
            &connection,
            project_id,
            &JournalEvent::ScreenStarted {
                title: title.clone(),
                revision: revision.is_some(),
            },
        )?;
        let (game_name, pitch, brief, style_json) = project_context(&connection, project_id)?;
        let design_document = design_document_of(&connection, project_id, brief)?;
        let sibling_titles = screen_titles(&connection, project_id)?;
        let reference = connection
            .query_row(
                "SELECT title, html FROM screens
                 WHERE project_id = ?1 AND id != ?2 AND html IS NOT NULL AND status IN ('draft', 'approved')
                 ORDER BY status = 'approved' DESC, position, id LIMIT 1",
                params![project_id, screen_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        let board = find_project(&connection, project_id)?.references;
        Ok(Some(Job::Screen(Box::new(ScreenJob {
            screen_id,
            project_id,
            game_name,
            pitch,
            design_document,
            style: parse_style(&style_json),
            sibling_titles,
            title,
            summary,
            board,
            reference,
            revision,
        }))))
    }

    pub fn apply_plan(
        &self,
        project_id: i64,
        plan: &Plan,
        skills: &[SkillRef],
    ) -> Result<(), StudioError> {
        let connection = self.lock()?;
        let transaction = connection.unchecked_transaction().map_err(storage)?;
        let now = unix_now();
        let style = encode_json(&plan.style)?;
        let moodboard = encode_json(&plan.references)?;
        let analysis = encode_json(&plan.analysis)?;
        let changed = transaction
            .execute(
                "UPDATE projects SET name = ?1, pitch = ?2, design_document = ?3, style = ?4, moodboard = ?5, prompt_analysis = ?6, status = 'designing', error = NULL, updated_at = ?7
                 WHERE id = ?8 AND status = 'planning'",
                params![plan.name, plan.pitch, plan.design_document, style, moodboard, analysis, now, project_id],
            )
            .map_err(storage)?;
        if changed == 0 {
            return Ok(());
        }
        for (position, screen) in (1_i64..).zip(&plan.screens) {
            transaction
                .execute(
                    "INSERT INTO screens (project_id, position, title, summary, status, updated_at) VALUES (?1, ?2, ?3, ?4, 'queued', ?5)",
                    params![project_id, position, screen.title, screen.summary, now],
                )
                .map_err(storage)?;
        }
        let queued_skills = insert_outputs(&transaction, project_id, skills, now)?;
        record(
            &transaction,
            project_id,
            &JournalEvent::PlanReady {
                name: plan.name.clone(),
                screens: count(plan.screens.len()),
                skills: queued_skills,
            },
        )?;
        if !plan.references.is_empty() {
            record(
                &transaction,
                project_id,
                &JournalEvent::BoardsPicked {
                    count: count(plan.references.len()),
                },
            )?;
        }
        transaction.commit().map_err(storage)
    }

    pub fn reference_context(&self, project_id: i64) -> Result<ReferenceContext, StudioError> {
        let connection = self.lock()?;
        let project = find_project(&connection, project_id)?;
        if project.status.is_planning() {
            return Err(StudioError::StillPlanning);
        }
        let (game_name, pitch, brief, style_json) = project_context(&connection, project_id)?;
        Ok(ReferenceContext {
            prompt: project.prompt,
            game_name,
            pitch,
            design_document: design_document_of(&connection, project_id, brief)?,
            style: parse_style(&style_json),
            references: project.references,
        })
    }

    pub fn set_reference_board(
        &self,
        project_id: i64,
        analysis: &PromptAnalysis,
        references: &[Reference],
    ) -> Result<Project, StudioError> {
        let connection = self.lock()?;
        let changed = connection
            .execute(
                "UPDATE projects SET moodboard = ?1, prompt_analysis = ?2, updated_at = ?3 WHERE id = ?4",
                params![encode_json(references)?, encode_json(analysis)?, unix_now(), project_id],
            )
            .map_err(storage)?;
        if changed == 0 {
            return Err(StudioError::NotFound);
        }
        record(
            &connection,
            project_id,
            &JournalEvent::BoardsPicked {
                count: count(references.len()),
            },
        )?;
        find_project(&connection, project_id)
    }

    pub fn reference_query(&self, project_id: i64, index: usize) -> Result<String, StudioError> {
        let connection = self.lock()?;
        find_project(&connection, project_id)?
            .references
            .into_iter()
            .nth(index)
            .map(|reference| reference.query)
            .ok_or(StudioError::NotFound)
    }

    pub fn reference_board(&self, project_id: i64) -> Result<Vec<Reference>, StudioError> {
        let connection = self.lock()?;
        let project = find_project(&connection, project_id)?;
        if project.status.is_planning() {
            return Err(StudioError::StillPlanning);
        }
        Ok(project.references)
    }

    pub fn set_reference_pins(
        &self,
        project_id: i64,
        index: usize,
        query: &str,
        pins: &[Pin],
    ) -> Result<bool, StudioError> {
        self.update_reference(project_id, index, query, |reference| {
            reference.pins = pins.to_vec();
            (!pins.is_empty()).then(|| JournalEvent::PinsImported {
                board: reference.title.clone(),
                count: count(pins.len()),
            })
        })
    }

    pub fn set_reference_artworks(
        &self,
        project_id: i64,
        index: usize,
        query: &str,
        artworks: &[Artwork],
    ) -> Result<bool, StudioError> {
        self.update_reference(project_id, index, query, |reference| {
            reference.artworks = artworks.to_vec();
            (!artworks.is_empty()).then(|| JournalEvent::ArtworksImported {
                board: reference.title.clone(),
                count: count(artworks.len()),
            })
        })
    }

    fn update_reference(
        &self,
        project_id: i64,
        index: usize,
        query: &str,
        change: impl FnOnce(&mut Reference) -> Option<JournalEvent>,
    ) -> Result<bool, StudioError> {
        let connection = self.lock()?;
        let mut references = find_project(&connection, project_id)?.references;
        let Some(reference) = references
            .get_mut(index)
            .filter(|reference| reference.query == query)
        else {
            return Ok(false);
        };
        let event = change(reference);
        connection
            .execute(
                "UPDATE projects SET moodboard = ?1 WHERE id = ?2",
                params![encode_json(&references)?, project_id],
            )
            .map_err(storage)?;
        if let Some(event) = event {
            record(&connection, project_id, &event)?;
        }
        Ok(true)
    }

    pub fn ensure_outputs(&self, project_id: i64, skills: &[SkillRef]) -> Result<(), StudioError> {
        let connection = self.lock()?;
        let project = find_project(&connection, project_id)?;
        if matches!(project.status, ProjectStatus::Designing | ProjectStatus::Ready) {
            let added = insert_outputs(&connection, project_id, skills, unix_now())?;
            if added > 0 {
                record(&connection, project_id, &JournalEvent::SkillsSynced { count: added })?;
            }
        }
        Ok(())
    }

    pub fn finish_output(&self, output_id: i64, content: &str) -> Result<(), StudioError> {
        let connection = self.lock()?;
        let finished = connection
            .query_row(
                "UPDATE skill_outputs SET status = 'done', content = ?1, error = NULL, updated_at = ?2
                 WHERE id = ?3 AND status = 'generating'
                 RETURNING project_id, title, stage",
                params![content, unix_now(), output_id],
                read_skill_ref,
            )
            .optional()
            .map_err(storage)?;
        let Some((project_id, title, stage)) = finished else {
            return Ok(());
        };
        record(&connection, project_id, &JournalEvent::SkillDone { title, stage })
    }

    pub fn fail_output(&self, output_id: i64, problem: &StudioError) -> Result<(), StudioError> {
        let connection = self.lock()?;
        let failed = connection
            .query_row(
                "UPDATE skill_outputs SET status = 'failed', error = ?1, updated_at = ?2
                 WHERE id = ?3 AND status = 'generating'
                 RETURNING project_id, title, stage",
                params![encode_error(problem), unix_now(), output_id],
                read_skill_ref,
            )
            .optional()
            .map_err(storage)?;
        let Some((project_id, title, stage)) = failed else {
            return Ok(());
        };
        record(
            &connection,
            project_id,
            &JournalEvent::SkillFailed {
                title,
                stage,
                error: problem.clone(),
            },
        )
    }

    pub fn regenerate_output(&self, output_id: i64) -> Result<SkillOutput, StudioError> {
        let connection = self.lock()?;
        let output = find_output(&connection, output_id)?;
        if !output.status.is_busy() {
            connection
                .execute(
                    "UPDATE skill_outputs SET status = 'queued', error = NULL, updated_at = ?1 WHERE id = ?2",
                    params![unix_now(), output_id],
                )
                .map_err(storage)?;
            record(
                &connection,
                output.project_id,
                &JournalEvent::SkillRequeued {
                    title: output.title,
                    stage: output.stage,
                },
            )?;
        }
        find_output(&connection, output_id)
    }

    pub fn fail_project(&self, project_id: i64, problem: &StudioError) -> Result<(), StudioError> {
        let connection = self.lock()?;
        let changed = connection
            .execute(
                "UPDATE projects SET status = 'failed', error = ?1, updated_at = ?2 WHERE id = ?3 AND status = 'planning'",
                params![encode_error(problem), unix_now(), project_id],
            )
            .map_err(storage)?;
        if changed == 0 {
            return Ok(());
        }
        record(
            &connection,
            project_id,
            &JournalEvent::PlanFailed {
                error: problem.clone(),
            },
        )
    }

    pub fn finish_screen(&self, screen_id: i64, html: &str) -> Result<(), StudioError> {
        let connection = self.lock()?;
        let finished = connection
            .query_row(
                "UPDATE screens SET status = 'draft', html = ?1, feedback = NULL, error = NULL, revision = revision + 1, updated_at = ?2
                 WHERE id = ?3 AND status = 'generating'
                 RETURNING project_id, title, revision",
                params![html, unix_now(), screen_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(storage)?;
        let Some((project_id, title, version)) = finished else {
            return Ok(());
        };
        record(&connection, project_id, &JournalEvent::ScreenDone { title, version })
    }

    pub fn fail_screen(&self, screen_id: i64, problem: &StudioError) -> Result<(), StudioError> {
        let connection = self.lock()?;
        let failed = connection
            .query_row(
                "UPDATE screens SET status = 'failed', error = ?1, updated_at = ?2
                 WHERE id = ?3 AND status = 'generating'
                 RETURNING project_id, title",
                params![encode_error(problem), unix_now(), screen_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        let Some((project_id, title)) = failed else {
            return Ok(());
        };
        record(
            &connection,
            project_id,
            &JournalEvent::ScreenFailed {
                title,
                error: problem.clone(),
            },
        )
    }

    pub fn add_screen(
        &self,
        project_id: i64,
        title: &str,
        summary: &str,
    ) -> Result<Screen, StudioError> {
        let title = validate_label(title, MAX_SCREEN_TITLE_CHARS)?;
        let summary = validate_screen_summary(summary)?;
        let connection = self.lock()?;
        let project = find_project(&connection, project_id)?;
        if !matches!(
            project.status,
            ProjectStatus::Designing | ProjectStatus::Ready
        ) {
            return Err(StudioError::StillPlanning);
        }
        connection
            .execute(
                "INSERT INTO screens (project_id, position, title, summary, status, updated_at)
                 VALUES (?1, (SELECT COALESCE(MAX(position), 0) + 1 FROM screens WHERE project_id = ?1), ?2, ?3, 'queued', ?4)",
                params![project_id, title, summary, unix_now()],
            )
            .map_err(storage)?;
        let screen_id = connection.last_insert_rowid();
        record(&connection, project_id, &JournalEvent::ScreenAdded { title })?;
        find_screen(&connection, screen_id)
    }

    pub fn refine_screen(&self, screen_id: i64, feedback: &str) -> Result<Screen, StudioError> {
        let feedback = validate_feedback(feedback)?;
        self.requeue(screen_id, Some(&feedback))
    }

    pub fn regenerate_screen(&self, screen_id: i64) -> Result<Screen, StudioError> {
        self.requeue(screen_id, None)
    }

    pub fn restyle(&self, project_id: i64, style: &StyleGuide) -> Result<Project, StudioError> {
        let connection = self.lock()?;
        let project = find_project(&connection, project_id)?;
        if project.status.is_planning() {
            return Err(StudioError::StillPlanning);
        }
        let drawing: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM screens WHERE project_id = ?1 AND status = 'generating')",
                params![project_id],
                |row| row.get(0),
            )
            .map_err(storage)?;
        if drawing {
            return Err(StudioError::ScreenBusy);
        }
        let transaction = connection.unchecked_transaction().map_err(storage)?;
        let now = unix_now();
        transaction
            .execute(
                "UPDATE projects SET style = ?1, updated_at = ?2 WHERE id = ?3",
                params![encode_json(style)?, now, project_id],
            )
            .map_err(storage)?;
        transaction
            .execute(
                "UPDATE screens SET status = 'queued', feedback = NULL, error = NULL, updated_at = ?1 WHERE project_id = ?2",
                params![now, project_id],
            )
            .map_err(storage)?;
        transaction.commit().map_err(storage)?;
        find_project(&connection, project_id)
    }

    fn requeue(&self, screen_id: i64, feedback: Option<&str>) -> Result<Screen, StudioError> {
        let connection = self.lock()?;
        let screen = find_screen(&connection, screen_id)?;
        if screen.status.is_busy() {
            return Err(StudioError::ScreenBusy);
        }
        connection
            .execute(
                "UPDATE screens SET status = 'queued', feedback = ?1, error = NULL, updated_at = ?2 WHERE id = ?3",
                params![feedback, unix_now(), screen_id],
            )
            .map_err(storage)?;
        let title = screen.title;
        let event = if feedback.is_some() {
            JournalEvent::ScreenRefined { title }
        } else {
            JournalEvent::ScreenRequeued { title }
        };
        record(&connection, screen.project_id, &event)?;
        find_screen(&connection, screen_id)
    }

    pub fn approve_screen(&self, screen_id: i64) -> Result<Screen, StudioError> {
        let connection = self.lock()?;
        let screen = find_screen(&connection, screen_id)?;
        if screen.status.is_busy() || screen.html.is_none() {
            return Err(StudioError::ScreenBusy);
        }
        if screen.status == ScreenStatus::Approved {
            return Ok(screen);
        }
        connection
            .execute(
                "UPDATE screens SET status = 'approved', error = NULL, updated_at = ?1 WHERE id = ?2",
                params![unix_now(), screen_id],
            )
            .map_err(storage)?;
        record(
            &connection,
            screen.project_id,
            &JournalEvent::ScreenApproved {
                title: screen.title,
            },
        )?;
        find_screen(&connection, screen_id)
    }

    pub fn form_project(
        &self,
        project_id: i64,
        name: &str,
        cover_screen_id: i64,
    ) -> Result<Project, StudioError> {
        let name = validate_label(name, MAX_PROJECT_NAME_CHARS)?;
        let connection = self.lock()?;
        let project = find_project(&connection, project_id)?;
        if project.status.is_planning() {
            return Err(StudioError::StillPlanning);
        }
        let cover = find_screen(&connection, cover_screen_id)?;
        if cover.project_id != project_id || cover.status != ScreenStatus::Approved {
            return Err(StudioError::NotApproved);
        }
        connection
            .execute(
                "UPDATE projects SET name = ?1, cover_screen_id = ?2, status = 'ready', error = NULL, updated_at = ?3 WHERE id = ?4",
                params![name, cover_screen_id, unix_now(), project_id],
            )
            .map_err(storage)?;
        record(&connection, project_id, &JournalEvent::ProjectFormed { name })?;
        find_project(&connection, project_id)
    }

    pub fn rename_project(&self, project_id: i64, name: &str) -> Result<Project, StudioError> {
        let name = validate_label(name, MAX_PROJECT_NAME_CHARS)?;
        let connection = self.lock()?;
        let project = find_project(&connection, project_id)?;
        if project.name == name {
            return Ok(project);
        }
        connection
            .execute(
                "UPDATE projects SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![name, unix_now(), project_id],
            )
            .map_err(storage)?;
        record(&connection, project_id, &JournalEvent::ProjectRenamed { name })?;
        find_project(&connection, project_id)
    }

    pub fn delete_project(&self, project_id: i64) -> Result<(), StudioError> {
        let changed = self
            .lock()?
            .execute("DELETE FROM projects WHERE id = ?1", params![project_id])
            .map_err(storage)?;
        if changed == 0 {
            return Err(StudioError::NotFound);
        }
        Ok(())
    }

    pub fn retire_agent_outputs(&self, project_id: i64) -> Result<usize, StudioError> {
        self.lock()?
            .execute(
                "DELETE FROM skill_outputs WHERE project_id = ?1 AND stage >= ?2 AND status != 'done'",
                params![project_id, BUILD_STAGE],
            )
            .map_err(storage)
    }

    pub fn retry_failed(&self, project_id: i64) -> Result<ProjectDetail, StudioError> {
        {
            let connection = self.lock()?;
            find_project(&connection, project_id)?;
            let now = unix_now();
            let mut requeued = 0;
            for statement in [
                "UPDATE projects SET status = 'queued', error = NULL, updated_at = ?1 WHERE id = ?2 AND status = 'failed'",
                "UPDATE screens SET status = 'queued', error = NULL, updated_at = ?1 WHERE project_id = ?2 AND status = 'failed'",
                "UPDATE skill_outputs SET status = 'queued', error = NULL, updated_at = ?1 WHERE project_id = ?2 AND status = 'failed'",
            ] {
                requeued += connection
                    .execute(statement, params![now, project_id])
                    .map_err(storage)?;
            }
            if requeued > 0 {
                record(
                    &connection,
                    project_id,
                    &JournalEvent::RetryQueued {
                        count: count(requeued),
                    },
                )?;
            }
        }
        self.detail(project_id)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StudioError> {
        self.connection
            .lock()
            .map_err(|_| StudioError::Storage("studio store lock is poisoned".into()))
    }
}

fn find_project(connection: &Connection, project_id: i64) -> Result<Project, StudioError> {
    connection
        .query_row(
            &format!("{SELECT_PROJECT} WHERE p.id = ?1"),
            params![project_id],
            read_project,
        )
        .optional()
        .map_err(storage)?
        .ok_or(StudioError::NotFound)
}

fn find_screen(connection: &Connection, screen_id: i64) -> Result<Screen, StudioError> {
    connection
        .query_row(
            &format!("{SELECT_SCREEN} WHERE id = ?1"),
            params![screen_id],
            read_screen,
        )
        .optional()
        .map_err(storage)?
        .ok_or(StudioError::NotFound)
}

fn read_project(row: &Row<'_>) -> rusqlite::Result<Project> {
    let status: String = row.get(4)?;
    let style: String = row.get(5)?;
    let error: Option<String> = row.get(11)?;
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        prompt: row.get(2)?,
        pitch: row.get(3)?,
        engine: parse_engine(&row.get::<_, String>(14)?),
        status: ProjectStatus::from_code(&status).unwrap_or(ProjectStatus::Failed),
        style: parse_style(&style),
        cover_screen_id: row.get(6)?,
        cover_html: row.get(7)?,
        screen_count: row.get(8)?,
        approved_count: row.get(9)?,
        pending_count: row.get(10)?,
        error: error.as_deref().map(decode_error),
        created_at: row.get(12)?,
        references: parse_references(&row.get::<_, String>(13)?),
        analysis: parse_analysis(&row.get::<_, String>(15)?),
    })
}

fn read_screen(row: &Row<'_>) -> rusqlite::Result<Screen> {
    let status: String = row.get(5)?;
    let error: Option<String> = row.get(8)?;
    Ok(Screen {
        id: row.get(0)?,
        project_id: row.get(1)?,
        position: row.get(2)?,
        title: row.get(3)?,
        summary: row.get(4)?,
        status: ScreenStatus::from_code(&status).unwrap_or(ScreenStatus::Failed),
        html: row.get(6)?,
        revision: row.get(7)?,
        error: error.as_deref().map(decode_error),
    })
}

fn output_ready() -> String {
    "o.status = 'queued' AND p.status IN ('designing', 'ready')
     AND NOT EXISTS (
         SELECT 1 FROM skill_outputs b
         WHERE b.project_id = o.project_id AND b.stage < o.stage AND b.status IN ('queued', 'generating')
     )"
    .to_owned()
}

fn screen_ready() -> String {
    format!(
        "s.status = 'queued' AND p.status IN ('designing', 'ready')
         AND NOT EXISTS (
             SELECT 1 FROM skill_outputs b
             WHERE b.project_id = s.project_id AND b.stage <= {DESIGN_STAGE} AND b.status IN ('queued', 'generating')
         )"
    )
}

fn claim_output(connection: &Connection, now: i64) -> Result<Option<OutputJob>, StudioError> {
    let claimed = connection
        .query_row(
            &format!(
                "UPDATE skill_outputs SET status = 'generating', error = NULL, updated_at = ?1
                 WHERE id = (
                     SELECT o.id FROM skill_outputs o JOIN projects p ON p.id = o.project_id
                     WHERE {}
                     ORDER BY o.project_id, o.stage, o.id LIMIT 1
                 )
                 RETURNING id, project_id, skill_name, title, stage",
                output_ready()
            ),
            params![now],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, u8>(4)?,
                ))
            },
        )
        .optional()
        .map_err(storage)?;
    let Some((output_id, project_id, skill_name, title, stage)) = claimed else {
        return Ok(None);
    };
    record(
        connection,
        project_id,
        &JournalEvent::SkillStarted {
            title: title.clone(),
            stage,
        },
    )?;
    let (prompt, engine): (String, String) = connection
        .query_row(
            "SELECT prompt, engine FROM projects WHERE id = ?1",
            params![project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(storage)?;
    let (game_name, pitch, brief, style_json) = project_context(connection, project_id)?;
    let moodboard: String = connection
        .query_row(
            "SELECT moodboard FROM projects WHERE id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(storage)?;
    Ok(Some(OutputJob {
        output_id,
        project_id,
        skill_name,
        title,
        prompt,
        engine: parse_engine(&engine),
        game_name,
        pitch,
        brief,
        style: parse_style(&style_json),
        screen_titles: screen_titles(connection, project_id)?,
        previous: finished_outputs(connection, project_id, i64::from(stage))?,
        references: parse_references(&moodboard),
    }))
}

fn project_context(
    connection: &Connection,
    project_id: i64,
) -> Result<(String, String, String, String), StudioError> {
    connection
        .query_row(
            "SELECT name, pitch, design_document, style FROM projects WHERE id = ?1",
            params![project_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(storage)
}

fn screen_titles(connection: &Connection, project_id: i64) -> Result<Vec<String>, StudioError> {
    let mut statement = connection
        .prepare("SELECT title FROM screens WHERE project_id = ?1 ORDER BY position, id")
        .map_err(storage)?;
    statement
        .query_map(params![project_id], |row| row.get(0))
        .map_err(storage)?
        .collect::<Result<Vec<String>, _>>()
        .map_err(storage)
}

fn finished_outputs(
    connection: &Connection,
    project_id: i64,
    below_stage: i64,
) -> Result<Vec<(String, String)>, StudioError> {
    let mut statement = connection
        .prepare(
            "SELECT title, content FROM skill_outputs
             WHERE project_id = ?1 AND stage < ?2 AND status = 'done' AND content IS NOT NULL
             ORDER BY stage, id",
        )
        .map_err(storage)?;
    statement
        .query_map(params![project_id, below_stage], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(storage)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage)
}

fn design_document_of(
    connection: &Connection,
    project_id: i64,
    brief: String,
) -> Result<String, StudioError> {
    let design_outputs = finished_outputs(connection, project_id, i64::from(DESIGN_STAGE) + 1)?;
    Ok(if design_outputs.is_empty() {
        brief
    } else {
        join_outputs(&design_outputs)
    })
}

fn has_column(connection: &Connection, table: &str, column: &str) -> Result<bool, StudioError> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(storage)?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage)?;
    Ok(names.iter().any(|name| name == column))
}

fn join_outputs(outputs: &[(String, String)]) -> String {
    outputs
        .iter()
        .map(|(title, content)| format!("## {title}\n\n{content}"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn insert_outputs(
    connection: &Connection,
    project_id: i64,
    skills: &[SkillRef],
    now: i64,
) -> Result<u32, StudioError> {
    let mut inserted = 0;
    for skill in skills {
        inserted += connection
            .execute(
                "INSERT OR IGNORE INTO skill_outputs (project_id, skill_name, title, stage, status, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'queued', ?5)",
                params![project_id, skill.name, skill.title, skill.stage, now],
            )
            .map_err(storage)?;
    }
    Ok(count(inserted))
}

fn record(connection: &Connection, project_id: i64, event: &JournalEvent) -> Result<(), StudioError> {
    connection
        .execute(
            "INSERT INTO journal (project_id, event, created_at) VALUES (?1, ?2, ?3)",
            params![project_id, encode_json(event)?, unix_now()],
        )
        .map(drop)
        .map_err(storage)
}

fn read_skill_ref(row: &Row<'_>) -> rusqlite::Result<(i64, String, u8)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
}

fn count(amount: usize) -> u32 {
    u32::try_from(amount).unwrap_or(u32::MAX)
}

fn find_output(connection: &Connection, output_id: i64) -> Result<SkillOutput, StudioError> {
    connection
        .query_row(
            &format!("{SELECT_OUTPUT} WHERE id = ?1"),
            params![output_id],
            read_output,
        )
        .optional()
        .map_err(storage)?
        .ok_or(StudioError::NotFound)
}

fn read_output(row: &Row<'_>) -> rusqlite::Result<SkillOutput> {
    let status: String = row.get(5)?;
    let error: Option<String> = row.get(7)?;
    Ok(SkillOutput {
        id: row.get(0)?,
        project_id: row.get(1)?,
        skill_name: row.get(2)?,
        title: row.get(3)?,
        stage: row.get(4)?,
        status: OutputStatus::from_code(&status).unwrap_or(OutputStatus::Failed),
        content: row.get(6)?,
        error: error.as_deref().map(decode_error),
    })
}

fn parse_style(json: &str) -> StyleGuide {
    serde_json::from_str(json).unwrap_or_default()
}

fn parse_engine(code: &str) -> GameEngine {
    GameEngine::from_code(code).unwrap_or_default()
}

fn parse_references(json: &str) -> Vec<Reference> {
    serde_json::from_str(json).unwrap_or_default()
}

fn parse_analysis(json: &str) -> PromptAnalysis {
    serde_json::from_str(json).unwrap_or_default()
}

fn encode_json<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, StudioError> {
    serde_json::to_string(value).map_err(|error| StudioError::Storage(error.to_string()))
}

fn encode_error(problem: &StudioError) -> String {
    serde_json::to_string(problem).unwrap_or_else(|_| problem.to_string())
}

fn decode_error(stored: &str) -> StudioError {
    serde_json::from_str(stored).unwrap_or_else(|_| StudioError::Claude(stored.to_owned()))
}

fn storage(error: rusqlite::Error) -> StudioError {
    StudioError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use crate::prompts::PlannedScreen;

    use super::*;

    const PROMPT: &str = "Стелс про злодія в нічному неоновому місті";

    fn in_memory() -> StudioStore {
        StudioStore::prepare(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn plan() -> Plan {
        Plan {
            name: "Нічний злодій".into(),
            pitch: "Стелс у неоновому місті.".into(),
            design_document: "# GDD".into(),
            style: StyleGuide {
                palette: vec!["#0b0e1a".into(); 5],
                fonts: "Unbounded / Manrope".into(),
                corner_radius: 12,
                mood: "Неонова акварель".into(),
                ..StyleGuide::default()
            },
            screens: ["Головне меню", "HUD", "Пауза"]
                .into_iter()
                .map(|title| PlannedScreen {
                    title: title.into(),
                    summary: "Опис".into(),
                })
                .collect(),
            analysis: PromptAnalysis {
                summary: "Нічний стелс у дощовому місті".into(),
                keywords: vec!["неон".into(), "дощ".into()],
                needs: vec![noctics_shared::SearchNeed {
                    topic: noctics_shared::ReferenceTopic::Vfx,
                    what: "Відблиски й іскри від ліхтарів".into(),
                }],
            },
            references: vec![Reference {
                topic: noctics_shared::ReferenceTopic::World,
                title: "Неонові провулки".into(),
                note: "Вологий асфальт і вивіски".into(),
                query: "neon alley rain".into(),
                palette: vec!["#0b0e1a".into()],
                pins: Vec::new(),
                artworks: Vec::new(),
            }],
        }
    }

    fn planned(store: &StudioStore) -> i64 {
        let project = store.create_project(PROMPT, GameEngine::Godot).unwrap();
        let Some(Job::Plan {
            project_id,
            prompt,
            engine,
        }) = store.claim_job().unwrap()
        else {
            panic!("expected a planning job");
        };
        assert_eq!((project_id, prompt.as_str()), (project.id, PROMPT));
        assert_eq!(engine, GameEngine::Godot);
        store.apply_plan(project_id, &plan(), &[]).unwrap();
        project_id
    }

    fn claim_screen(store: &StudioStore) -> ScreenJob {
        match store.claim_job().unwrap() {
            Some(Job::Screen(job)) => *job,
            _ => panic!("expected a screen job"),
        }
    }

    #[test]
    fn restyling_redraws_every_screen_in_the_new_style() {
        let store = in_memory();
        let project_id = planned(&store);
        let first = claim_screen(&store);
        assert_eq!(
            store.restyle(project_id, &StyleGuide::default()).unwrap_err(),
            StudioError::ScreenBusy
        );
        store.finish_screen(first.screen_id, "<!DOCTYPE html><html></html>").unwrap();
        store.approve_screen(first.screen_id).unwrap();
        let fresh = StyleGuide {
            palette: vec!["#1a1c2b".into()],
            palette_notes: vec!["фон — сутінкове небо".into()],
            look: "Паперові ліхтарі".into(),
            ..StyleGuide::default()
        };
        let project = store.restyle(project_id, &fresh).unwrap();
        assert_eq!(project.style, fresh);
        let detail = store.detail(project_id).unwrap();
        assert!(detail.screens.iter().all(|screen| screen.status == ScreenStatus::Queued));
        let redrawn = claim_screen(&store);
        assert_eq!(redrawn.style, fresh);
        assert!(redrawn.reference.is_none());
        assert!(redrawn.revision.is_none());
    }

    fn skill(name: &str, stage: u8) -> SkillRef {
        SkillRef {
            name: name.into(),
            title: name.into(),
            stage,
        }
    }

    fn claim_output_job(store: &StudioStore) -> OutputJob {
        match store.claim_job().unwrap() {
            Some(Job::Output(job)) => *job,
            _ => panic!("expected a skill output job"),
        }
    }

    #[test]
    fn runs_every_active_skill_in_stage_order_before_screens() {
        let store = in_memory();
        let project = store.create_project(PROMPT, GameEngine::Godot).unwrap();
        let Some(Job::Plan { project_id, .. }) = store.claim_job().unwrap() else {
            panic!("expected a planning job");
        };
        let skills = [
            skill("game-scripts", 6),
            skill("idea-to-gdd", 1),
            skill("concept-prompts", 2),
        ];
        store.apply_plan(project_id, &plan(), &skills).unwrap();
        assert_eq!(store.detail(project.id).unwrap().outputs.len(), 3);
        assert_eq!(store.list_projects().unwrap()[0].pending_count, 6);

        let design = claim_output_job(&store);
        assert_eq!(design.skill_name, "idea-to-gdd");
        assert!(design.previous.is_empty());
        assert!(store.claim_job().unwrap().is_none());
        assert!(!store.has_pending().unwrap());
        store.finish_output(design.output_id, "# Повний GDD").unwrap();

        let concepts = claim_output_job(&store);
        assert_eq!(concepts.skill_name, "concept-prompts");
        assert_eq!(concepts.previous, [("idea-to-gdd".to_owned(), "# Повний GDD".to_owned())]);
        assert_eq!(concepts.screen_titles.len(), 3);

        let menu = claim_screen(&store);
        assert_eq!(menu.design_document, "## idea-to-gdd\n\n# Повний GDD");
        store.fail_output(concepts.output_id, &StudioError::UsageLimit).unwrap();

        let scripts = claim_output_job(&store);
        assert_eq!(scripts.skill_name, "game-scripts");
        assert_eq!(scripts.previous.len(), 1);

        store.ensure_outputs(project.id, &[skill("lore-and-naming", 1)]).unwrap();
        let outputs = store.detail(project.id).unwrap().outputs;
        assert_eq!(outputs.len(), 4);
        let failed = outputs.iter().find(|output| output.skill_name == "concept-prompts").unwrap();
        assert_eq!(failed.error, Some(StudioError::UsageLimit));
        assert_eq!(store.regenerate_output(failed.id).unwrap().status, OutputStatus::Queued);
    }

    #[test]
    fn is_ready_for_code_once_every_skill_and_screen_has_a_result() {
        let store = in_memory();
        let project = store.create_project(PROMPT, GameEngine::Godot).unwrap();
        assert!(!store.ready_for_code(project.id).unwrap());
        let Some(Job::Plan { project_id, .. }) = store.claim_job().unwrap() else {
            panic!("expected a planning job");
        };
        store
            .apply_plan(project_id, &plan(), &[skill("idea-to-gdd", 1)])
            .unwrap();
        assert!(!store.ready_for_code(project_id).unwrap());
        let design = claim_output_job(&store);
        assert_eq!(design.project_id, project_id);
        store.finish_output(design.output_id, "# GDD").unwrap();
        let screens = store.detail(project_id).unwrap().screens.len();
        for index in 0..screens {
            assert!(!store.ready_for_code(project_id).unwrap());
            let screen = claim_screen(&store);
            assert_eq!(screen.project_id, project_id);
            if index == 0 {
                store.fail_screen(screen.screen_id, &StudioError::UsageLimit).unwrap();
                assert!(!store.ready_for_code(project_id).unwrap());
                store.retry_failed(project_id).unwrap();
                let retried = claim_screen(&store);
                store.finish_screen(retried.screen_id, "<html>menu</html>").unwrap();
            } else {
                store.finish_screen(screen.screen_id, "<html>screen</html>").unwrap();
            }
        }
        assert!(store.ready_for_code(project_id).unwrap());
        assert!(!store.ready_for_code(project_id + 100).unwrap());
    }

    #[test]
    fn stuck_build_stage_skills_stop_blocking_the_code_agent() {
        let store = in_memory();
        store.create_project(PROMPT, GameEngine::Godot).unwrap();
        let Some(Job::Plan { project_id, .. }) = store.claim_job().unwrap() else {
            panic!("expected a planning job");
        };
        store
            .apply_plan(project_id, &plan(), &[skill("idea-to-gdd", 1), skill("game-scripts", BUILD_STAGE)])
            .unwrap();
        while let Some(job) = store.claim_job().unwrap() {
            match job {
                Job::Output(job) if job.skill_name == "game-scripts" => store
                    .fail_output(job.output_id, &StudioError::Process("Claude Code did not answer in time".into()))
                    .unwrap(),
                Job::Output(job) => store.finish_output(job.output_id, "# GDD").unwrap(),
                Job::Screen(job) => store.finish_screen(job.screen_id, "<html></html>").unwrap(),
                Job::Plan { .. } => panic!("the plan is already applied"),
            }
        }
        assert!(!store.ready_for_code(project_id).unwrap());
        assert_eq!(store.retire_agent_outputs(project_id).unwrap(), 1);
        assert!(store.ready_for_code(project_id).unwrap());
        let detail = store.detail(project_id).unwrap();
        assert_eq!(detail.outputs.len(), 1);
        assert_eq!(detail.outputs[0].skill_name, "idea-to-gdd");
        assert_eq!(store.retire_agent_outputs(project_id).unwrap(), 0);
    }

    #[test]
    fn journals_what_the_core_did_newest_first() {
        let store = in_memory();
        let project = store.create_project(PROMPT, GameEngine::Godot).unwrap();
        let Some(Job::Plan { project_id, .. }) = store.claim_job().unwrap() else {
            panic!("expected a planning job");
        };
        store
            .apply_plan(project_id, &plan(), &[skill("idea-to-gdd", 1)])
            .unwrap();
        let design = claim_output_job(&store);
        store.finish_output(design.output_id, "# GDD").unwrap();

        let menu = claim_screen(&store);
        store.fail_screen(menu.screen_id, &StudioError::UsageLimit).unwrap();
        store.fail_screen(menu.screen_id, &StudioError::UsageLimit).unwrap();
        store.retry_failed(project.id).unwrap();
        let retried = claim_screen(&store);
        store.finish_screen(retried.screen_id, "<html>menu</html>").unwrap();
        store.approve_screen(retried.screen_id).unwrap();
        store.approve_screen(retried.screen_id).unwrap();
        store.rename_project(project.id, "Нічний злодій").unwrap();
        store.set_engine(project.id, GameEngine::Unity).unwrap();

        let menu_title = || "Головне меню".to_owned();
        let events: Vec<JournalEvent> = store
            .journal(project.id)
            .unwrap()
            .into_iter()
            .map(|entry| entry.event)
            .collect();
        assert_eq!(
            events,
            [
                JournalEvent::EngineChanged {
                    engine: GameEngine::Unity
                },
                JournalEvent::ScreenApproved { title: menu_title() },
                JournalEvent::ScreenDone {
                    title: menu_title(),
                    version: 1
                },
                JournalEvent::ScreenStarted {
                    title: menu_title(),
                    revision: false
                },
                JournalEvent::RetryQueued { count: 1 },
                JournalEvent::ScreenFailed {
                    title: menu_title(),
                    error: StudioError::UsageLimit
                },
                JournalEvent::ScreenStarted {
                    title: menu_title(),
                    revision: false
                },
                JournalEvent::SkillDone {
                    title: "idea-to-gdd".into(),
                    stage: 1
                },
                JournalEvent::SkillStarted {
                    title: "idea-to-gdd".into(),
                    stage: 1
                },
                JournalEvent::BoardsPicked { count: 1 },
                JournalEvent::PlanReady {
                    name: "Нічний злодій".into(),
                    screens: 3,
                    skills: 1
                },
                JournalEvent::ProjectStarted {
                    engine: GameEngine::Godot
                },
            ]
        );

        let pin = Pin::checked("42", "https://i.pinimg.com/236x/aa/bb.jpg", "Неон").unwrap();
        store
            .set_reference_pins(project.id, 0, "neon alley rain", std::slice::from_ref(&pin))
            .unwrap();
        assert_eq!(
            store.journal(project.id).unwrap()[0].event,
            JournalEvent::PinsImported {
                board: "Неонові провулки".into(),
                count: 1
            }
        );

        store.delete_project(project.id).unwrap();
        assert!(store.journal(project.id).unwrap().is_empty());
    }

    #[test]
    fn plans_then_queues_screens_in_order() {
        let store = in_memory();
        let project_id = planned(&store);
        let detail = store.detail(project_id).unwrap();
        assert_eq!(detail.project.name, "Нічний злодій");
        assert_eq!(detail.project.status, ProjectStatus::Designing);
        assert_eq!(detail.design_document, "# GDD");
        let titles: Vec<&str> = detail
            .screens
            .iter()
            .map(|screen| screen.title.as_str())
            .collect();
        assert_eq!(titles, ["Головне меню", "HUD", "Пауза"]);
        assert!(
            detail
                .screens
                .iter()
                .all(|screen| screen.status == ScreenStatus::Queued)
        );

        let first = claim_screen(&store);
        assert_eq!(first.title, "Головне меню");
        assert_eq!(first.sibling_titles.len(), 3);
        assert!(first.reference.is_none() && first.revision.is_none());
        store
            .finish_screen(first.screen_id, "<html>menu</html>")
            .unwrap();

        let second = claim_screen(&store);
        assert_eq!(second.title, "HUD");
        assert_eq!(
            second.reference,
            Some(("Головне меню".to_owned(), "<html>menu</html>".to_owned()))
        );
    }

    #[test]
    fn keeps_and_replaces_the_reference_board() {
        let store = in_memory();
        let project_id = planned(&store);
        let project = store.detail(project_id).unwrap().project;
        assert_eq!(project.references[0].title, "Неонові провулки");
        assert_eq!(project.analysis, plan().analysis);
        assert_eq!(store.reference_query(project_id, 0).unwrap(), "neon alley rain");
        assert_eq!(store.reference_query(project_id, 5), Err(StudioError::NotFound));

        let context = store.reference_context(project_id).unwrap();
        assert_eq!(context.prompt, PROMPT);
        assert_eq!(context.game_name, "Нічний злодій");
        assert_eq!(context.design_document, "# GDD");

        let board = store.reference_board(project_id).unwrap();
        assert_eq!(board[0].query, "neon alley rain");
        assert!(board[0].pins.is_empty());
        let pin = Pin::checked("42", "https://i.pinimg.com/236x/aa/bb.jpg", "Неон").unwrap();
        let pins = std::slice::from_ref(&pin);
        assert!(store.set_reference_pins(project_id, 0, "neon alley rain", pins).unwrap());
        assert!(!store.set_reference_pins(project_id, 0, "other board", pins).unwrap());
        assert!(!store.set_reference_pins(project_id, 3, "neon alley rain", pins).unwrap());
        let pinned = store.detail(project_id).unwrap().project;
        assert_eq!(pinned.references[0].pins, pins);

        let artwork = Artwork::checked(
            "2B1Xae",
            "https://cdna.artstation.com/p/assets/images/images/085/683/878/smaller_square/neon.jpg",
            "Neon alley",
            "Artist",
        )
        .unwrap();
        let artworks = std::slice::from_ref(&artwork);
        assert!(store.set_reference_artworks(project_id, 0, "neon alley rain", artworks).unwrap());
        assert!(!store.set_reference_artworks(project_id, 0, "other board", artworks).unwrap());
        let found = store.detail(project_id).unwrap().project;
        assert_eq!(found.references[0].pins, pins);
        assert_eq!(found.references[0].artworks, artworks);
        assert_eq!(
            store.journal(project_id).unwrap()[0].event,
            JournalEvent::ArtworksImported {
                board: "Неонові провулки".into(),
                count: 1
            }
        );

        let replaced = store
            .set_reference_board(project_id, &PromptAnalysis::default(), &[])
            .unwrap();
        assert!(replaced.references.is_empty());
        assert!(replaced.analysis.is_empty());

        let queued = store.create_project(PROMPT, GameEngine::Godot).unwrap();
        assert_eq!(
            store.reference_context(queued.id).err(),
            Some(StudioError::StillPlanning)
        );
        assert_eq!(
            store.reference_board(queued.id).err(),
            Some(StudioError::StillPlanning)
        );
    }

    #[test]
    fn adds_the_moodboard_column_to_an_older_database() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE projects (
                    id INTEGER PRIMARY KEY, name TEXT NOT NULL, prompt TEXT NOT NULL,
                    pitch TEXT NOT NULL DEFAULT '', design_document TEXT NOT NULL DEFAULT '',
                    style TEXT NOT NULL DEFAULT '{}', status TEXT NOT NULL, cover_screen_id INTEGER,
                    error TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
                );
                INSERT INTO projects (name, prompt, status, created_at, updated_at)
                VALUES ('Стара гра', 'промпт старої гри', 'ready', 1, 1);",
            )
            .unwrap();
        let store = StudioStore::prepare(connection).unwrap();
        let projects = store.list_projects().unwrap();
        assert_eq!(projects[0].name, "Стара гра");
        assert!(projects[0].references.is_empty());
        assert!(projects[0].analysis.is_empty());
        assert_eq!(projects[0].engine, GameEngine::Godot);
    }

    #[test]
    fn keeps_the_engine_the_game_is_built_on() {
        let store = in_memory();
        let project = store.create_project(PROMPT, GameEngine::ThreeJs).unwrap();
        assert_eq!(project.engine, GameEngine::ThreeJs);
        let Some(Job::Plan { project_id, engine, .. }) = store.claim_job().unwrap() else {
            panic!("expected a planning job");
        };
        assert_eq!(engine, GameEngine::ThreeJs);
        store
            .apply_plan(project_id, &plan(), &[skill("game-scripts", 6)])
            .unwrap();

        let moved = store.set_engine(project_id, GameEngine::Penumbra).unwrap();
        assert_eq!(moved.engine, GameEngine::Penumbra);
        assert_eq!(claim_output_job(&store).engine, GameEngine::Penumbra);
        assert_eq!(
            store.set_engine(999, GameEngine::Unity),
            Err(StudioError::NotFound)
        );
    }

    #[test]
    fn refines_approves_and_forms_the_project() {
        let store = in_memory();
        let project_id = planned(&store);
        let menu = claim_screen(&store);
        store
            .finish_screen(menu.screen_id, "<html>v1</html>")
            .unwrap();

        assert_eq!(
            store.form_project(project_id, "Нічний злодій", menu.screen_id),
            Err(StudioError::NotApproved)
        );

        store
            .refine_screen(menu.screen_id, "Додай логотип")
            .unwrap();
        let revision = loop {
            let job = claim_screen(&store);
            if job.screen_id == menu.screen_id {
                break job;
            }
            store
                .finish_screen(job.screen_id, "<html>other</html>")
                .unwrap();
        };
        assert_eq!(
            revision.revision,
            Some(("<html>v1</html>".to_owned(), "Додай логотип".to_owned()))
        );
        assert_eq!(
            store.refine_screen(menu.screen_id, "ще"),
            Err(StudioError::ScreenBusy)
        );
        store
            .finish_screen(menu.screen_id, "<html>v2</html>")
            .unwrap();

        let approved = store.approve_screen(menu.screen_id).unwrap();
        assert_eq!(
            (approved.status, approved.revision),
            (ScreenStatus::Approved, 2)
        );

        let formed = store
            .form_project(project_id, "  Тіні   неону ", menu.screen_id)
            .unwrap();
        assert_eq!(formed.status, ProjectStatus::Ready);
        assert_eq!(formed.name, "Тіні неону");
        assert_eq!(formed.cover_html.as_deref(), Some("<html>v2</html>"));
        assert_eq!(
            (
                formed.screen_count,
                formed.approved_count,
                formed.pending_count
            ),
            (3, 1, 2)
        );
    }

    #[test]
    fn failures_are_kept_and_retried() {
        let store = in_memory();
        let project = store.create_project(PROMPT, GameEngine::Godot).unwrap();
        let Some(Job::Plan { project_id, .. }) = store.claim_job().unwrap() else {
            panic!("expected a planning job");
        };
        store
            .fail_project(project_id, &StudioError::RateLimited)
            .unwrap();
        let failed = store.detail(project.id).unwrap().project;
        assert_eq!(failed.status, ProjectStatus::Failed);
        assert_eq!(failed.error, Some(StudioError::RateLimited));
        assert!(!store.has_pending().unwrap());

        store.retry_failed(project.id).unwrap();
        assert!(store.has_pending().unwrap());
        assert!(matches!(store.claim_job().unwrap(), Some(Job::Plan { .. })));
    }

    #[test]
    fn rejects_screens_before_planning_and_bad_input() {
        let store = in_memory();
        let project = store.create_project(PROMPT, GameEngine::Godot).unwrap();
        assert_eq!(
            store.add_screen(project.id, "Магазин", ""),
            Err(StudioError::StillPlanning)
        );
        assert_eq!(
            store.create_project("коротко", GameEngine::Godot),
            Err(StudioError::PromptTooShort)
        );
        assert_eq!(
            store.rename_project(999, "Назва"),
            Err(StudioError::NotFound)
        );
    }

    #[test]
    fn deleting_a_project_removes_its_screens() {
        let store = in_memory();
        let project_id = planned(&store);
        let extra = store.add_screen(project_id, "Магазин", "Апгрейди").unwrap();
        assert_eq!(extra.position, 4);
        store.delete_project(project_id).unwrap();
        assert!(store.list_projects().unwrap().is_empty());
        assert!(!store.has_pending().unwrap());
        assert_eq!(store.detail(project_id).unwrap_err(), StudioError::NotFound);
    }
}
