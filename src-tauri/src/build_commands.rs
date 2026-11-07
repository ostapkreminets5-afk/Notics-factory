use std::path::PathBuf;
use std::sync::Arc;

use noctics_shared::{BuildError, BuildOverview, BuildTarget, GamesFolder};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::builder::{Builder, Folder};
use crate::commands::{ActiveSession, off_main_thread};
use crate::games_root::GamesRoot;

type Builders<'a> = State<'a, Arc<Builder>>;
type Roots<'a> = State<'a, Arc<GamesRoot>>;

const SOURCE_FOLDER: &str = "source";
const PLAYTEST_FOLDER: &str = "playtest";
const MAIN_WINDOW: &str = "main";
const PICKER_TITLE: &str = "Папка, у якій Noctics створює ігри";

fn guard(session: &ActiveSession) -> Result<(), BuildError> {
    if session.is_signed_in() {
        Ok(())
    } else {
        Err(BuildError::SignedOut)
    }
}

async fn on_builder<T, F>(builder: &Arc<Builder>, task: F) -> Result<T, BuildError>
where
    T: Send + 'static,
    F: FnOnce(&Arc<Builder>) -> Result<T, BuildError> + Send + 'static,
{
    let builder = Arc::clone(builder);
    off_main_thread(move || task(&builder), BuildError::Storage).await
}

fn opener_failure(error: tauri_plugin_opener::Error) -> BuildError {
    BuildError::Process(format!("could not open: {error}"))
}

fn reveal(app: &AppHandle, path: PathBuf) -> Result<(), BuildError> {
    app.opener().reveal_item_in_dir(path).map_err(opener_failure)
}

fn open(app: &AppHandle, path: PathBuf) -> Result<(), BuildError> {
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(opener_failure)
}

#[tauri::command]
pub async fn build_overview(
    project_id: i64,
    refresh: bool,
    builder: Builders<'_>,
    session: State<'_, ActiveSession>,
) -> Result<BuildOverview, BuildError> {
    guard(&session)?;
    on_builder(&builder, move |builder| builder.overview(project_id, refresh)).await
}

#[tauri::command]
pub async fn generate_game(
    project_id: i64,
    request: Option<String>,
    builder: Builders<'_>,
    session: State<'_, ActiveSession>,
) -> Result<BuildOverview, BuildError> {
    guard(&session)?;
    on_builder(&builder, move |builder| {
        builder.generate(project_id, request)?;
        builder.overview(project_id, false)
    })
    .await
}

#[tauri::command]
pub async fn stop_generation(
    project_id: i64,
    builder: Builders<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), BuildError> {
    guard(&session)?;
    builder.stop_generation(project_id);
    Ok(())
}

#[tauri::command]
pub async fn link_game_source(
    project_id: i64,
    path: String,
    builder: Builders<'_>,
    session: State<'_, ActiveSession>,
) -> Result<BuildOverview, BuildError> {
    guard(&session)?;
    on_builder(&builder, move |builder| {
        builder.link_source(project_id, &path)?;
        builder.overview(project_id, false)
    })
    .await
}

#[tauri::command]
pub async fn start_builds(
    project_id: i64,
    targets: Vec<BuildTarget>,
    builder: Builders<'_>,
    session: State<'_, ActiveSession>,
) -> Result<BuildOverview, BuildError> {
    guard(&session)?;
    on_builder(&builder, move |builder| {
        builder.queue(project_id, &targets)?;
        builder.overview(project_id, false)
    })
    .await
}

#[tauri::command]
pub async fn cancel_build(
    id: i64,
    builder: Builders<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), BuildError> {
    guard(&session)?;
    on_builder(&builder, move |builder| builder.cancel(id)).await
}

#[tauri::command]
pub async fn fix_build(
    id: i64,
    builder: Builders<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), BuildError> {
    guard(&session)?;
    on_builder(&builder, move |builder| builder.fix(id)).await
}

#[tauri::command]
pub async fn reveal_build(
    id: i64,
    app: AppHandle,
    builder: Builders<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), BuildError> {
    guard(&session)?;
    let path = on_builder(&builder, move |builder| builder.artifact_path(id)).await?;
    reveal(&app, path)
}

#[tauri::command]
pub async fn open_build_log(
    id: i64,
    app: AppHandle,
    builder: Builders<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), BuildError> {
    guard(&session)?;
    let path = on_builder(&builder, move |builder| builder.log_path(id)).await?;
    open(&app, path)
}

#[tauri::command]
pub async fn open_game_folder(
    project_id: i64,
    folder: String,
    app: AppHandle,
    builder: Builders<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), BuildError> {
    guard(&session)?;
    let which = match folder.as_str() {
        SOURCE_FOLDER => Folder::Source,
        PLAYTEST_FOLDER => Folder::Playtest,
        _ => Folder::Builds,
    };
    let path = on_builder(&builder, move |builder| builder.folder(project_id, which)).await?;
    open(&app, path)
}

#[tauri::command]
pub async fn start_playtest(
    project_id: i64,
    builder: Builders<'_>,
    session: State<'_, ActiveSession>,
) -> Result<BuildOverview, BuildError> {
    guard(&session)?;
    on_builder(&builder, move |builder| {
        builder.playtest(project_id)?;
        builder.overview(project_id, false)
    })
    .await
}

#[tauri::command]
pub async fn stop_playtest(
    project_id: i64,
    builder: Builders<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), BuildError> {
    guard(&session)?;
    builder.stop_playtest(project_id);
    Ok(())
}

async fn on_root<T, F>(root: &Arc<GamesRoot>, task: F) -> Result<T, BuildError>
where
    T: Send + 'static,
    F: FnOnce(&GamesRoot) -> Result<T, BuildError> + Send + 'static,
{
    let root = Arc::clone(root);
    off_main_thread(move || task(&root), BuildError::Storage).await
}

#[tauri::command]
pub async fn games_folder(root: Roots<'_>, session: State<'_, ActiveSession>) -> Result<GamesFolder, BuildError> {
    guard(&session)?;
    Ok(root.snapshot())
}

#[tauri::command]
pub async fn pick_games_folder(
    app: AppHandle,
    root: Roots<'_>,
    session: State<'_, ActiveSession>,
) -> Result<GamesFolder, BuildError> {
    guard(&session)?;
    on_root(&root, move |root| {
        let current = root.path();
        let mut dialog = app.dialog().file().set_title(PICKER_TITLE);
        if current.is_dir() {
            dialog = dialog.set_directory(&current);
        }
        if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
            dialog = dialog.set_parent(&window);
        }
        match dialog.blocking_pick_folder() {
            None => Ok(root.snapshot()),
            Some(picked) => {
                let path = picked
                    .into_path()
                    .map_err(|error| BuildError::Process(error.to_string()))?;
                root.choose(&path.to_string_lossy())
            }
        }
    })
    .await
}

#[tauri::command]
pub async fn reset_games_folder(root: Roots<'_>, session: State<'_, ActiveSession>) -> Result<GamesFolder, BuildError> {
    guard(&session)?;
    on_root(&root, GamesRoot::reset).await
}

#[tauri::command]
pub async fn open_games_folder(
    app: AppHandle,
    root: Roots<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), BuildError> {
    guard(&session)?;
    let path = on_root(&root, GamesRoot::ensure).await?;
    open(&app, path)
}
