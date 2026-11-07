use noctics_shared::{ArtstationError, ArtstationStatus};
use serde::Serialize;
use wasm_bindgen::JsValue;

use crate::bridge::{command_error, invoke};

#[derive(Serialize)]
struct NoArgs {}

#[derive(Serialize)]
struct ProjectArgs {
    id: i64,
}

#[derive(Serialize)]
struct ArtworkArgs<'a> {
    id: &'a str,
}

fn artstation_error(error: JsValue) -> ArtstationError {
    command_error(error, ArtstationError::Network)
}

pub async fn status() -> Result<ArtstationStatus, ArtstationError> {
    invoke("artstation_status", &NoArgs {})
        .await
        .map_err(artstation_error)
}

pub async fn load_reference_artworks(project_id: i64) -> Result<(), ArtstationError> {
    invoke("load_reference_artworks", &ProjectArgs { id: project_id })
        .await
        .map_err(artstation_error)
}

pub async fn open_artwork(artwork_id: &str) -> Result<(), ArtstationError> {
    invoke("open_artwork", &ArtworkArgs { id: artwork_id })
        .await
        .map_err(artstation_error)
}
