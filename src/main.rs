mod app;
mod artifacts;
mod auth;
mod bridge;
mod build;
mod build_bridge;
mod comfy;
mod comfy_bridge;
mod design;
mod engine_switcher;
mod fonts;
mod home;
mod icons;
mod journal;
mod modal;
mod pipeline;
mod preview;
mod project_switcher;
mod projects;
mod references;
mod settings;
mod sidebar_layout;
mod skills;
mod stage_track;
mod studio;
mod titlebar;
mod tools;
mod workspace;

use app::App;
use dioxus::prelude::*;
use dioxus_logger::tracing::Level;

fn main() {
    dioxus_logger::init(Level::INFO).expect("failed to init logger");
    launch(App);
}
