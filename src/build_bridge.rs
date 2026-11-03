use noctics_shared::{BuildError, BuildOverview, BuildTarget, GamesFolder};
use serde::Serialize;
use wasm_bindgen::JsValue;

use crate::bridge::{command_error, invoke};

pub const SOURCE_FOLDER: &str = "source";
pub const BUILDS_FOLDER: &str = "builds";

#[derive(Serialize)]
struct NoArgs {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OverviewArgs {
    project_id: i64,
    refresh: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateArgs<'a> {
    project_id: i64,
    request: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectArgs {
    project_id: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LinkArgs<'a> {
    project_id: i64,
    path: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TargetsArgs<'a> {
    project_id: i64,
    targets: &'a [BuildTarget],
}

#[derive(Serialize)]
struct BuildArgs {
    id: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderArgs<'a> {
    project_id: i64,
    folder: &'a str,
}

fn build_error(error: JsValue) -> BuildError {
    command_error(error, BuildError::Storage)
}

pub async fn overview(project_id: i64, refresh: bool) -> Result<BuildOverview, BuildError> {
    invoke("build_overview", &OverviewArgs { project_id, refresh })
        .await
        .map_err(build_error)
}

pub async fn generate(project_id: i64, request: Option<&str>) -> Result<BuildOverview, BuildError> {
    invoke("generate_game", &GenerateArgs { project_id, request })
        .await
        .map_err(build_error)
}

pub async fn stop_generation(project_id: i64) -> Result<(), BuildError> {
    invoke("stop_generation", &ProjectArgs { project_id })
        .await
        .map_err(build_error)
}

pub async fn link_source(project_id: i64, path: &str) -> Result<BuildOverview, BuildError> {
    invoke("link_game_source", &LinkArgs { project_id, path })
        .await
        .map_err(build_error)
}

pub async fn start(project_id: i64, targets: &[BuildTarget]) -> Result<BuildOverview, BuildError> {
    invoke("start_builds", &TargetsArgs { project_id, targets })
        .await
        .map_err(build_error)
}

pub async fn cancel(id: i64) -> Result<(), BuildError> {
    invoke("cancel_build", &BuildArgs { id })
        .await
        .map_err(build_error)
}

pub async fn fix(id: i64) -> Result<(), BuildError> {
    invoke("fix_build", &BuildArgs { id })
        .await
        .map_err(build_error)
}

pub async fn reveal(id: i64) -> Result<(), BuildError> {
    invoke("reveal_build", &BuildArgs { id })
        .await
        .map_err(build_error)
}

pub async fn open_log(id: i64) -> Result<(), BuildError> {
    invoke("open_build_log", &BuildArgs { id })
        .await
        .map_err(build_error)
}

pub async fn open_folder(project_id: i64, folder: &str) -> Result<(), BuildError> {
    invoke("open_game_folder", &FolderArgs { project_id, folder })
        .await
        .map_err(build_error)
}

pub async fn games_folder() -> Result<GamesFolder, BuildError> {
    invoke("games_folder", &NoArgs {}).await.map_err(build_error)
}

pub async fn pick_games_folder() -> Result<GamesFolder, BuildError> {
    invoke("pick_games_folder", &NoArgs {}).await.map_err(build_error)
}

pub async fn reset_games_folder() -> Result<GamesFolder, BuildError> {
    invoke("reset_games_folder", &NoArgs {}).await.map_err(build_error)
}

pub async fn open_games_folder() -> Result<(), BuildError> {
    invoke("open_games_folder", &NoArgs {}).await.map_err(build_error)
}
