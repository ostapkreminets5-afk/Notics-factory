use std::sync::Arc;

use noctics_shared::{OpenDesignBoard, OpenDesignError};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::commands::{ActiveSession, off_main_thread};
use crate::open_design::OpenDesign;
use crate::studio::Studio;

type Designer<'a> = State<'a, Arc<OpenDesign>>;

fn guard(session: &ActiveSession) -> Result<(), OpenDesignError> {
    if session.is_signed_in() {
        Ok(())
    } else {
        Err(OpenDesignError::SignedOut)
    }
}

#[tauri::command]
pub async fn open_design_board(
    id: i64,
    open_design: Designer<'_>,
    session: State<'_, ActiveSession>,
) -> Result<OpenDesignBoard, OpenDesignError> {
    guard(&session)?;
    let open_design = Arc::clone(&open_design);
    off_main_thread(move || Ok(open_design.board(id)), OpenDesignError::Storage).await
}

#[tauri::command]
pub async fn start_open_design(
    id: i64,
    open_design: Designer<'_>,
    studio: State<'_, Arc<Studio>>,
    session: State<'_, ActiveSession>,
) -> Result<OpenDesignBoard, OpenDesignError> {
    guard(&session)?;
    let open_design = Arc::clone(&open_design);
    let store = Arc::clone(&studio.store);
    off_main_thread(
        move || {
            open_design.start(store, id)?;
            Ok(open_design.board(id))
        },
        OpenDesignError::Storage,
    )
    .await
}

#[tauri::command]
pub async fn open_design_folder(
    id: i64,
    app: AppHandle,
    open_design: Designer<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), OpenDesignError> {
    guard(&session)?;
    let folder = open_design.project_folder(id).ok_or(OpenDesignError::NotFound)?;
    app.opener()
        .open_path(folder.to_string_lossy(), None::<&str>)
        .map_err(|error| OpenDesignError::Storage(format!("could not open the folder: {error}")))
}

#[tauri::command]
pub async fn open_design_variant(
    id: i64,
    file: String,
    app: AppHandle,
    open_design: Designer<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), OpenDesignError> {
    guard(&session)?;
    let link = open_design.variant_url(id, &file).ok_or(OpenDesignError::NotFound)?;
    app.opener()
        .open_url(link, None::<&str>)
        .map_err(|error| OpenDesignError::Daemon(format!("could not open the design: {error}")))
}
