use std::sync::Arc;

use noctics_shared::{
    BuildTarget, CLAUDE_MODEL, ClaudeUsage, GameEngine, JournalEntry, Project, ProjectDetail,
    Screen, SkillOutput, StudioError, StudioSettings,
};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use url::Url;

use crate::asset_hunt::Hunter;
use crate::builder::Builder;
use crate::commands::{ActiveSession, off_main_thread};
use crate::skills::SkillStore;
use crate::studio::Studio;
use crate::usage;

type Studios<'a> = State<'a, Arc<Studio>>;

const PINTEREST_SEARCH: &str = "https://www.pinterest.com/search/pins/";

fn guard(session: &ActiveSession) -> Result<(), StudioError> {
    if session.is_signed_in() {
        Ok(())
    } else {
        Err(StudioError::SignedOut)
    }
}

async fn on_store<T, F>(studio: &Arc<Studio>, task: F) -> Result<T, StudioError>
where
    T: Send + 'static,
    F: FnOnce(&Studio) -> Result<T, StudioError> + Send + 'static,
{
    let studio = Arc::clone(studio);
    off_main_thread(move || task(&studio), StudioError::Storage).await
}

#[tauri::command]
pub async fn studio_settings(
    studio: Studios<'_>,
    skills: State<'_, Arc<SkillStore>>,
    session: State<'_, ActiveSession>,
) -> Result<StudioSettings, StudioError> {
    guard(&session)?;
    let skills = Arc::clone(&skills);
    let settings = on_store(&studio, move |studio| {
        Ok(StudioSettings {
            account: studio.account(true),
            model: CLAUDE_MODEL.to_owned(),
            skills_folder: skills
                .export_dir()
                .map(|dir| dir.to_string_lossy().into_owned()),
        })
    })
    .await?;
    if settings.account.connected() {
        studio.kick();
    }
    Ok(settings)
}

#[tauri::command]
pub async fn claude_usage(
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Option<ClaudeUsage>, StudioError> {
    guard(&session)?;
    on_store(&studio, |_| Ok(usage::fetch())).await
}

#[tauri::command]
pub async fn claude_login(
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), StudioError> {
    guard(&session)?;
    on_store(&studio, |studio| studio.claude().open_login()).await
}

#[tauri::command]
pub async fn list_projects(
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Vec<Project>, StudioError> {
    guard(&session)?;
    on_store(&studio, |studio| studio.store.list_projects()).await
}

#[tauri::command]
pub async fn project_detail(
    id: i64,
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<ProjectDetail, StudioError> {
    guard(&session)?;
    on_store(&studio, move |studio| studio.store.detail(id)).await
}

#[tauri::command]
pub async fn project_journal(
    id: i64,
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Vec<JournalEntry>, StudioError> {
    guard(&session)?;
    on_store(&studio, move |studio| studio.store.journal(id)).await
}

#[tauri::command]
pub async fn start_project(
    prompt: String,
    engine: GameEngine,
    targets: Vec<BuildTarget>,
    studio: Studios<'_>,
    builder: State<'_, Arc<Builder>>,
    session: State<'_, ActiveSession>,
) -> Result<Project, StudioError> {
    guard(&session)?;
    let created = on_store(&studio, move |studio| {
        studio.require_connection()?;
        studio.store.create_project(&prompt, engine)
    })
    .await?;
    let _ = builder.plan_builds(created.id, &targets);
    studio.notify();
    studio.kick();
    Ok(created)
}

#[tauri::command]
pub async fn set_project_engine(
    id: i64,
    engine: GameEngine,
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Project, StudioError> {
    guard(&session)?;
    let updated = on_store(&studio, move |studio| studio.store.set_engine(id, engine)).await?;
    studio.notify();
    Ok(updated)
}

#[tauri::command]
pub async fn add_screen(
    project_id: i64,
    title: String,
    summary: String,
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Screen, StudioError> {
    guard(&session)?;
    let added = on_store(&studio, move |studio| {
        studio.store.add_screen(project_id, &title, &summary)
    })
    .await?;
    studio.notify();
    studio.kick();
    Ok(added)
}

#[tauri::command]
pub async fn refine_screen(
    id: i64,
    feedback: String,
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Screen, StudioError> {
    guard(&session)?;
    let queued = on_store(&studio, move |studio| {
        studio.store.refine_screen(id, &feedback)
    })
    .await?;
    studio.notify();
    studio.kick();
    Ok(queued)
}

#[tauri::command]
pub async fn regenerate_screen(
    id: i64,
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Screen, StudioError> {
    guard(&session)?;
    let queued = on_store(&studio, move |studio| studio.store.regenerate_screen(id)).await?;
    studio.notify();
    studio.kick();
    Ok(queued)
}

#[tauri::command]
pub async fn approve_screen(
    id: i64,
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Screen, StudioError> {
    guard(&session)?;
    let approved = on_store(&studio, move |studio| studio.store.approve_screen(id)).await?;
    studio.notify();
    Ok(approved)
}

#[tauri::command]
pub async fn form_project(
    id: i64,
    name: String,
    cover_screen_id: i64,
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Project, StudioError> {
    guard(&session)?;
    let formed = on_store(&studio, move |studio| {
        studio.store.form_project(id, &name, cover_screen_id)
    })
    .await?;
    studio.notify();
    Ok(formed)
}

#[tauri::command]
pub async fn rename_project(
    id: i64,
    name: String,
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Project, StudioError> {
    guard(&session)?;
    let renamed = on_store(&studio, move |studio| {
        studio.store.rename_project(id, &name)
    })
    .await?;
    studio.notify();
    Ok(renamed)
}

#[tauri::command]
pub async fn delete_project(
    id: i64,
    studio: Studios<'_>,
    hunter: State<'_, Arc<Hunter>>,
    session: State<'_, ActiveSession>,
) -> Result<(), StudioError> {
    guard(&session)?;
    on_store(&studio, move |studio| studio.store.delete_project(id)).await?;
    let hunter = Arc::clone(&hunter);
    let _ = off_main_thread(move || hunter.forget(id), StudioError::Storage).await;
    studio.notify();
    Ok(())
}

#[tauri::command]
pub async fn sync_project(
    id: i64,
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<ProjectDetail, StudioError> {
    guard(&session)?;
    let detail = on_store(&studio, move |studio| {
        studio.store.retire_agent_outputs(id)?;
        studio.store.ensure_outputs(id, &studio.deliverable_skills()?)?;
        studio.store.retry_failed(id)
    })
    .await?;
    studio.notify();
    studio.kick();
    studio.advance(id);
    Ok(detail)
}

#[tauri::command]
pub async fn pick_references(
    id: i64,
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Project, StudioError> {
    guard(&session)?;
    on_store(&studio, |studio| studio.require_connection()).await?;
    let updated = studio.pick_references(id).await?;
    studio.notify();
    Ok(updated)
}

#[tauri::command]
pub async fn restyle_project(
    id: i64,
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Project, StudioError> {
    guard(&session)?;
    on_store(&studio, |studio| studio.require_connection()).await?;
    let restyled = studio.restyle(id).await?;
    studio.notify();
    studio.kick();
    Ok(restyled)
}

#[tauri::command]
pub async fn open_reference(
    id: i64,
    index: usize,
    app: AppHandle,
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), StudioError> {
    guard(&session)?;
    let query = on_store(&studio, move |studio| studio.store.reference_query(id, index)).await?;
    let link = Url::parse_with_params(PINTEREST_SEARCH, [("q", query)])
        .map_err(|error| StudioError::Process(error.to_string()))?;
    app.opener()
        .open_url(link.as_str(), None::<&str>)
        .map_err(|error| StudioError::Process(format!("could not open the link: {error}")))
}

#[tauri::command]
pub async fn regenerate_output(
    id: i64,
    studio: Studios<'_>,
    session: State<'_, ActiveSession>,
) -> Result<SkillOutput, StudioError> {
    guard(&session)?;
    let queued = on_store(&studio, move |studio| studio.store.regenerate_output(id)).await?;
    studio.notify();
    studio.kick();
    Ok(queued)
}