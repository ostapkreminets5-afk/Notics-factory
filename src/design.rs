use dioxus::prelude::*;
use noctics_shared::{
    MAX_FEEDBACK_CHARS, MAX_PROJECT_NAME_CHARS, MAX_SCREEN_SUMMARY_CHARS, MAX_SCREEN_TITLE_CHARS,
    OutputStatus, Project, ProjectDetail, ProjectStatus, Screen, ScreenStatus, SkillOutput,
    StyleGuide,
};

use crate::artifacts::ArtifactShelf;
use crate::bridge;
use crate::icons::CloseIcon;
use crate::modal::Modal;
use crate::preview::ScreenPreview;
use crate::studio::{
    Section, explain, output_status_label, output_status_tone, project_status_label,
    screen_status_label, screen_status_tone, use_studio,
};

#[derive(Clone, PartialEq)]
enum Dialog {
    Closed,
    Viewing(i64),
    Adding,
    Forming(i64),
    Output(i64),
}

#[component]
pub fn ClaudeDesignPage() -> Element {
    let studio = use_studio();
    let mut notice = use_signal(|| None::<String>);
    let mut dialog = use_signal(|| Dialog::Closed);
    let mut syncing = use_signal(|| false);

    let detail = use_resource(move || async move {
        let _ = (studio.revision)();
        let id = studio.focused_project()?.id;
        bridge::project_detail(id).await.ok()
    });

    let focused = studio.focused_project();
    let loaded: Option<ProjectDetail> = detail
        .read()
        .clone()
        .flatten()
        .filter(|loaded| focused.as_ref().is_some_and(|project| project.id == loaded.project.id));
    let project = loaded.as_ref().map(|loaded| loaded.project.clone()).or(focused);
    let projects = studio.projects.read().clone();
    let designing = project
        .as_ref()
        .is_some_and(|project| matches!(project.status, ProjectStatus::Designing | ProjectStatus::Ready));
    let first_approved = loaded.as_ref().and_then(|loaded| {
        loaded
            .screens
            .iter()
            .find(|screen| screen.status == ScreenStatus::Approved)
            .map(|screen| screen.id)
    });
    let formable = project
        .as_ref()
        .is_some_and(|project| project.status == ProjectStatus::Designing)
        && first_approved.is_some();
    let project_id = project.as_ref().map(|project| project.id);

    let sync = move |_| {
        let Some(id) = project_id else {
            return;
        };
        syncing.set(true);
        spawn(async move {
            if let Err(problem) = bridge::sync_project(id).await {
                notice.set(Some(explain(&problem)));
            }
            syncing.set(false);
        });
    };

    rsx! {
        section { class: "page",
            header { class: "page-head has-action",
                div { class: "page-head-text",
                    p { class: "mono-label", "Система · Claude Design" }
                    h1 { class: "page-title", "Інтерфейс гри" }
                    p { class: "page-lead", "Екрани, які ядро проєктує в Claude Design і передає на етап збірки." }
                }
                div { class: "head-actions",
                    if let (true, Some(cover)) = (formable, first_approved) {
                        button {
                            class: "ghost-button",
                            r#type: "button",
                            onclick: move |_| dialog.set(Dialog::Forming(cover)),
                            "Сформувати проєкт"
                        }
                    }
                    button {
                        class: "ghost-button",
                        r#type: "button",
                        disabled: project_id.is_none() || syncing(),
                        onclick: sync,
                        if syncing() {
                            span { class: "spinner" }
                        }
                        "Синхронізувати"
                    }
                    button {
                        class: "primary-button",
                        r#type: "button",
                        disabled: !designing,
                        onclick: move |_| dialog.set(Dialog::Adding),
                        "+ Згенерувати екран"
                    }
                }
            }
            if projects.len() > 1 {
                div { class: "chip-row", role: "tablist", aria_label: "Проєкти",
                    for candidate in projects.iter() {
                        {
                            let id = candidate.id;
                            let active = project_id == Some(id);
                            rsx! {
                                button {
                                    key: "{id}",
                                    class: if active { "chip is-active" } else { "chip" },
                                    r#type: "button",
                                    role: "tab",
                                    aria_selected: if active { "true" } else { "false" },
                                    onclick: move |_| {
                                        let mut selected = studio.selected;
                                        selected.set(Some(id));
                                    },
                                    "{candidate.name}"
                                }
                            }
                        }
                    }
                }
            }
            if !studio.connected() && studio.settings.read().is_some() {
                div { class: "page-alert is-info", role: "status",
                    span { "Щоб Claude генерував екрани, підключи підписку Claude в Налаштуваннях." }
                    button {
                        class: "ghost-button is-small",
                        r#type: "button",
                        onclick: move |_| studio.open(Section::Settings),
                        "Налаштування"
                    }
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
            ArtifactShelf {}
            match project.clone() {
                None => rsx! {
                    div { class: "page-empty",
                        p { class: "mono-label", "Ще немає екранів" }
                        p { "Опиши гру в конвеєрі — Claude складе дизайн-документ, стиль і спроєктує екрани тут." }
                        button {
                            class: "primary-button is-start",
                            r#type: "button",
                            onclick: move |_| studio.open(Section::Pipeline),
                            "Відкрити конвеєр гри"
                        }
                    }
                },
                Some(project) if project.status.is_planning() => rsx! {
                    div { class: "planning-card",
                        span { class: "spinner" }
                        div {
                            p { class: "mono-label", "{project_status_label(project.status)}" }
                            p { class: "planning-text",
                                "Claude читає навички студії й складає дизайн-документ, стиль і список екранів для гри."
                            }
                            p { class: "planning-prompt", "«{project.prompt}»" }
                        }
                    }
                },
                Some(project) if project.status == ProjectStatus::Failed => rsx! {
                    div { class: "page-alert", role: "alert",
                        span {
                            {project.error.as_ref().map(explain).unwrap_or_else(|| "Не вдалося спланувати гру.".to_owned())}
                        }
                        button {
                            class: "ghost-button is-small",
                            r#type: "button",
                            onclick: sync,
                            "Спробувати знову"
                        }
                    }
                },
                Some(_) => rsx! {
                    if let Some(loaded) = loaded.clone() {
                        StyleBar { style: loaded.project.style.clone() }
                        div { class: "design-grid",
                            for screen in loaded.screens.iter().cloned() {
                                DesignCard {
                                    key: "{screen.id}",
                                    on_open: move |id| dialog.set(Dialog::Viewing(id)),
                                    screen,
                                }
                            }
                        }
                        SkillResults {
                            outputs: loaded.outputs.clone(),
                            on_open: move |id| dialog.set(Dialog::Output(id)),
                        }
                    }
                },
            }
        }
        {
            let close = move |_| dialog.set(Dialog::Closed);
            match (dialog(), loaded.clone()) {
                (Dialog::Viewing(screen_id), Some(loaded)) => {
                    match loaded.screens.iter().find(|screen| screen.id == screen_id).cloned() {
                        Some(screen) => rsx! {
                            ScreenViewer {
                                key: "{screen_id}",
                                project: loaded.project.clone(),
                                screen,
                                on_close: close,
                                on_form: move |cover| dialog.set(Dialog::Forming(cover)),
                            }
                        },
                        None => rsx! {},
                    }
                }
                (Dialog::Output(output_id), Some(loaded)) => {
                    match loaded.outputs.iter().find(|output| output.id == output_id).cloned() {
                        Some(output) => rsx! {
                            OutputViewer { key: "{output_id}", output, on_close: close }
                        },
                        None => rsx! {},
                    }
                }
                (Dialog::Adding, Some(loaded)) => rsx! {
                    AddScreenDialog { project_id: loaded.project.id, on_close: close }
                },
                (Dialog::Forming(cover), Some(loaded)) => rsx! {
                    FormProjectDialog {
                        project: loaded.project.clone(),
                        candidates: loaded
                            .screens
                            .iter()
                            .filter(|screen| screen.status == ScreenStatus::Approved)
                            .cloned()
                            .collect::<Vec<_>>(),
                        initial_cover: cover,
                        on_close: close,
                        on_formed: move |_| {
                            dialog.set(Dialog::Closed);
                            studio.open(Section::Projects);
                        },
                    }
                },
                _ => rsx! {},
            }
        }
    }
}

#[component]
fn SkillResults(outputs: Vec<SkillOutput>, on_open: EventHandler<i64>) -> Element {
    let studio = use_studio();
    let done = outputs
        .iter()
        .filter(|output| output.status == OutputStatus::Done)
        .count();
    rsx! {
        section { class: "skill-results",
            header { class: "skill-results-head",
                p { class: "mono-label", "Результати навичок · {done} / {outputs.len()}" }
                button {
                    class: "ghost-button is-small",
                    r#type: "button",
                    onclick: move |_| studio.open(Section::Skills),
                    "Керувати навичками"
                }
            }
            if outputs.is_empty() {
                p { class: "skill-results-empty",
                    "Для цього проєкту ще немає результатів навичок. Увімкни навички в SKILLS і натисни «Синхронізувати»."
                }
            }
            div { class: "skill-results-list",
                for output in outputs {
                    {
                        let id = output.id;
                        let preview = output
                            .content
                            .as_deref()
                            .map(preview_line)
                            .or_else(|| output.error.as_ref().map(explain))
                            .unwrap_or_else(|| "Claude ще не дійшов до цієї навички.".to_owned());
                        rsx! {
                            button {
                                key: "{id}",
                                class: "skill-result",
                                r#type: "button",
                                onclick: move |_| on_open.call(id),
                                span { class: "skill-result-stage", "Етап {output.stage:02}" }
                                span { class: "skill-result-text",
                                    span { class: "skill-result-title", "{output.title}" }
                                    span { class: "skill-result-preview", "{preview}" }
                                }
                                span { class: "design-status {output_status_tone(output.status)}",
                                    "{output_status_label(output.status)}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn OutputViewer(output: SkillOutput, on_close: EventHandler<()>) -> Element {
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let id = output.id;
    let busy = output.status.is_busy();

    let regenerate = move |_| {
        if pending() {
            return;
        }
        pending.set(true);
        error.set(None);
        spawn(async move {
            if let Err(problem) = bridge::regenerate_output(id).await {
                error.set(Some(explain(&problem)));
            }
            pending.set(false);
        });
    };

    rsx! {
        Modal { label: output.title.clone(), wide: true, on_close,
            header { class: "editor-head",
                div {
                    p { class: "mono-label", "Навичка · Етап {output.stage:02} · {output.skill_name}" }
                    h2 { class: "editor-title", "{output.title}" }
                }
                div { class: "editor-head-side",
                    span { class: "design-status {output_status_tone(output.status)}",
                        "{output_status_label(output.status)}"
                    }
                    button {
                        class: "icon-button",
                        r#type: "button",
                        aria_label: "Закрити",
                        onclick: move |_| on_close.call(()),
                        CloseIcon {}
                    }
                }
            }
            div { class: "editor-body",
                if let Some(problem) = output.error.as_ref() {
                    p { class: "alert", role: "alert", "{explain(problem)}" }
                }
                if let Some(message) = error() {
                    p { class: "alert", role: "alert", "{message}" }
                }
                match output.content.as_deref() {
                    Some(content) => rsx! {
                        pre { class: "skill-result-content", "{content}" }
                    },
                    None if busy => rsx! {
                        p { class: "skill-results-empty", "Claude виконує цю навичку…" }
                    },
                    None => rsx! {
                        p { class: "skill-results-empty", "Результату ще немає." }
                    },
                }
            }
            footer { class: "editor-foot",
                div { class: "editor-foot-actions",
                    button {
                        class: "primary-button",
                        r#type: "button",
                        disabled: busy || pending(),
                        onclick: regenerate,
                        if pending() {
                            span { class: "spinner" }
                        }
                        "Перегенерувати"
                    }
                }
            }
        }
    }
}

fn preview_line(content: &str) -> String {
    content
        .lines()
        .map(|line| line.trim_start_matches(['#', '-', '*', '>', ' ']).trim())
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

#[component]
fn StyleBar(style: StyleGuide) -> Element {
    if style.palette.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "style-bar",
            span { class: "mono-label", "Стиль гри" }
            div { class: "style-swatches",
                for (index, color) in style.palette.iter().enumerate() {
                    span {
                        key: "{index}",
                        class: "style-swatch",
                        title: "{color}",
                        style: "background:{color}",
                    }
                }
            }
            span { class: "style-summary", "{style.summary()}" }
        }
    }
}

#[component]
fn DesignCard(screen: Screen, on_open: EventHandler<i64>) -> Element {
    let id = screen.id;
    rsx! {
        article {
            class: "design-card",
            tabindex: "0",
            aria_label: "{screen.title} — відкрити екран",
            onclick: move |_| on_open.call(id),
            onkeydown: move |event| {
                if event.key() == Key::Enter {
                    on_open.call(id);
                }
            },
            ScreenPreview {
                html: screen.html.clone(),
                label: screen.title.clone(),
                busy: screen.status == ScreenStatus::Generating,
            }
            div { class: "design-card-body",
                div { class: "design-card-head",
                    h3 { class: "design-card-title", "{screen.title}" }
                    span { class: "design-status {screen_status_tone(screen.status)}",
                        "{screen_status_label(screen.status)}"
                    }
                }
                p { class: "design-card-summary", "{screen.summary}" }
            }
        }
    }
}

#[component]
fn ScreenViewer(
    project: Project,
    screen: Screen,
    on_close: EventHandler<()>,
    on_form: EventHandler<i64>,
) -> Element {
    let mut feedback = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let id = screen.id;
    let busy = screen.status.is_busy();
    let form_label = if project.status == ProjectStatus::Ready {
        "Зробити обкладинкою"
    } else {
        "Сформувати проєкт"
    };

    let mut act = move |action: ScreenAction| {
        if pending() {
            return;
        }
        let note = feedback();
        pending.set(true);
        error.set(None);
        spawn(async move {
            let outcome = match action {
                ScreenAction::Refine => bridge::refine_screen(id, &note).await,
                ScreenAction::Regenerate => bridge::regenerate_screen(id).await,
                ScreenAction::Approve => bridge::approve_screen(id).await,
            };
            match outcome {
                Ok(_) if action == ScreenAction::Refine => feedback.set(String::new()),
                Ok(_) => {}
                Err(problem) => error.set(Some(explain(&problem))),
            }
            pending.set(false);
        });
    };

    rsx! {
        Modal { label: screen.title.clone(), wide: true, on_close,
            header { class: "editor-head",
                div {
                    p { class: "mono-label", "{project.name} · Екран {screen.position:02} · Версія {screen.revision}" }
                    h2 { class: "editor-title", "{screen.title}" }
                }
                div { class: "editor-head-side",
                    span { class: "design-status {screen_status_tone(screen.status)}",
                        "{screen_status_label(screen.status)}"
                    }
                    button {
                        class: "icon-button",
                        r#type: "button",
                        aria_label: "Закрити",
                        onclick: move |_| on_close.call(()),
                        CloseIcon {}
                    }
                }
            }
            div { class: "editor-body viewer-body",
                ScreenPreview {
                    html: screen.html.clone(),
                    label: screen.title.clone(),
                    busy,
                    live: true,
                }
                p { class: "viewer-summary", "{screen.summary}" }
                if let Some(problem) = screen.error.as_ref() {
                    p { class: "alert", role: "alert", "{explain(problem)}" }
                }
                if let Some(message) = error() {
                    p { class: "alert", role: "alert", "{message}" }
                }
                form {
                    class: "viewer-feedback",
                    onsubmit: move |event| {
                        event.prevent_default();
                        act(ScreenAction::Refine);
                    },
                    label { class: "mono-label", r#for: "screen-feedback", "Промпт для правок" }
                    div { class: "viewer-feedback-row",
                        textarea {
                            class: "input is-area",
                            id: "screen-feedback",
                            rows: "2",
                            maxlength: "{MAX_FEEDBACK_CHARS}",
                            placeholder: "Наприклад: збільш мінікарту, а кнопки зроби теплішими",
                            disabled: busy,
                            value: "{feedback}",
                            oninput: move |event| feedback.set(event.value()),
                        }
                        button {
                            class: "primary-button",
                            r#type: "submit",
                            disabled: busy || pending() || feedback.read().trim().is_empty(),
                            "Надіслати правки"
                        }
                    }
                }
            }
            footer { class: "editor-foot",
                button {
                    class: "ghost-button",
                    r#type: "button",
                    disabled: busy || pending(),
                    onclick: move |_| act(ScreenAction::Regenerate),
                    "Перегенерувати"
                }
                div { class: "editor-foot-actions",
                    match screen.status {
                        ScreenStatus::Draft => rsx! {
                            button {
                                class: "primary-button",
                                r#type: "button",
                                disabled: pending(),
                                onclick: move |_| act(ScreenAction::Approve),
                                if pending() {
                                    span { class: "spinner" }
                                }
                                "Апрувнути"
                            }
                        },
                        ScreenStatus::Approved => rsx! {
                            button {
                                class: "primary-button",
                                r#type: "button",
                                onclick: move |_| on_form.call(id),
                                "{form_label}"
                            }
                        },
                        _ => rsx! {
                            span { class: "mono-label", "Claude працює над екраном…" }
                        },
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScreenAction {
    Refine,
    Regenerate,
    Approve,
}

#[component]
fn AddScreenDialog(project_id: i64, on_close: EventHandler<()>) -> Element {
    let mut title = use_signal(String::new);
    let mut summary = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let submit = move |event: FormEvent| {
        event.prevent_default();
        if pending() {
            return;
        }
        pending.set(true);
        error.set(None);
        spawn(async move {
            match bridge::add_screen(project_id, &title(), &summary()).await {
                Ok(_) => on_close.call(()),
                Err(problem) => {
                    error.set(Some(explain(&problem)));
                    pending.set(false);
                }
            }
        });
    };

    rsx! {
        Modal { label: "Новий екран".to_owned(), locked: pending(), on_close,
            form { novalidate: true, onsubmit: submit,
                header { class: "editor-head",
                    div {
                        p { class: "mono-label", "Claude Design · Новий екран" }
                        h2 { class: "editor-title", "Згенерувати екран" }
                    }
                    button {
                        class: "icon-button",
                        r#type: "button",
                        aria_label: "Закрити",
                        onclick: move |_| on_close.call(()),
                        CloseIcon {}
                    }
                }
                div { class: "editor-body",
                    div { class: "field",
                        label { class: "mono-label", r#for: "screen-title", "Назва екрана" }
                        input {
                            class: "input",
                            id: "screen-title",
                            r#type: "text",
                            placeholder: "Напр. Магазин",
                            maxlength: "{MAX_SCREEN_TITLE_CHARS}",
                            value: "{title}",
                            oninput: move |event| title.set(event.value()),
                            onmounted: move |event| async move {
                                let _ = event.data().set_focus(true).await;
                            },
                        }
                    }
                    div { class: "field",
                        label { class: "mono-label", r#for: "screen-summary", "Що на екрані" }
                        textarea {
                            class: "input is-area",
                            id: "screen-summary",
                            rows: "3",
                            placeholder: "Апгрейди відмичок, валюта, порівняння характеристик",
                            maxlength: "{MAX_SCREEN_SUMMARY_CHARS}",
                            value: "{summary}",
                            oninput: move |event| summary.set(event.value()),
                        }
                    }
                    if let Some(message) = error() {
                        p { class: "alert", role: "alert", "{message}" }
                    }
                }
                footer { class: "editor-foot",
                    div { class: "editor-foot-actions",
                        button {
                            class: "ghost-button",
                            r#type: "button",
                            disabled: pending(),
                            onclick: move |_| on_close.call(()),
                            "Скасувати"
                        }
                        button { class: "primary-button", r#type: "submit", disabled: pending(),
                            if pending() {
                                span { class: "spinner" }
                            }
                            "Згенерувати"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn FormProjectDialog(
    project: Project,
    candidates: Vec<Screen>,
    initial_cover: i64,
    on_close: EventHandler<()>,
    on_formed: EventHandler<Project>,
) -> Element {
    let mut name = use_signal(|| project.name.clone());
    let mut cover = use_signal(|| initial_cover);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let project_id = project.id;

    let submit = move |event: FormEvent| {
        event.prevent_default();
        if pending() {
            return;
        }
        pending.set(true);
        error.set(None);
        spawn(async move {
            match bridge::form_project(project_id, &name(), cover()).await {
                Ok(formed) => on_formed.call(formed),
                Err(problem) => {
                    error.set(Some(explain(&problem)));
                    pending.set(false);
                }
            }
        });
    };

    rsx! {
        Modal { label: "Формування проєкту".to_owned(), wide: true, locked: pending(), on_close,
            form { novalidate: true, onsubmit: submit,
                header { class: "editor-head",
                    div {
                        p { class: "mono-label", "Claude Design · Формування проєкту" }
                        h2 { class: "editor-title", "Проєкт гри" }
                    }
                    button {
                        class: "icon-button",
                        r#type: "button",
                        aria_label: "Закрити",
                        onclick: move |_| on_close.call(()),
                        CloseIcon {}
                    }
                }
                div { class: "editor-body",
                    div { class: "field",
                        div { class: "field-head",
                            label { class: "mono-label", r#for: "project-name", "Назва проєкту" }
                            span { class: "mono-label", "від Claude" }
                        }
                        input {
                            class: "input",
                            id: "project-name",
                            r#type: "text",
                            maxlength: "{MAX_PROJECT_NAME_CHARS}",
                            value: "{name}",
                            oninput: move |event| name.set(event.value()),
                        }
                    }
                    div { class: "field",
                        span { class: "mono-label", "Обкладинка — скрін апрувнутого екрана" }
                        div { class: "cover-grid", role: "radiogroup", aria_label: "Обкладинка",
                            for candidate in candidates {
                                {
                                    let candidate_id = candidate.id;
                                    let active = cover() == candidate_id;
                                    rsx! {
                                        button {
                                            key: "{candidate_id}",
                                            class: if active { "cover-option is-active" } else { "cover-option" },
                                            r#type: "button",
                                            role: "radio",
                                            aria_checked: if active { "true" } else { "false" },
                                            onclick: move |_| cover.set(candidate_id),
                                            ScreenPreview { html: candidate.html.clone(), label: candidate.title.clone() }
                                            span { class: "cover-option-title", "{candidate.title}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if let Some(message) = error() {
                        p { class: "alert", role: "alert", "{message}" }
                    }
                }
                footer { class: "editor-foot",
                    div { class: "editor-foot-actions",
                        button {
                            class: "ghost-button",
                            r#type: "button",
                            disabled: pending(),
                            onclick: move |_| on_close.call(()),
                            "Скасувати"
                        }
                        button { class: "primary-button", r#type: "submit", disabled: pending(),
                            if pending() {
                                span { class: "spinner" }
                            }
                            "Сформувати проєкт"
                        }
                    }
                }
            }
        }
    }
}
