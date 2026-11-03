use dioxus::prelude::*;
use noctics_shared::{GameEngine, StudioError};

use crate::bridge;
use crate::icons::{ChevronDownIcon, CodeIcon};
use crate::sidebar_layout::use_sidebar_layout;
use crate::studio::{explain, use_studio};

const PANEL_ID: &str = "engine-switcher-panel";

#[component]
pub fn EngineSwitcher() -> Element {
    let studio = use_studio();
    let layout = use_sidebar_layout();
    let mut open = use_signal(|| false);
    let mut error = use_signal(|| None::<StudioError>);
    let focused = studio.focused_project();
    let project_id = focused.as_ref().map(|project| project.id);
    let current = focused.map_or_else(|| (studio.draft_engine)(), |project| project.engine);
    let note = if project_id.is_some() {
        format!("Рушій · {}", current.stack())
    } else {
        format!("Рушій нової гри · {}", current.stack())
    };

    let mut choose = move |engine: GameEngine| {
        open.set(false);
        error.set(None);
        let Some(id) = project_id else {
            studio.choose_draft_engine(engine);
            return;
        };
        if engine == current {
            return;
        }
        spawn(async move {
            match bridge::set_project_engine(id, engine).await {
                Ok(_) => studio.changed(),
                Err(problem) => error.set(Some(problem)),
            }
        });
    };

    rsx! {
        div {
            class: if open() { "project-switcher engine-switcher is-open" } else { "project-switcher engine-switcher" },
            onkeydown: move |event| {
                if event.key() == Key::Escape {
                    open.set(false);
                }
            },
            button {
                class: "switcher-head",
                r#type: "button",
                title: "На чому пишеться гра — натисни, щоб змінити рушій",
                aria_expanded: if open() { "true" } else { "false" },
                aria_controls: PANEL_ID,
                onclick: move |_| {
                    if layout.collapsed() {
                        layout.set_collapsed(false);
                        open.set(true);
                    } else {
                        open.set(!open());
                    }
                },
                CodeIcon {}
                span { class: "nav-text",
                    span { class: "nav-title", "{current.label()}" }
                    span { class: "nav-note", "{note}" }
                }
                ChevronDownIcon {}
            }
            if let Some(problem) = error() {
                p { class: "switcher-empty is-error", role: "alert", "{explain(&problem)}" }
            }
            if open() {
                div { class: "switcher-panel", id: PANEL_ID,
                    ul { class: "switcher-list", role: "listbox", aria_label: "Рушій гри",
                        for engine in GameEngine::ALL {
                            li { key: "{engine.code()}",
                                button {
                                    class: if engine == current { "switcher-item is-current" } else { "switcher-item" },
                                    r#type: "button",
                                    role: "option",
                                    aria_selected: if engine == current { "true" } else { "false" },
                                    onclick: move |_| choose(engine),
                                    span { class: if engine == current { "nav-dot is-online" } else { "nav-dot" } }
                                    span { class: "switcher-name", "{engine.label()}" }
                                    span { class: "switcher-meta", "{engine.stack()}" }
                                }
                            }
                        }
                    }
                    p { class: "switcher-empty engine-hint",
                        "Claude писатиме код, сцени й структуру проєкту під обраний рушій."
                    }
                }
            }
        }
    }
}
