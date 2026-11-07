use std::sync::Arc;

use noctics_shared::{AssetHunt, StudioError, is_web_link};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::asset_hunt::Hunter;
use crate::commands::{ActiveSession, off_main_thread};

type Hunters<'a> = State<'a, Arc<Hunter>>;

fn guard(session: &ActiveSession) -> Result<(), StudioError> {
    if session.is_signed_in() {
        Ok(())
    } else {
        Err(StudioError::SignedOut)
    }
}

async fn on_hunter<T, F>(hunter: &Arc<Hunter>, task: F) -> Result<T, StudioError>
where
    T: Send + 'static,
    F: FnOnce(&Arc<Hunter>) -> Result<T, StudioError> + Send + 'static,
{
    let hunter = Arc::clone(hunter);
    off_main_thread(move || task(&hunter), StudioError::Storage).await
}

#[tauri::command]
pub async fn asset_hunt(
    id: i64,
    hunter: Hunters<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Option<AssetHunt>, StudioError> {
    guard(&session)?;
    on_hunter(&hunter, move |hunter| {
        hunter.kick();
        hunter.hunt(id)
    })
    .await
}

#[tauri::command]
pub async fn start_asset_hunt(
    id: i64,
    request: String,
    hunter: Hunters<'_>,
    session: State<'_, ActiveSession>,
) -> Result<AssetHunt, StudioError> {
    guard(&session)?;
    on_hunter(&hunter, move |hunter| hunter.start(id, &request)).await
}

#[tauri::command]
pub async fn open_asset_find(
    id: i64,
    url: String,
    app: AppHandle,
    hunter: Hunters<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), StudioError> {
    guard(&session)?;
    let find = on_hunter(&hunter, move |hunter| hunter.find(id, &url)).await?;
    if !is_web_link(&find.url) {
        return Err(StudioError::NotFound);
    }
    app.opener()
        .open_url(find.url.as_str(), None::<&str>)
        .map_err(|error| StudioError::Process(format!("could not open the link: {error}")))
}
