use std::sync::Arc;

use noctics_shared::{ArtstationError, ArtstationStatus, artwork_page};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::artstation::{ArtStation, Scope};
use crate::commands::{ActiveSession, off_main_thread};
use crate::studio::Studio;

type Searcher<'a> = State<'a, Arc<ArtStation>>;

fn guard(session: &ActiveSession) -> Result<(), ArtstationError> {
    if session.is_signed_in() {
        Ok(())
    } else {
        Err(ArtstationError::SignedOut)
    }
}

#[tauri::command]
pub async fn artstation_status(
    artstation: Searcher<'_>,
    session: State<'_, ActiveSession>,
) -> Result<ArtstationStatus, ArtstationError> {
    guard(&session)?;
    Ok(artstation.status())
}

#[tauri::command]
pub async fn load_reference_artworks(
    id: i64,
    artstation: Searcher<'_>,
    studio: State<'_, Arc<Studio>>,
    session: State<'_, ActiveSession>,
) -> Result<(), ArtstationError> {
    guard(&session)?;
    let store = Arc::clone(&studio.store);
    let artstation = Arc::clone(&artstation);
    off_main_thread(
        move || artstation.load_board(store, id, Scope::Everything),
        ArtstationError::Network,
    )
    .await
}

#[tauri::command]
pub async fn open_artwork(
    id: String,
    app: AppHandle,
    session: State<'_, ActiveSession>,
) -> Result<(), ArtstationError> {
    guard(&session)?;
    let link = artwork_page(&id).ok_or(ArtstationError::NotFound)?;
    app.opener()
        .open_url(link, None::<&str>)
        .map_err(|error| ArtstationError::Network(format!("could not open the artwork: {error}")))
}
