use std::sync::Arc;

use noctics_shared::{ToolDraft, ToolError, ToolSlot, Toolbox};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::commands::{ActiveSession, off_main_thread};
use crate::toolbox::ToolboxStore;

type Tools<'a> = State<'a, Arc<ToolboxStore>>;

fn guard(session: &ActiveSession) -> Result<(), ToolError> {
    if session.is_signed_in() {
        Ok(())
    } else {
        Err(ToolError::SignedOut)
    }
}

async fn on_store<T, F>(tools: &Arc<ToolboxStore>, task: F) -> Result<T, ToolError>
where
    T: Send + 'static,
    F: FnOnce(&ToolboxStore) -> Result<T, ToolError> + Send + 'static,
{
    let tools = Arc::clone(tools);
    off_main_thread(move || task(&tools), ToolError::Storage).await
}

#[tauri::command]
pub async fn toolbox(
    tools: Tools<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Toolbox, ToolError> {
    guard(&session)?;
    on_store(&tools, |tools| tools.snapshot()).await
}

#[tauri::command]
pub async fn choose_tool(
    slot: ToolSlot,
    tool_id: String,
    tools: Tools<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Toolbox, ToolError> {
    guard(&session)?;
    on_store(&tools, move |tools| tools.choose(slot, &tool_id)).await
}

#[tauri::command]
pub async fn add_tool(
    draft: ToolDraft,
    tools: Tools<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Toolbox, ToolError> {
    guard(&session)?;
    on_store(&tools, move |tools| tools.add(&draft)).await
}

#[tauri::command]
pub async fn remove_tool(
    tool_id: String,
    tools: Tools<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Toolbox, ToolError> {
    guard(&session)?;
    on_store(&tools, move |tools| tools.remove(&tool_id)).await
}

#[tauri::command]
pub async fn open_tool(
    tool_id: String,
    app: AppHandle,
    tools: Tools<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), ToolError> {
    guard(&session)?;
    let url = on_store(&tools, move |tools| tools.url_of(&tool_id)).await?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| ToolError::Storage(format!("could not open the link: {error}")))
}
