use noctics_shared::{MixamoError, MixamoOverview, MotionClip, MotionDraft, MotionOrder};
use serde::Serialize;
use wasm_bindgen::JsValue;

use crate::bridge::{command_error, invoke};

#[derive(Serialize)]
struct NoArgs {}

#[derive(Serialize)]
struct IdArgs {
    id: i64,
}

#[derive(Serialize)]
struct QueryArgs<'a> {
    query: &'a str,
}

#[derive(Serialize)]
struct DraftArgs<'a> {
    draft: &'a MotionDraft,
}

fn mixamo_error(error: JsValue) -> MixamoError {
    command_error(error, MixamoError::Storage)
}

pub async fn overview() -> Result<MixamoOverview, MixamoError> {
    invoke("mixamo_overview", &NoArgs {}).await.map_err(mixamo_error)
}

pub async fn login() -> Result<(), MixamoError> {
    invoke("mixamo_login", &NoArgs {}).await.map_err(mixamo_error)
}

pub async fn search(query: &str) -> Result<Vec<MotionClip>, MixamoError> {
    invoke("mixamo_search", &QueryArgs { query })
        .await
        .map_err(mixamo_error)
}

pub async fn order(draft: &MotionDraft) -> Result<MotionOrder, MixamoError> {
    invoke("order_motion", &DraftArgs { draft })
        .await
        .map_err(mixamo_error)
}

pub async fn plan(project_id: i64) -> Result<Vec<MotionOrder>, MixamoError> {
    invoke("plan_motions", &IdArgs { id: project_id })
        .await
        .map_err(mixamo_error)
}

pub async fn retry(id: i64) -> Result<(), MixamoError> {
    invoke("retry_motion", &IdArgs { id }).await.map_err(mixamo_error)
}

pub async fn delete(id: i64) -> Result<(), MixamoError> {
    invoke("delete_motion", &IdArgs { id }).await.map_err(mixamo_error)
}

pub async fn reveal(id: i64) -> Result<(), MixamoError> {
    invoke("reveal_motion", &IdArgs { id }).await.map_err(mixamo_error)
}
