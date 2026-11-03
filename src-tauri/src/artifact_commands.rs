use std::sync::Arc;

use noctics_shared::{ArtifactDraft, ArtifactError, DesignArtifact};
use tauri::{AppHandle, State};

use crate::artifacts::{ArtifactStore, open_window};
use crate::commands::{ActiveSession, off_main_thread};

type Shelf<'a> = State<'a, Arc<ArtifactStore>>;

fn guard(session: &ActiveSession) -> Result<(), ArtifactError> {
    if session.is_signed_in() {
        Ok(())
    } else {
        Err(ArtifactError::SignedOut)
    }
}

async fn on_store<T, F>(shelf: &Arc<ArtifactStore>, task: F) -> Result<T, ArtifactError>
where
    T: Send + 'static,
    F: FnOnce(&ArtifactStore) -> Result<T, ArtifactError> + Send + 'static,
{
    let shelf = Arc::clone(shelf);
    off_main_thread(move || task(&shelf), ArtifactError::Storage).await
}

#[tauri::command]
pub async fn design_artifacts(
    shelf: Shelf<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Vec<DesignArtifact>, ArtifactError> {
    guard(&session)?;
    on_store(&shelf, |shelf| shelf.list()).await
}

#[tauri::command]
pub async fn add_design_artifact(
    draft: ArtifactDraft,
    shelf: Shelf<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Vec<DesignArtifact>, ArtifactError> {
    guard(&session)?;
    on_store(&shelf, move |shelf| shelf.add(&draft)).await
}

#[tauri::command]
pub async fn remove_design_artifact(
    id: i64,
    shelf: Shelf<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Vec<DesignArtifact>, ArtifactError> {
    guard(&session)?;
    on_store(&shelf, move |shelf| shelf.remove(id)).await
}

#[tauri::command]
pub async fn open_design_artifact(
    id: i64,
    app: AppHandle,
    shelf: Shelf<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), ArtifactError> {
    guard(&session)?;
    let artifact = on_store(&shelf, move |shelf| shelf.get(id)).await?;
    open_window(&app, Arc::clone(&shelf), &artifact)
}
