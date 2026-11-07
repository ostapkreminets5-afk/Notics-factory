mod accounts;
mod artifact_commands;
mod artifact_watch;
mod artifacts;
mod asset_hunt;
mod asset_hunt_commands;
mod asset_hunt_store;
mod asset_previews;
mod artstation;
mod artstation_api;
mod artstation_commands;
mod build_commands;
mod build_store;
mod builder;
mod claude_code;
mod codegen;
mod comfy;
mod comfy_api;
mod comfy_commands;
mod comfy_forge;
mod comfy_prompts;
mod comfy_recipes;
mod comfy_store;
mod commands;
mod games_root;
mod local_api;
mod mixamo;
mod mixamo_api;
mod mixamo_catalog;
mod mixamo_commands;
mod mixamo_store;
mod open_design;
mod open_design_commands;
mod pinterest;
mod pinterest_api;
mod pinterest_commands;
mod playtest;
mod process;
mod prompts;
mod recipes;
mod skill_seeds;
mod skills;
mod studio;
mod studio_commands;
mod studio_store;
mod toolbox;
mod toolbox_commands;
mod toolchains;
mod usage;
mod window_zoom;

use std::sync::Arc;

use tauri::Manager;

use crate::accounts::AccountStore;
use crate::artifacts::ArtifactStore;
use crate::asset_hunt::Hunter;
use crate::asset_hunt_store::HuntStore;
use crate::artstation::ArtStation;
use crate::open_design::OpenDesign;
use crate::build_store::BuildStore;
use crate::builder::Builder;
use crate::comfy::COMFY_URL;
use crate::comfy_forge::Forge;
use crate::comfy_store::ComfyStore;
use crate::commands::ActiveSession;
use crate::games_root::GamesRoot;
use crate::mixamo::Mixamo;
use crate::mixamo_store::MixamoStore;
use crate::pinterest::Pinterest;
use crate::skills::SkillStore;
use crate::studio::Studio;
use crate::studio_store::StudioStore;
use crate::toolbox::ToolboxStore;

const ACCOUNTS_FILE: &str = "accounts.sqlite3";
const SKILLS_FILE: &str = "skills.sqlite3";
const STUDIO_FILE: &str = "studio.sqlite3";
const TOOLBOX_FILE: &str = "toolbox.sqlite3";
const BUILDS_FILE: &str = "builds.sqlite3";
const ARTIFACTS_FILE: &str = "artifacts.sqlite3";
const COMFY_FILE: &str = "comfy.sqlite3";
const ASSET_HUNT_FILE: &str = "asset-hunt.sqlite3";
const MIXAMO_FILE: &str = "mixamo.sqlite3";
const MIXAMO_LIBRARY_DIR: &str = "mixamo-library";
const COMFY_GALLERY_DIR: &str = "comfy-gallery";
const COMFY_LAUNCH_LOG: &str = "comfyui-launch.log";
const SKILLS_EXPORT_DIR: &str = "skills";
const CLAUDE_WORKDIR: &str = "claude-workspace";
const GAMES_DIR: &str = "NocticsFactory";
const FALLBACK_GAMES_DIR: &str = "games";
const GAMES_FOLDER_FILE: &str = "games-folder.txt";
const BUILD_CACHE_DIR: &str = "build-cache";
const OPEN_DESIGN_DIR: &str = "open-design";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(ActiveSession::default())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let accounts = AccountStore::open(&data_dir.join(ACCOUNTS_FILE))?;
            let skills = Arc::new(SkillStore::open(
                &data_dir.join(SKILLS_FILE),
                data_dir.join(SKILLS_EXPORT_DIR),
            )?);
            let store = Arc::new(StudioStore::open(&data_dir.join(STUDIO_FILE))?);
            let studio = Arc::new(Studio::new(
                store,
                Arc::clone(&skills),
                app.handle().clone(),
                data_dir.join(CLAUDE_WORKDIR),
            ));
            let tools = ToolboxStore::open(&data_dir.join(TOOLBOX_FILE))?;
            app.manage(Arc::new(accounts));
            app.manage(Arc::new(tools));
            let artifacts = Arc::new(ArtifactStore::open(&data_dir.join(ARTIFACTS_FILE))?);
            artifact_watch::watch(app.handle().clone(), Arc::clone(&artifacts));
            app.manage(artifacts);
            app.manage(skills);
            let pinterest = Arc::new(Pinterest::new(app.handle().clone()));
            pinterest_api::serve(Arc::clone(&pinterest));
            app.manage(pinterest);
            let artstation = Arc::new(ArtStation::new(app.handle().clone()));
            artstation_api::serve(Arc::clone(&artstation));
            app.manage(artstation);
            app.manage(Arc::new(OpenDesign::new(
                app.handle().clone(),
                data_dir.join(OPEN_DESIGN_DIR),
            )));
            let default_games_root = app
                .path()
                .document_dir()
                .map(|documents| documents.join(GAMES_DIR))
                .unwrap_or_else(|_| data_dir.join(FALLBACK_GAMES_DIR));
            let games_root = Arc::new(GamesRoot::open(
                data_dir.join(GAMES_FOLDER_FILE),
                default_games_root,
            ));
            app.manage(Arc::clone(&games_root));
            let builder = Arc::new(Builder::new(
                Arc::new(BuildStore::open(&data_dir.join(BUILDS_FILE))?),
                Arc::clone(&studio),
                app.handle().clone(),
                games_root,
                data_dir.join(BUILD_CACHE_DIR),
            ));
            let forge = Arc::new(Forge::new(
                Arc::new(ComfyStore::open(&data_dir.join(COMFY_FILE))?),
                Arc::clone(&studio),
                app.handle().clone(),
                COMFY_URL,
                data_dir.join(COMFY_GALLERY_DIR),
                data_dir.join(COMFY_LAUNCH_LOG),
            ));
            comfy_api::serve(Arc::clone(&forge));
            app.manage(Arc::clone(&forge));
            app.manage(Arc::clone(&studio));
            app.manage(Arc::clone(&builder));
            let hunter = Arc::new(Hunter::new(
                Arc::new(HuntStore::open(&data_dir.join(ASSET_HUNT_FILE))?),
                Arc::clone(&studio),
            ));
            app.manage(Arc::clone(&hunter));
            let mixamo = Arc::new(Mixamo::new(
                Arc::new(MixamoStore::open(&data_dir.join(MIXAMO_FILE))?),
                Arc::clone(&studio),
                app.handle().clone(),
                data_dir.join(MIXAMO_LIBRARY_DIR),
            ));
            mixamo_api::serve(Arc::clone(&mixamo));
            app.manage(Arc::clone(&mixamo));
            mixamo.start();
            studio.kick();
            builder.kick();
            forge.start();
            hunter.kick();
            pinterest::resume_after_start(app.handle().clone());
            artstation::resume_after_start(app.handle().clone());
            open_design::resume_after_start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            window_zoom::set_ui_zoom,
            commands::has_accounts,
            commands::register,
            commands::login,
            commands::logout,
            commands::current_session,
            commands::list_skills,
            commands::create_skill,
            commands::update_skill,
            commands::set_skill_enabled,
            commands::delete_skill,
            commands::skills_folder,
            studio_commands::studio_settings,
            studio_commands::claude_usage,
            studio_commands::claude_login,
            studio_commands::list_projects,
            studio_commands::project_detail,
            studio_commands::project_journal,
            studio_commands::start_project,
            studio_commands::set_project_engine,
            studio_commands::add_screen,
            studio_commands::refine_screen,
            studio_commands::regenerate_screen,
            studio_commands::approve_screen,
            studio_commands::form_project,
            studio_commands::rename_project,
            studio_commands::delete_project,
            studio_commands::sync_project,
            studio_commands::regenerate_output,
            studio_commands::pick_references,
            studio_commands::restyle_project,
            studio_commands::open_reference,
            pinterest_commands::pinterest_status,
            pinterest_commands::pinterest_login,
            pinterest_commands::load_reference_pins,
            pinterest_commands::open_pin,
            artstation_commands::artstation_status,
            artstation_commands::load_reference_artworks,
            artstation_commands::open_artwork,
            open_design_commands::open_design_board,
            open_design_commands::start_open_design,
            open_design_commands::open_design_folder,
            open_design_commands::open_design_variant,
            toolbox_commands::toolbox,
            toolbox_commands::choose_tool,
            toolbox_commands::add_tool,
            toolbox_commands::remove_tool,
            toolbox_commands::open_tool,
            artifact_commands::design_artifacts,
            artifact_commands::add_design_artifact,
            artifact_commands::remove_design_artifact,
            artifact_commands::open_design_artifact,
            build_commands::build_overview,
            build_commands::generate_game,
            build_commands::stop_generation,
            build_commands::link_game_source,
            build_commands::start_builds,
            build_commands::cancel_build,
            build_commands::fix_build,
            build_commands::reveal_build,
            build_commands::open_build_log,
            build_commands::open_game_folder,
            build_commands::start_playtest,
            build_commands::stop_playtest,
            build_commands::games_folder,
            build_commands::pick_games_folder,
            build_commands::reset_games_folder,
            build_commands::open_games_folder,
            comfy_commands::comfy_overview,
            comfy_commands::create_asset_tasks,
            comfy_commands::retry_model_run,
            comfy_commands::delete_asset_task,
            comfy_commands::set_model_enabled,
            comfy_commands::launch_comfy,
            comfy_commands::open_run_file,
            comfy_commands::reveal_run_file,
            asset_hunt_commands::asset_hunt,
            asset_hunt_commands::start_asset_hunt,
            asset_hunt_commands::open_asset_find,
            mixamo_commands::mixamo_overview,
            mixamo_commands::mixamo_login,
            mixamo_commands::mixamo_search,
            mixamo_commands::order_motion,
            mixamo_commands::plan_motions,
            mixamo_commands::retry_motion,
            mixamo_commands::delete_motion,
            mixamo_commands::reveal_motion,
            commands::window_minimize,
            commands::window_toggle_maximize,
            commands::window_is_maximized,
            commands::window_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Noctics Factory");
}
