use std::sync::Arc;

use noctics_shared::{PinterestError, PinterestStatus, pin_page};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::commands::{ActiveSession, off_main_thread};
use crate::pinterest::{Pinterest, Refresh};
use crate::studio::Studio;

type Boards<'a> = State<'a, Arc<Pinterest>>;

fn guard(session: &ActiveSession) -> Result<(), PinterestError> {
    if session.is_signed_in() {
        Ok(())
    } else {
        Err(PinterestError::SignedOut)
    }
}

async fn in_background<T, F>(pinterest: &Arc<Pinterest>, task: F) -> Result<T, PinterestError>
where
    T: Send + 'static,
    F: FnOnce(&Arc<Pinterest>) -> Result<T, PinterestError> + Send + 'static,
{
    let pinterest = Arc::clone(pinterest);
    off_main_thread(move || task(&pinterest), PinterestError::Browser).await
}

#[tauri::command]
pub async fn pinterest_status(
    pinterest: Boards<'_>,
    session: State<'_, ActiveSession>,
) -> Result<PinterestStatus, PinterestError> {
    guard(&session)?;
    in_background(&pinterest, |pinterest| Ok(pinterest.status())).await
}

#[tauri::command]
pub async fn pinterest_login(
    pinterest: Boards<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), PinterestError> {
    guard(&session)?;
    in_background(&pinterest, |pinterest| pinterest.open_login()).await
}

#[tauri::command]
pub async fn load_reference_pins(
    id: i64,
    pinterest: Boards<'_>,
    studio: State<'_, Arc<Studio>>,
    session: State<'_, ActiveSession>,
) -> Result<(), PinterestError> {
    guard(&session)?;
    let store = Arc::clone(&studio.store);
    in_background(&pinterest, move |pinterest| {
        pinterest.load_board(store, id, Refresh::Everything)
    })
    .await
}

#[tauri::command]
pub async fn open_pin(
    id: String,
    app: AppHandle,
    session: State<'_, ActiveSession>,
) -> Result<(), PinterestError> {
    guard(&session)?;
    let link = pin_page(&id).ok_or(PinterestError::NotFound)?;
    app.opener()
        .open_url(link, None::<&str>)
        .map_err(|error| PinterestError::Browser(format!("could not open the pin: {error}")))
}
