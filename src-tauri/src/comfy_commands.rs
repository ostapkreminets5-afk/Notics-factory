use std::path::PathBuf;
use std::sync::Arc;

use noctics_shared::{AssetDraft, ComfyError, ComfyOverview, TaskOrigin};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::comfy_forge::Forge;
use crate::commands::{ActiveSession, off_main_thread};

type Forges<'a> = State<'a, Arc<Forge>>;

fn guard(session: &ActiveSession) -> Result<(), ComfyError> {
    if session.is_signed_in() {
        Ok(())
    } else {
        Err(ComfyError::SignedOut)
    }
}

async fn on_forge<T, F>(forge: &Arc<Forge>, task: F) -> Result<T, ComfyError>
where
    T: Send + 'static,
    F: FnOnce(&Arc<Forge>) -> Result<T, ComfyError> + Send + 'static,
{
    let forge = Arc::clone(forge);
    off_main_thread(move || task(&forge), ComfyError::Storage).await
}

fn opener_failure(error: tauri_plugin_opener::Error) -> ComfyError {
    ComfyError::Storage(format!("could not open: {error}"))
}

#[tauri::command]
pub async fn comfy_overview(
    forge: Forges<'_>,
    session: State<'_, ActiveSession>,
) -> Result<ComfyOverview, ComfyError> {
    guard(&session)?;
    on_forge(&forge, |forge| forge.overview()).await
}

#[tauri::command]
pub async fn create_asset_tasks(
    draft: AssetDraft,
    forge: Forges<'_>,
    session: State<'_, ActiveSession>,
) -> Result<ComfyOverview, ComfyError> {
    guard(&session)?;
    on_forge(&forge, move |forge| {
        forge.create(&draft, TaskOrigin::Studio, None)?;
        forge.overview()
    })
    .await
}

#[tauri::command]
pub async fn retry_model_run(
    id: i64,
    forge: Forges<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), ComfyError> {
    guard(&session)?;
    on_forge(&forge, move |forge| forge.retry(id)).await
}

#[tauri::command]
pub async fn delete_asset_task(
    id: i64,
    forge: Forges<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), ComfyError> {
    guard(&session)?;
    on_forge(&forge, move |forge| forge.delete(id)).await
}

#[tauri::command]
pub async fn set_model_enabled(
    model_id: String,
    enabled: bool,
    forge: Forges<'_>,
    session: State<'_, ActiveSession>,
) -> Result<ComfyOverview, ComfyError> {
    guard(&session)?;
    on_forge(&forge, move |forge| {
        forge.set_enabled(&model_id, enabled)?;
        forge.overview()
    })
    .await
}

#[tauri::command]
pub async fn launch_comfy(
    forge: Forges<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), ComfyError> {
    guard(&session)?;
    on_forge(&forge, |forge| forge.launch()).await
}

async fn run_file(forge: &Arc<Forge>, run_id: i64, index: usize) -> Result<PathBuf, ComfyError> {
    on_forge(forge, move |forge| forge.file_path(run_id, index)).await
}

#[tauri::command]
pub async fn open_run_file(
    run_id: i64,
    index: usize,
    app: AppHandle,
    forge: Forges<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), ComfyError> {
    guard(&session)?;
    let path = run_file(&forge, run_id, index).await?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(opener_failure)
}

#[tauri::command]
pub async fn reveal_run_file(
    run_id: i64,
    index: usize,
    app: AppHandle,
    forge: Forges<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), ComfyError> {
    guard(&session)?;
    let path = run_file(&forge, run_id, index).await?;
    app.opener().reveal_item_in_dir(path).map_err(opener_failure)
}
