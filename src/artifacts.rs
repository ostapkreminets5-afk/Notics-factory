use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_logger::tracing;
use noctics_shared::{
    ARTIFACTS_CHANGED_EVENT, ArtifactDraft, ArtifactError, ArtifactOrigin, DesignArtifact,
    MAX_ARTIFACT_LINK_CHARS, MAX_ARTIFACT_TITLE_CHARS,
};
use serde::Serialize;
use wasm_bindgen::JsValue;

use crate::bridge::{self, command_error, invoke};
use crate::icons::{CloseIcon, ExternalIcon, PanelsIcon, PlusIcon, TrashIcon};

const MILLIS_PER_SECOND: f64 = 1000.0;

#[derive(Serialize)]
struct NoArgs {}

#[derive(Serialize)]
struct DraftArgs<'a> {
    draft: &'a ArtifactDraft,
}

#[derive(Serialize)]
struct IdArgs {
    id: i64,
}

fn artifact_error(error: JsValue) -> ArtifactError {
    command_error(error, ArtifactError::Storage)
}

async fn design_artifacts() -> Result<Vec<DesignArtifact>, ArtifactError> {
    invoke("design_artifacts", &NoArgs {})
        .await
        .map_err(artifact_error)
}

async fn add_design_artifact(draft: &ArtifactDraft) -> Result<Vec<DesignArtifact>, ArtifactError> {
    invoke("add_design_artifact", &DraftArgs { draft })
        .await
        .map_err(artifact_error)
}

async fn remove_design_artifact(id: i64) -> Result<Vec<DesignArtifact>, ArtifactError> {
    invoke("remove_design_artifact", &IdArgs { id })
        .await
        .map_err(artifact_error)
}

async fn open_design_artifact(id: i64) -> Result<(), ArtifactError> {
    invoke("open_design_artifact", &IdArgs { id })
        .await
        .map_err(artifact_error)
}

fn explain(problem: &ArtifactError) -> String {
    match problem {
        ArtifactError::SignedOut => "Сесія завершилась — увійди знову.".to_owned(),
        ArtifactError::NotFound => "Цього артефакту вже немає на полиці.".to_owned(),
        ArtifactError::InvalidLink => {
            "Це не посилання на артефакт claude.ai. Скопіюй адресу виду https://claude.ai/artifact/…".to_owned()
        }
        ArtifactError::Duplicate => "Цей артефакт уже є на полиці.".to_owned(),
        ArtifactError::TitleTooLong => {
            format!("Назва задовга — максимум {MAX_ARTIFACT_TITLE_CHARS} символів.")
        }
        ArtifactError::Window(detail) => {
            tracing::warn!("artifact window failed: {detail}");
            "Не вдалося відкрити вікно артефакту.".to_owned()
        }
        ArtifactError::Storage(detail) => {
            tracing::error!("artifact storage failed: {detail}");
            "Не вдалося звернутися до сховища артефактів.".to_owned()
        }
    }
}

fn origin_label(origin: ArtifactOrigin) -> &'static str {
    match origin {
        ArtifactOrigin::Session => "Авто · з роботи Claude",
        ArtifactOrigin::Manual => "Додано вручну",
    }
}

fn added_on(seconds: i64) -> String {
    let date = js_sys::Date::new(&JsValue::from_f64(seconds as f64 * MILLIS_PER_SECOND));
    format!(
        "{:02}.{:02}.{}",
        date.get_date(),
        date.get_month() + 1,
        date.get_full_year()
    )
}

#[component]
pub fn ArtifactShelf() -> Element {
    let mut artifacts = use_signal(Vec::<DesignArtifact>::new);
    let mut adding = use_signal(|| false);
    let mut link = use_signal(String::new);
    let mut title = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut revision = use_signal(|| 0_u64);
    let subscription = use_hook(|| Rc::new(RefCell::new(None::<bridge::Subscription>)));

    use_future(move || {
        let slot = Rc::clone(&subscription);
        async move {
            let listening = bridge::listen(ARTIFACTS_CHANGED_EVENT, move || {
                if let Ok(mut current) = revision.try_write() {
                    *current += 1;
                }
            })
            .await;
            *slot.borrow_mut() = listening;
        }
    });

    use_resource(move || async move {
        let _ = revision();
        if let Ok(list) = design_artifacts().await {
            artifacts.set(list);
        }
    });

    let mut close_form = move || {
        adding.set(false);
        link.set(String::new());
        title.set(String::new());
        error.set(None);
    };

    let submit = move |event: FormEvent| {
        event.prevent_default();
        if pending() || link.read().trim().is_empty() {
            return;
        }
        pending.set(true);
        error.set(None);
        let draft = ArtifactDraft {
            title: title(),
            link: link(),
        };
        spawn(async move {
            match add_design_artifact(&draft).await {
                Ok(list) => {
                    artifacts.set(list);
                    close_form();
                }
                Err(problem) => error.set(Some(explain(&problem))),
            }
            pending.set(false);
        });
    };

    let open = move |id: i64| {
        spawn(async move {
            if let Err(problem) = open_design_artifact(id).await {
                error.set(Some(explain(&problem)));
            }
        });
    };

    let remove = move |id: i64| {
        spawn(async move {
            match remove_design_artifact(id).await {
                Ok(list) => artifacts.set(list),
                Err(problem) => error.set(Some(explain(&problem))),
            }
        });
    };

    let count = artifacts.read().len();

    rsx! {
        section { class: "artifact-shelf", aria_label: "Артефакти Claude Design",
            header { class: "artifact-shelf-head",
                p { class: "mono-label", "Артефакти Claude Design · {count}" }
                if !adding() {
                    button {
                        class: "ghost-button is-small",
                        r#type: "button",
                        onclick: move |_| adding.set(true),
                        PlusIcon {}
                        "Додати артефакт"
                    }
                }
            }
            if adding() {
                form { class: "artifact-form", novalidate: true, onsubmit: submit,
                    div { class: "field artifact-form-link",
                        label { class: "mono-label", r#for: "artifact-link", "Посилання з claude.ai" }
                        input {
                            class: "input",
                            id: "artifact-link",
                            r#type: "url",
                            placeholder: "https://claude.ai/artifact/…",
                            maxlength: "{MAX_ARTIFACT_LINK_CHARS}",
                            value: "{link}",
                            oninput: move |event| link.set(event.value()),
                            onmounted: move |event| async move {
                                let _ = event.data().set_focus(true).await;
                            },
                        }
                    }
                    div { class: "field artifact-form-title",
                        label { class: "mono-label", r#for: "artifact-title", "Назва · необов’язково" }
                        input {
                            class: "input",
                            id: "artifact-title",
                            r#type: "text",
                            placeholder: "Візьмемо з артефакту",
                            maxlength: "{MAX_ARTIFACT_TITLE_CHARS}",
                            value: "{title}",
                            oninput: move |event| title.set(event.value()),
                        }
                    }
                    div { class: "artifact-form-actions",
                        button {
                            class: "ghost-button",
                            r#type: "button",
                            disabled: pending(),
                            onclick: move |_| close_form(),
                            "Скасувати"
                        }
                        button {
                            class: "primary-button",
                            r#type: "submit",
                            disabled: pending() || link.read().trim().is_empty(),
                            if pending() {
                                span { class: "spinner" }
                            }
                            "Додати"
                        }
                    }
                }
            }
            if let Some(message) = error() {
                div { class: "page-alert", role: "alert",
                    span { "{message}" }
                    button {
                        class: "icon-button",
                        r#type: "button",
                        aria_label: "Сховати повідомлення",
                        onclick: move |_| error.set(None),
                        CloseIcon {}
                    }
                }
            }
            if count == 0 && !adding() {
                p { class: "artifact-shelf-empty",
                    "Артефакти, які Claude створює під час роботи, з’являються тут самі — застосунок стежить за сесіями Claude Code на цьому комп’ютері. Будь-який інший артефакт можна додати посиланням. Відкриваються вони у вікні застосунку: увійди там у claude.ai один раз, і сесія збережеться."
                }
            }
            div { class: "artifact-grid",
                for artifact in artifacts.read().iter().cloned() {
                    ArtifactCard {
                        key: "{artifact.id}",
                        artifact,
                        on_open: open,
                        on_remove: remove,
                    }
                }
            }
        }
    }
}

#[component]
fn ArtifactCard(
    artifact: DesignArtifact,
    on_open: EventHandler<i64>,
    on_remove: EventHandler<i64>,
) -> Element {
    let mut armed = use_signal(|| false);
    let id = artifact.id;
    let code = artifact.code().to_owned();
    let added = added_on(artifact.added_at);

    rsx! {
        article {
            class: "artifact-card",
            tabindex: "0",
            aria_label: "{artifact.title} — відкрити артефакт",
            onclick: move |_| on_open.call(id),
            onkeydown: move |event| {
                if event.key() == Key::Enter {
                    on_open.call(id);
                }
            },
            onmouseleave: move |_| armed.set(false),
            div { class: "artifact-card-top",
                span { class: "artifact-badge", PanelsIcon {} }
                span { class: "mono-label", "{origin_label(artifact.origin)}" }
                button {
                    class: if armed() { "icon-button is-danger is-armed" } else { "icon-button is-danger" },
                    r#type: "button",
                    aria_label: if armed() { "Точно прибрати з полиці" } else { "Прибрати з полиці" },
                    title: if armed() { "Натисни ще раз, щоб прибрати" } else { "Прибрати з полиці" },
                    onclick: move |event| {
                        event.stop_propagation();
                        if armed() {
                            on_remove.call(id);
                        } else {
                            armed.set(true);
                        }
                    },
                    onkeydown: move |event| event.stop_propagation(),
                    TrashIcon {}
                }
            }
            h3 { class: "artifact-card-title", "{artifact.title}" }
            p { class: "artifact-card-meta", "{code} · {added}" }
            span { class: "artifact-card-open",
                "Відкрити"
                ExternalIcon {}
            }
        }
    }
}
