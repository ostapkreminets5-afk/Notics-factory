use std::sync::Arc;

use noctics_shared::{MixamoError, MixamoOverview, MotionClip, MotionDraft, MotionOrder};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::commands::{ActiveSession, off_main_thread};
use crate::mixamo::Mixamo;

const SEARCH_LIMIT: usize = 36;

type Animations<'a> = State<'a, Arc<Mixamo>>;

fn guard(session: &ActiveSession) -> Result<(), MixamoError> {
    if session.is_signed_in() {
        Ok(())
    } else {
        Err(MixamoError::SignedOut)
    }
}

async fn on_mixamo<T, F>(mixamo: &Arc<Mixamo>, task: F) -> Result<T, MixamoError>
where
    T: Send + 'static,
    F: FnOnce(&Arc<Mixamo>) -> Result<T, MixamoError> + Send + 'static,
{
    let mixamo = Arc::clone(mixamo);
    off_main_thread(move || task(&mixamo), MixamoError::Storage).await
}

#[tauri::command]
pub async fn mixamo_overview(mixamo: Animations<'_>, session: State<'_, ActiveSession>) -> Result<MixamoOverview, MixamoError> {
    guard(&session)?;
    on_mixamo(&mixamo, |mixamo| mixamo.overview()).await
}

#[tauri::command]
pub async fn mixamo_login(mixamo: Animations<'_>, session: State<'_, ActiveSession>) -> Result<(), MixamoError> {
    guard(&session)?;
    on_mixamo(&mixamo, |mixamo| mixamo.open_login()).await
}

#[tauri::command]
pub async fn mixamo_search(
    query: String,
    mixamo: Animations<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Vec<MotionClip>, MixamoError> {
    guard(&session)?;
    on_mixamo(&mixamo, move |mixamo| mixamo.search(&query, SEARCH_LIMIT)).await
}

#[tauri::command]
pub async fn order_motion(
    draft: MotionDraft,
    mixamo: Animations<'_>,
    session: State<'_, ActiveSession>,
) -> Result<MotionOrder, MixamoError> {
    guard(&session)?;
    on_mixamo(&mixamo, move |mixamo| mixamo.create(&draft)).await
}

#[tauri::command]
pub async fn plan_motions(
    id: i64,
    mixamo: Animations<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Vec<MotionOrder>, MixamoError> {
    guard(&session)?;
    on_mixamo(&mixamo, move |mixamo| mixamo.plan_project(id)).await
}

#[tauri::command]
pub async fn retry_motion(id: i64, mixamo: Animations<'_>, session: State<'_, ActiveSession>) -> Result<(), MixamoError> {
    guard(&session)?;
    on_mixamo(&mixamo, move |mixamo| mixamo.retry(id)).await
}

#[tauri::command]
pub async fn delete_motion(id: i64, mixamo: Animations<'_>, session: State<'_, ActiveSession>) -> Result<(), MixamoError> {
    guard(&session)?;
    on_mixamo(&mixamo, move |mixamo| mixamo.delete(id)).await
}

#[tauri::command]
pub async fn reveal_motion(
    id: i64,
    app: AppHandle,
    mixamo: Animations<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), MixamoError> {
    guard(&session)?;
    let path = on_mixamo(&mixamo, move |mixamo| mixamo.file(id)).await?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|error| MixamoError::Storage(format!("could not open: {error}")))
}
