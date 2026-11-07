use noctics_shared::{AssetHunt, StudioError};
use serde::Serialize;
use wasm_bindgen::JsValue;

use crate::bridge::{command_error, invoke};

#[derive(Serialize)]
struct IdArgs {
    id: i64,
}

#[derive(Serialize)]
struct StartArgs<'a> {
    id: i64,
    request: &'a str,
}

#[derive(Serialize)]
struct FindArgs<'a> {
    id: i64,
    url: &'a str,
}

fn hunt_error(error: JsValue) -> StudioError {
    command_error(error, StudioError::Process)
}

pub async fn hunt(project_id: i64) -> Result<Option<AssetHunt>, StudioError> {
    invoke("asset_hunt", &IdArgs { id: project_id })
        .await
        .map_err(hunt_error)
}

pub async fn start(project_id: i64, request: &str) -> Result<AssetHunt, StudioError> {
    invoke("start_asset_hunt", &StartArgs { id: project_id, request })
        .await
        .map_err(hunt_error)
}

pub async fn open(project_id: i64, url: &str) -> Result<(), StudioError> {
    invoke("open_asset_find", &FindArgs { id: project_id, url })
        .await
        .map_err(hunt_error)
}
