use noctics_shared::{AssetDraft, ComfyError, ComfyOverview};
use serde::Serialize;
use wasm_bindgen::JsValue;

use crate::bridge::{command_error, invoke};

#[derive(Serialize)]
struct NoArgs {}

#[derive(Serialize)]
struct DraftArgs<'a> {
    draft: &'a AssetDraft,
}

#[derive(Serialize)]
struct IdArgs {
    id: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToggleArgs<'a> {
    model_id: &'a str,
    enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileArgs {
    run_id: i64,
    index: usize,
}

fn comfy_error(error: JsValue) -> ComfyError {
    command_error(error, ComfyError::Storage)
}

pub async fn overview() -> Result<ComfyOverview, ComfyError> {
    invoke("comfy_overview", &NoArgs {}).await.map_err(comfy_error)
}

pub async fn create(draft: &AssetDraft) -> Result<ComfyOverview, ComfyError> {
    invoke("create_asset_tasks", &DraftArgs { draft })
        .await
        .map_err(comfy_error)
}

pub async fn retry(run_id: i64) -> Result<(), ComfyError> {
    invoke("retry_model_run", &IdArgs { id: run_id })
        .await
        .map_err(comfy_error)
}

pub async fn delete(task_id: i64) -> Result<(), ComfyError> {
    invoke("delete_asset_task", &IdArgs { id: task_id })
        .await
        .map_err(comfy_error)
}

pub async fn set_enabled(model_id: &str, enabled: bool) -> Result<ComfyOverview, ComfyError> {
    invoke("set_model_enabled", &ToggleArgs { model_id, enabled })
        .await
        .map_err(comfy_error)
}

pub async fn launch() -> Result<(), ComfyError> {
    invoke("launch_comfy", &NoArgs {}).await.map_err(comfy_error)
}

pub async fn open_file(run_id: i64, index: usize) -> Result<(), ComfyError> {
    invoke("open_run_file", &FileArgs { run_id, index })
        .await
        .map_err(comfy_error)
}

pub async fn reveal_file(run_id: i64, index: usize) -> Result<(), ComfyError> {
    invoke("reveal_run_file", &FileArgs { run_id, index })
        .await
        .map_err(comfy_error)
}
