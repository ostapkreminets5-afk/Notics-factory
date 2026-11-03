use dioxus::prelude::*;
use noctics_shared::{MAX_PROJECT_NAME_CHARS, Project, ProjectStatus};

use crate::bridge;
use crate::build_bridge;
use crate::icons::{CloseIcon, PencilIcon, TrashIcon};
use crate::preview::ScreenPreview;
use crate::studio::{Section, explain, project_status_label, use_studio};

#[component]
pub fn ProjectsPage() -> Element {
    let studio = use_studio();
    let mut notice = use_signal(|| None::<String>);
    let projects = studio.projects.read().clone();
    let games_folder = use_resource(|| async { build_bridge::games_folder().await.ok() });
    let folder_path = games_folder().flatten().map(|folder| folder.path);

    rsx! {
        section { class: "page",
            header { class: "page-head has-action",
                div { class: "page-head-text",
                    p { class: "mono-label", "Система · Projects" }
                    h1 { class: "page-title", "Проєкти" }
                    p { class: "page-lead", "Ігри з конвеєра: назва від Claude, обкладинка — скрін апрувнутого екрана з Claude Design." }
                }
                button {
                    class: "primary-button",
                    r#type: "button",
                    onclick: move |_| studio.open(Section::Pipeline),
                    "Конвеєр гри"
                }
            }
            if let Some(message) = notice() {
                div { class: "page-alert", role: "alert",
                    span { "{message}" }
                    button {
                        class: "icon-button",
                        r#type: "button",
                        aria_label: "Сховати повідомлення",
                        onclick: move |_| notice.set(None),
                        CloseIcon {}
                    }
                }
            }
            if projects.is_empty() {
                div { class: "page-empty",
                    p { class: "mono-label", "Ще немає проєктів" }
                    p { "Опиши ідею гри — ядро проведе її через конвеєр. Після апруву екранів у Claude Design проєкт з’явиться тут." }
                    button {
                        class: "primary-button is-start",
                        r#type: "button",
                        onclick: move |_| studio.open(Section::Pipeline),
                        "Конвеєр гри"
                    }
                }
            } else {
                div { class: "project-grid",
                    for project in projects {
                        ProjectCard {
                            key: "{project.id}",
                            on_problem: move |message| notice.set(Some(message)),
                            project,
                        }
                    }
                }
            }
            if let Some(path) = folder_path {
                p { class: "mono-label projects-folder",
                    "Папка ігор · "
                    span { class: "projects-folder-path", "{path}" }
                    button {
                        class: "projects-folder-change",
                        r#type: "button",
                        onclick: move |_| studio.open(Section::Settings),
                        "Змінити"
                    }
                }
            }
        }
    }
}

#[component]
fn ProjectCard(project: Project, on_problem: EventHandler<String>) -> Element {
    let studio = use_studio();
    let mut renaming = use_signal(|| false);
    let mut draft = use_signal(|| project.name.clone());
    let mut confirming = use_signal(|| false);
    let id = project.id;
    let ready = project.status == ProjectStatus::Ready;
    let busy = project.status.is_planning();
    let current_name = project.name.clone();

    let save = move |event: FormEvent| {
        event.prevent_default();
        spawn(async move {
            match bridge::rename_project(id, &draft()).await {
                Ok(_) => renaming.set(false),
                Err(problem) => on_problem.call(explain(&problem)),
            }
        });
    };

    let remove = move |event: MouseEvent| {
        event.stop_propagation();
        if !confirming() {
            confirming.set(true);
            return;
        }
        spawn(async move {
            if let Err(problem) = bridge::delete_project(id).await {
                on_problem.call(explain(&problem));
                confirming.set(false);
            }
        });
    };

    rsx! {
        article {
            class: if ready { "project-card is-ready" } else { "project-card" },
            ScreenPreview {
                html: project.cover_html.clone(),
                label: if ready { project.name.clone() } else { project_status_label(project.status).to_owned() },
                busy,
            }
            button {
                class: "project-open",
                r#type: "button",
                aria_label: "Відкрити «{project.name}» у Claude Design",
                onclick: move |_| studio.open_project(id),
            }
            div { class: "project-card-body",
                if renaming() {
                    form { class: "project-rename", onsubmit: save,
                        input {
                            class: "input",
                            r#type: "text",
                            aria_label: "Назва проєкту",
                            maxlength: "{MAX_PROJECT_NAME_CHARS}",
                            value: "{draft}",
                            oninput: move |event| draft.set(event.value()),
                            onkeydown: move |event| {
                                if event.key() == Key::Escape {
                                    renaming.set(false);
                                }
                            },
                            onmounted: move |event| async move {
                                let _ = event.data().set_focus(true).await;
                            },
                        }
                        button { class: "primary-button is-small", r#type: "submit", "Зберегти" }
                    }
                } else {
                    div { class: "project-card-head",
                        h2 { class: "project-title", "{project.name}" }
                        div { class: "project-tools",
                            button {
                                class: "icon-button",
                                r#type: "button",
                                aria_label: "Перейменувати",
                                onclick: move |_| {
                                    draft.set(current_name.clone());
                                    renaming.set(true);
                                },
                                PencilIcon {}
                            }
                            button {
                                class: if confirming() { "icon-button is-danger is-armed" } else { "icon-button is-danger" },
                                r#type: "button",
                                aria_label: if confirming() { "Точно видалити?" } else { "Видалити проєкт" },
                                title: if confirming() { "Натисни ще раз, щоб видалити" } else { "Видалити" },
                                onclick: remove,
                                onblur: move |_| confirming.set(false),
                                TrashIcon {}
                            }
                        }
                    }
                }
                p { class: "project-pitch",
                    if project.pitch.is_empty() {
                        "«{project.prompt}»"
                    } else {
                        "{project.pitch}"
                    }
                }
                footer { class: "project-foot",
                    span { class: if ready { "is-ready" } else { "" }, "{project_status_label(project.status)}" }
                    span { "{project.approved_count} / {project.screen_count} апрув" }
                }
            }
        }
    }
}
