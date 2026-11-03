use dioxus::prelude::*;
use noctics_shared::{Project, ProjectStatus};

use crate::icons::{ChevronDownIcon, FolderIcon};
use crate::sidebar_layout::use_sidebar_layout;
use crate::studio::{project_status_label, use_studio};

const PANEL_ID: &str = "project-switcher-panel";

#[component]
pub fn ProjectSwitcher() -> Element {
    let studio = use_studio();
    let layout = use_sidebar_layout();
    let mut open = use_signal(|| false);
    let mut selected = studio.selected;
    let projects = studio.projects.read().clone();
    let current = studio.focused_project();
    let current_id = current.as_ref().map(|project| project.id);

    let (title, note) = match &current {
        Some(project) => (
            project.name.clone(),
            format!("Проєкт · {}", project_status_label(project.status)),
        ),
        None => ("Проєкти".to_owned(), "Ще немає ігор".to_owned()),
    };

    rsx! {
        div {
            class: if open() { "project-switcher is-open" } else { "project-switcher" },
            onkeydown: move |event| {
                if event.key() == Key::Escape {
                    open.set(false);
                }
            },
            button {
                class: "switcher-head",
                r#type: "button",
                title: "Поточний проєкт — натисни, щоб вибрати інший",
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
                FolderIcon {}
                span { class: "nav-text",
                    span { class: "nav-title", "{title}" }
                    span { class: "nav-note", "{note}" }
                }
                ChevronDownIcon {}
            }
            if open() {
                div { class: "switcher-panel", id: PANEL_ID,
                    if projects.is_empty() {
                        p { class: "switcher-empty",
                            "Запусти конвеєр гри — проєкт з’явиться тут."
                        }
                    } else {
                        ul { class: "switcher-list", aria_label: "Проєкти",
                            for project in projects {
                                li { key: "{project.id}",
                                    button {
                                        class: if Some(project.id) == current_id { "switcher-item is-current" } else { "switcher-item" },
                                        r#type: "button",
                                        title: "{project.name} · {project_status_label(project.status)}",
                                        aria_current: if Some(project.id) == current_id { "true" } else { "false" },
                                        onclick: {
                                            let id = project.id;
                                            move |_| {
                                                open.set(false);
                                                selected.set(Some(id));
                                            }
                                        },
                                        span { class: "nav-dot {project_tone(&project)}" }
                                        span { class: "switcher-name", "{project.name}" }
                                        span { class: "switcher-meta",
                                            "{project.approved_count}/{project.screen_count}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn project_tone(project: &Project) -> &'static str {
    match project.status {
        ProjectStatus::Failed => "is-attention",
        status if status.is_planning() || project.pending_count > 0 => "is-busy",
        ProjectStatus::Ready => "is-online",
        _ => "is-idle",
    }
}
