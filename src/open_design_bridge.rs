use noctics_shared::{OpenDesignBoard, OpenDesignError};
use serde::Serialize;
use wasm_bindgen::JsValue;

use crate::bridge::{command_error, invoke};

#[derive(Serialize)]
struct ProjectArgs {
    id: i64,
}

#[derive(Serialize)]
struct VariantArgs<'a> {
    id: i64,
    file: &'a str,
}

fn open_design_error(error: JsValue) -> OpenDesignError {
    command_error(error, OpenDesignError::Storage)
}

pub async fn board(project_id: i64) -> Result<OpenDesignBoard, OpenDesignError> {
    invoke("open_design_board", &ProjectArgs { id: project_id })
        .await
        .map_err(open_design_error)
}

pub async fn start(project_id: i64) -> Result<OpenDesignBoard, OpenDesignError> {
    invoke("start_open_design", &ProjectArgs { id: project_id })
        .await
        .map_err(open_design_error)
}

pub async fn open_folder(project_id: i64) -> Result<(), OpenDesignError> {
    invoke("open_design_folder", &ProjectArgs { id: project_id })
        .await
        .map_err(open_design_error)
}

pub async fn open_variant(project_id: i64, file: &str) -> Result<(), OpenDesignError> {
    invoke("open_design_variant", &VariantArgs { id: project_id, file })
        .await
        .map_err(open_design_error)
}
