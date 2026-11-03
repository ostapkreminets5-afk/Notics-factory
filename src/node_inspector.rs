use std::future::Future;

use dioxus::prelude::*;
use noctics_shared::{
    AssetTask, BuildOverview, BuildStatus, BuildTarget, GameEngine, MAX_PROMPT_CHARS, Medium, OutputStatus,
    PinterestStatus, ProjectDetail, Readiness, ScreenStatus, SourceOrigin, SourceStatus, StudioError,
    format_size,
};

use crate::build::explain_build;
use crate::build_bridge::{self, SOURCE_FOLDER};
use crate::bridge;
use crate::factory_mode::{FactoryMode, use_factory_mode};
use crate::icons::{CheckIcon, CloseIcon, ExternalIcon, PlusIcon, RefreshIcon};
use crate::node_graph::{
    Node, NodeId, Phase, Snapshot, build_phase, latest_build, output_phase, run_phase, screen_phase, stage_outputs,
    task_phase,
};
use crate::references::explain_pinterest;
use crate::studio::{Section, explain, output_status_label, screen_status_label, use_studio};

const ACTIVITY_LINES: usize = 14;
const BUILD_LOG_LINES: usize = 6;
const PIN_PREVIEWS: usize = 4;
const ASSET_PREVIEWS: usize = 6;

#[derive(Clone, Copy, PartialEq)]
struct Actions {
    busy: Signal<Option<String>>,
    notice: Signal<Option<String>>,
}

impl Actions {
    fn pending(self, key: &str) -> bool {
        self.busy.read().as_deref() == Some(key)
    }

    fn locked(self) -> bool {
        self.busy.read().is_some()
    }

    fn run(self, key: impl Into<String>, task: impl Future<Output = Result<(), String>> + 'static) {
        let mut busy = self.busy;
        let mut notice = self.notice;
        if busy.peek().is_some() {
            return;
        }
        busy.set(Some(key.into()));
        notice.set(None);
        spawn(async move {
            if let Err(text) = task.await {
                notice.set(Some(text));
            }
            busy.set(None);
        });
    }
}

#[component]
pub fn NodeInspector(
    node: Node,
    snapshot: Snapshot,
    draft_prompt: Signal<String>,
    on_close: EventHandler<()>,
    on_started: EventHandler<i64>,
    on_new: EventHandler<()>,
) -> Element {
    let actions = use_context_provider(|| Actions {
        busy: Signal::new(None),
        notice: Signal::new(None),
    });
    let notice = (actions.notice)();
    let phase = node.phase;
    let id = node.id;
    let project_id = snapshot.project_id();

    let panel = match (id, &snapshot.detail) {
        (NodeId::Prompt, _) => rsx! {
            PromptPanel { snapshot: snapshot.clone(), draft_prompt, on_started, on_new }
        },
        (_, None) => rsx! {
            p { class: "nf-inspector-lead",
                "Цей вузол оживе, щойно запустиш гру з вузла «Промпт гри»."
            }
        },
        (NodeId::Plan, Some(detail)) => rsx! {
            PlanPanel { detail: detail.clone() }
        },
        (NodeId::References, Some(detail)) => rsx! {
            ReferencesPanel { detail: detail.clone(), pinterest: snapshot.pinterest.clone() }
        },
        (NodeId::Foundation | NodeId::Skills, Some(detail)) => rsx! {
            SkillsPanel { detail: detail.clone(), id }
        },
        (NodeId::Screens, Some(detail)) => rsx! {
            ScreensPanel { detail: detail.clone() }
        },
        (NodeId::Code, Some(detail)) => rsx! {
            CodePanel { project_id: detail.project.id, overview: snapshot.overview.clone() }
        },
        (NodeId::Assets, Some(_)) => rsx! {
            AssetsPanel {
                tasks: snapshot.project_tasks().into_iter().cloned().collect::<Vec<AssetTask>>(),
                online: snapshot.comfy.as_ref().map(|comfy| comfy.status.online),
            }
        },
        (NodeId::Builds, Some(detail)) => rsx! {
            BuildPanel {
                project_id: detail.project.id,
                overview: snapshot.overview.clone(),
                targets: BuildTarget::ALL.to_vec(),
            }
        },
        (NodeId::Build(target), Some(detail)) => rsx! {
            BuildPanel {
                project_id: detail.project.id,
                overview: snapshot.overview.clone(),
                targets: vec![target],
            }
        },
    };

    rsx! {
        aside {
            class: "nf-inspector {node.tint.tint()}",
            aria_label: "Вузол «{node.title}»",
            onkeydown: move |event| {
                if event.key() == Key::Escape {
                    on_close.call(());
                }
            },
            header { class: "nf-inspector-head",
                div { class: "nf-inspector-heading",
                    h2 { class: "nf-inspector-title", "{node.title}" }
                    p { class: "nf-inspector-state",
                        span { class: "nf-led is-{phase.code()}" }
                        span { "{phase.label()}" }
                        span { class: "nf-node-system", "{node.system}" }
                    }
                }
                button {
                    class: "nf-inspector-close",
                    r#type: "button",
                    title: "Закрити (Esc)",
                    aria_label: "Закрити інспектор",
                    onclick: move |_| on_close.call(()),
                    CloseIcon {}
                }
            }
            div { class: "nf-inspector-body",
                if !node.summary.is_empty() && id != NodeId::Prompt {
                    p { class: "nf-inspector-lead", "{node.summary}" }
                }
                if let Some(text) = notice {
                    p { class: "nf-notice", role: "alert", "{text}" }
                }
                {panel}
                ClassicLink { id, project_id }
            }
        }
    }
}

#[component]
fn PromptPanel(
    snapshot: Snapshot,
    draft_prompt: Signal<String>,
    on_started: EventHandler<i64>,
    on_new: EventHandler<()>,
) -> Element {
    let studio = use_studio();
    let actions = use_context::<Actions>();

    if let Some(detail) = &snapshot.detail {
        let engine = detail.project.engine;
        let planned = snapshot
            .overview
            .as_ref()
            .map(|overview| overview.planned.iter().map(|target| target.label()).collect::<Vec<_>>())
            .filter(|labels| !labels.is_empty())
            .map_or_else(|| "не обрано".to_owned(), |labels| labels.join(", "));
        return rsx! {
            section { class: "nf-section",
                h3 { class: "nf-section-title", "Промпт" }
                p { class: "nf-text", "{detail.project.prompt}" }
            }
            dl { class: "nf-facts",
                dt { "Рушій" }
                dd { "{engine.label()}, {engine.stack()}" }
                dt { "Платформи" }
                dd { "{planned}" }
            }
            div { class: "nf-actions",
                button {
                    class: "nf-button is-primary",
                    r#type: "button",
                    onclick: move |_| on_new.call(()),
                    PlusIcon {}
                    "Нова гра"
                }
            }
        };
    }

    let mut prompt = draft_prompt;
    let length = prompt.read().chars().count();
    let engine = (studio.draft_engine)();
    let empty = prompt.read().trim().is_empty();
    let launching = actions.pending("launch");
    let launch = move || {
        let text = prompt.peek().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let targets = studio.build_plan.peek().clone();
        let engine = *studio.draft_engine.peek();
        actions.run("launch", async move {
            match bridge::start_project(&text, engine, &targets).await {
                Ok(project) => {
                    prompt.set(String::new());
                    studio.changed();
                    on_started.call(project.id);
                    Ok(())
                }
                Err(problem) => Err(explain_launch(&problem)),
            }
        });
    };

    rsx! {
        form {
            class: "nf-section",
            novalidate: true,
            onsubmit: move |event| {
                event.prevent_default();
                launch();
            },
            div { class: "nf-field",
                label { class: "nf-label", r#for: "nf-prompt",
                    span { "Опиши гру" }
                    span { class: "nf-label-count", "{length} / {MAX_PROMPT_CHARS}" }
                }
                textarea {
                    class: "nf-textarea",
                    id: "nf-prompt",
                    maxlength: "{MAX_PROMPT_CHARS}",
                    placeholder: "Стелс про злодія, який краде світло з ліхтарів нічного міста, щоб урятувати сестру",
                    value: "{prompt}",
                    oninput: move |event| prompt.set(event.value()),
                    onkeydown: move |event| {
                        if event.key() == Key::Enter && event.modifiers().ctrl() {
                            event.prevent_default();
                            launch();
                        }
                    },
                }
            }
            div { class: "nf-field",
                span { class: "nf-label", "Рушій" }
                div { class: "nf-toggles", role: "radiogroup", aria_label: "Рушій гри",
                    for choice in GameEngine::ALL {
                        button {
                            key: "{choice.code()}",
                            class: if choice == engine { "nf-toggle is-on" } else { "nf-toggle" },
                            r#type: "button",
                            role: "radio",
                            aria_checked: if choice == engine { "true" } else { "false" },
                            title: "{choice.stack()}",
                            onclick: move |_| studio.choose_draft_engine(choice),
                            "{choice.label()}"
                        }
                    }
                }
            }
            div { class: "nf-field",
                span { class: "nf-label", "Платформи білду" }
                div { class: "nf-toggles", role: "group", aria_label: "Платформи білду",
                    for target in BuildTarget::ALL {
                        button {
                            key: "{target.code()}",
                            class: if studio.plans_build(target) { "nf-toggle is-on" } else { "nf-toggle" },
                            r#type: "button",
                            aria_pressed: if studio.plans_build(target) { "true" } else { "false" },
                            title: "{target.package()}",
                            onclick: move |_| studio.toggle_build_target(target),
                            "{target.label()}"
                        }
                    }
                }
            }
            div { class: "nf-actions",
                button {
                    class: "nf-button is-primary",
                    r#type: "submit",
                    disabled: empty || actions.locked(),
                    if launching {
                        span { class: "nf-spinner" }
                        "Запускаємо"
                    } else {
                        "Запустити фабрику"
                    }
                }
            }
            p { class: "nf-inspector-lead",
                "Ctrl+Enter теж запускає. Далі все піде по кабелях: план, референси, навички, екрани, код, ассети й білди."
            }
        }
    }
}

fn explain_launch(problem: &StudioError) -> String {
    match problem {
        StudioError::ClaudeCodeMissing | StudioError::ClaudeSignedOut => format!(
            "{} Налаштування відкриваються в класичному режимі.",
            explain(problem)
        ),
        other => explain(other),
    }
}

#[component]
fn PlanPanel(detail: ProjectDetail) -> Element {
    let project = &detail.project;
    if !matches!(project.status, noctics_shared::ProjectStatus::Designing | noctics_shared::ProjectStatus::Ready) {
        return rsx! {
            section { class: "nf-section",
                h3 { class: "nf-section-title", "Промпт, з якого Claude планує" }
                p { class: "nf-text", "{project.prompt}" }
            }
        };
    }
    let style = &project.style;

    rsx! {
        section { class: "nf-section",
            p { class: "nf-game-name", "{project.name}" }
            if !project.pitch.is_empty() {
                p { class: "nf-text", "{project.pitch}" }
            }
        }
        if !style.palette.is_empty() {
            section { class: "nf-section",
                h3 { class: "nf-section-title", "Палітра" }
                div { class: "nf-palette",
                    for color in style.palette.iter() {
                        span { key: "{color}", class: "nf-palette-chip",
                            span { class: "nf-palette-color", style: "background: {color};" }
                            "{color}"
                        }
                    }
                }
            }
        }
        dl { class: "nf-facts",
            dt { "Шрифти" }
            dd { "{style.fonts}" }
            dt { "Кути" }
            dd { "{style.corner_radius} px" }
            dt { "Настрій" }
            dd { "{style.mood}" }
            dt { "Екрани" }
            dd { "{detail.screens.len()}" }
        }
        if !detail.design_document.trim().is_empty() {
            section { class: "nf-section",
                h3 { class: "nf-section-title", "Дизайн-документ" }
                div { class: "nf-document", "{detail.design_document}" }
            }
        }
    }
}

#[component]
fn ReferencesPanel(detail: ProjectDetail, pinterest: Option<PinterestStatus>) -> Element {
    let studio = use_studio();
    let actions = use_context::<Actions>();
    let project_id = detail.project.id;
    let signed_out = pinterest.as_ref().is_some_and(|status| status.signed_in == Some(false));

    rsx! {
        div { class: "nf-actions",
            button {
                class: "nf-button",
                r#type: "button",
                disabled: actions.locked(),
                onclick: move |_| {
                    actions.run("pick", async move {
                        bridge::pick_references(project_id).await.map_err(|problem| explain(&problem))?;
                        studio.changed();
                        Ok(())
                    })
                },
                if actions.pending("pick") {
                    span { class: "nf-spinner" }
                } else {
                    RefreshIcon {}
                }
                "Підібрати заново"
            }
            button {
                class: "nf-button is-quiet",
                r#type: "button",
                disabled: actions.locked() || signed_out,
                onclick: move |_| {
                    actions.run("pins", async move {
                        bridge::load_reference_pins(project_id)
                            .await
                            .map_err(|problem| explain_pinterest(&problem))?;
                        studio.changed();
                        Ok(())
                    })
                },
                "Підтягнути піни"
            }
        }
        if signed_out {
            p { class: "nf-inspector-lead",
                "Pinterest не підключено. Увійди в розділі Pinterest класичного режиму — і піни підтягнуться."
            }
        }
        div { class: "nf-items",
            for (index, reference) in detail.project.references.iter().enumerate() {
                div { key: "{index}", class: "nf-item",
                    div { class: "nf-item-head",
                        span { class: if reference.pins.is_empty() && !reference.topic.is_audio() { "nf-led" } else { "nf-led is-done" } }
                        span { class: "nf-item-title", title: "{reference.title}", "{reference.title}" }
                        span { class: "nf-item-tag", "{reference.topic.label()}" }
                    }
                    if !reference.note.is_empty() {
                        p { class: "nf-item-note", "{reference.note}" }
                    }
                    if !reference.pins.is_empty() {
                        div { class: "nf-pins",
                            for pin in reference.pins.iter().take(PIN_PREVIEWS) {
                                img {
                                    key: "{pin.id}{pin.image}",
                                    src: "{pin.image}",
                                    alt: "{pin.title}",
                                    draggable: "false",
                                    "loading": "lazy",
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
fn SkillsPanel(detail: ProjectDetail, id: NodeId) -> Element {
    let studio = use_studio();
    let actions = use_context::<Actions>();
    let project_id = detail.project.id;
    let outputs = stage_outputs(&detail, id);

    rsx! {
        div { class: "nf-actions",
            button {
                class: "nf-button is-quiet",
                r#type: "button",
                title: "Додати в проєкт навички, які увімкнули після планування",
                disabled: actions.locked(),
                onclick: move |_| {
                    actions.run("sync", async move {
                        bridge::sync_project(project_id).await.map_err(|problem| explain(&problem))?;
                        studio.changed();
                        Ok(())
                    })
                },
                if actions.pending("sync") {
                    span { class: "nf-spinner" }
                } else {
                    RefreshIcon {}
                }
                "Синхронізувати навички"
            }
        }
        if outputs.is_empty() {
            p { class: "nf-inspector-lead",
                "Тут немає жодної навички. Увімкни або створи її в SKILLS і натисни «Синхронізувати навички»."
            }
        }
        div { class: "nf-items",
            for output in outputs {
                div { key: "{output.id}", class: "nf-item",
                    div { class: "nf-item-head",
                        span { class: "nf-led is-{output_phase(output.status).code()}" }
                        span { class: "nf-item-tag", "{output.stage:02}" }
                        span { class: "nf-item-title", title: "{output.title}", "{output.title}" }
                        span { class: "nf-item-status", "{output_status_label(output.status)}" }
                    }
                    if let Some(problem) = &output.error {
                        p { class: "nf-item-note is-failed", "{explain(problem)}" }
                    }
                    if let Some(content) = output.content.as_ref().filter(|text| !text.trim().is_empty()) {
                        details {
                            summary { "Показати документ" }
                            div { class: "nf-document", "{content}" }
                        }
                    }
                    if matches!(output.status, OutputStatus::Done | OutputStatus::Failed) {
                        div { class: "nf-actions",
                            button {
                                class: "nf-button is-quiet",
                                r#type: "button",
                                disabled: actions.locked(),
                                onclick: {
                                    let output_id = output.id;
                                    move |_| {
                                        actions.run(format!("output-{output_id}"), async move {
                                            bridge::regenerate_output(output_id)
                                                .await
                                                .map_err(|problem| explain(&problem))?;
                                            studio.changed();
                                            Ok(())
                                        })
                                    }
                                },
                                if actions.pending(&format!("output-{}", output.id)) {
                                    span { class: "nf-spinner" }
                                } else {
                                    RefreshIcon {}
                                }
                                "Перегенерувати"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ScreensPanel(detail: ProjectDetail) -> Element {
    let studio = use_studio();
    let actions = use_context::<Actions>();

    rsx! {
        div { class: "nf-items",
            for screen in detail.screens.iter() {
                div { key: "{screen.id}", class: "nf-item",
                    div { class: "nf-item-head",
                        span { class: "nf-led is-{screen_phase(screen.status).code()}" }
                        span { class: "nf-item-title", title: "{screen.title}", "{screen.title}" }
                        span { class: "nf-item-status", "{screen_status_label(screen.status)}" }
                    }
                    if let Some(html) = &screen.html {
                        div { class: "nf-preview",
                            iframe {
                                title: "{screen.title}",
                                tabindex: "-1",
                                "sandbox": "",
                                "scrolling": "no",
                                "srcdoc": "{html}",
                            }
                        }
                    }
                    if !screen.summary.is_empty() {
                        p { class: "nf-item-note", "{screen.summary}" }
                    }
                    if let Some(problem) = &screen.error {
                        p { class: "nf-item-note is-failed", "{explain(problem)}" }
                    }
                    div { class: "nf-actions",
                        if screen.status == ScreenStatus::Draft {
                            button {
                                class: "nf-button is-primary",
                                r#type: "button",
                                disabled: actions.locked(),
                                onclick: {
                                    let screen_id = screen.id;
                                    move |_| {
                                        actions.run(format!("approve-{screen_id}"), async move {
                                            bridge::approve_screen(screen_id)
                                                .await
                                                .map_err(|problem| explain(&problem))?;
                                            studio.changed();
                                            Ok(())
                                        })
                                    }
                                },
                                CheckIcon {}
                                "Апрувнути"
                            }
                        }
                        if matches!(screen.status, ScreenStatus::Draft | ScreenStatus::Approved | ScreenStatus::Failed) {
                            button {
                                class: "nf-button is-quiet",
                                r#type: "button",
                                disabled: actions.locked(),
                                onclick: {
                                    let screen_id = screen.id;
                                    move |_| {
                                        actions.run(format!("redo-{screen_id}"), async move {
                                            bridge::regenerate_screen(screen_id)
                                                .await
                                                .map_err(|problem| explain(&problem))?;
                                            studio.changed();
                                            Ok(())
                                        })
                                    }
                                },
                                if actions.pending(&format!("redo-{}", screen.id)) {
                                    span { class: "nf-spinner" }
                                } else {
                                    RefreshIcon {}
                                }
                                "Перегенерувати"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn CodePanel(project_id: i64, overview: Option<BuildOverview>) -> Element {
    let actions = use_context::<Actions>();
    let Some(overview) = overview else {
        return rsx! {
            p { class: "nf-inspector-lead", "Завантажую стан збірки…" }
        };
    };
    let source = overview.source.clone();
    let generating = source
        .as_ref()
        .is_some_and(|source| source.status == SourceStatus::Generating);
    let activity: Vec<String> = source
        .as_ref()
        .map(|source| {
            source
                .activity
                .iter()
                .rev()
                .take(ACTIVITY_LINES)
                .rev()
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    rsx! {
        dl { class: "nf-facts",
            dt { "Рушій" }
            dd { "{overview.engine.label()}" }
            if let Some(source) = &source {
                dt { "Папка" }
                dd { "{source.path}" }
                dt { "Джерело" }
                dd {
                    match source.origin {
                        SourceOrigin::Generated => "Написав Claude-агент",
                        SourceOrigin::Linked => "Підключена папка",
                    }
                }
            }
        }
        if !activity.is_empty() {
            section { class: "nf-section",
                h3 { class: "nf-section-title", "Хід роботи агента" }
                ul { class: "nf-log",
                    for (index, line) in activity.into_iter().enumerate() {
                        li { key: "{index}", title: "{line}", "{line}" }
                    }
                }
            }
        }
        if let Some(problem) = source.as_ref().and_then(|source| source.error.clone()) {
            p { class: "nf-item-note is-failed", "{problem}" }
        }
        div { class: "nf-actions",
            if generating {
                button {
                    class: "nf-button is-danger",
                    r#type: "button",
                    disabled: actions.locked(),
                    onclick: move |_| {
                        actions.run("stop", async move {
                            build_bridge::stop_generation(project_id)
                                .await
                                .map_err(|problem| explain_build(&problem))
                        })
                    },
                    "Зупинити агента"
                }
            } else {
                button {
                    class: "nf-button is-primary",
                    r#type: "button",
                    disabled: actions.locked(),
                    onclick: move |_| {
                        actions.run("generate", async move {
                            build_bridge::generate(project_id, None)
                                .await
                                .map(|_| ())
                                .map_err(|problem| explain_build(&problem))
                        })
                    },
                    if actions.pending("generate") {
                        span { class: "nf-spinner" }
                    }
                    if source.is_some() { "Перегенерувати код" } else { "Згенерувати код" }
                }
            }
            if source.is_some() {
                button {
                    class: "nf-button is-quiet",
                    r#type: "button",
                    onclick: move |_| {
                        actions.run("folder", async move {
                            build_bridge::open_folder(project_id, SOURCE_FOLDER)
                                .await
                                .map_err(|problem| explain_build(&problem))
                        })
                    },
                    ExternalIcon {}
                    "Відкрити папку"
                }
            }
        }
    }
}

#[component]
fn AssetsPanel(tasks: Vec<AssetTask>, online: Option<bool>) -> Element {
    rsx! {
        if online == Some(false) {
            p { class: "nf-inspector-lead",
                "ComfyUI зараз вимкнено. Задачі чекають у черзі й продовжаться, щойно він запуститься."
            }
        }
        if tasks.is_empty() {
            p { class: "nf-inspector-lead",
                "Поки що жодного ассета. Claude-агент замовляє арт, звук і 3D через ComfyUI, коли пише код гри."
            }
        }
        div { class: "nf-items",
            for task in tasks.iter() {
                div { key: "{task.id}", class: "nf-item",
                    div { class: "nf-item-head",
                        span { class: "nf-led is-{task_phase(task.status).code()}" }
                        span { class: "nf-item-title", title: "{task.title}", "{task.title}" }
                        span { class: "nf-item-tag", "{task.kind.label()}" }
                    }
                    if !task.runs.is_empty() {
                        ul { class: "nf-rows",
                            for run in task.runs.iter() {
                                li { key: "{run.id}", class: "nf-row",
                                    span { class: "nf-led is-{run_phase(run.status).code()}" }
                                    span { class: "nf-row-text", "{run.model_name}" }
                                    if let Some(seconds) = run.seconds() {
                                        span { class: "nf-row-tag", "{seconds} с" }
                                    }
                                }
                            }
                        }
                    }
                    if task.runs.iter().any(|run| run.files.iter().any(|file| file.medium == Medium::Image)) {
                        div { class: "nf-gallery",
                            for (index, file) in task
                                .runs
                                .iter()
                                .flat_map(|run| run.files.iter())
                                .filter(|file| file.medium == Medium::Image)
                                .take(ASSET_PREVIEWS)
                                .enumerate()
                            {
                                img {
                                    key: "{index}",
                                    src: "{file.url}",
                                    alt: "{task.title}",
                                    draggable: "false",
                                    "loading": "lazy",
                                }
                            }
                        }
                    }
                    if let Some(note) = &task.note {
                        p { class: "nf-item-note", "{note}" }
                    }
                }
            }
        }
    }
}

#[component]
fn BuildPanel(project_id: i64, overview: Option<BuildOverview>, targets: Vec<BuildTarget>) -> Element {
    let actions = use_context::<Actions>();
    let Some(overview) = overview else {
        return rsx! {
            p { class: "nf-inspector-lead", "Завантажую стан збірки…" }
        };
    };
    let can_build = overview.can_build();

    rsx! {
        if !can_build {
            p { class: "nf-inspector-lead",
                "Збирати можна, коли код гри готовий. Він з’явиться у вузлі «Код гри»."
            }
        }
        div { class: "nf-items",
            for target in targets {
                {
                    let readiness = overview.readiness(target);
                    let build = latest_build(&overview, target).cloned();
                    let phase = build.as_ref().map_or(Phase::Idle, |build| build_phase(build.status));
                    let active = build.as_ref().is_some_and(|build| build.status.is_active());
                    let ready = readiness.is_ready();
                    let readiness_text = match &readiness {
                        Readiness::Ready(note) => note.clone(),
                        Readiness::Missing(missing) => format!("Бракує: {missing}"),
                        Readiness::NeedsHost(note) => note.clone(),
                        Readiness::Unsupported => "Рушій не збирає під цю платформу".to_owned(),
                    };
                    let log: Vec<String> = build
                        .as_ref()
                        .map(|build| {
                            let lines: Vec<&str> = build.log_tail.lines().filter(|line| !line.trim().is_empty()).collect();
                            let skip = lines.len().saturating_sub(BUILD_LOG_LINES);
                            lines.into_iter().skip(skip).map(str::to_owned).collect()
                        })
                        .unwrap_or_default();
                    let start_key = format!("start-{}", target.code());
                    rsx! {
                        div { key: "{target.code()}", class: "nf-item",
                            div { class: "nf-item-head",
                                span { class: "nf-led is-{phase.code()}" }
                                span { class: "nf-item-title", "Білд {target.label()}" }
                                span { class: "nf-item-tag", "{target.package()}" }
                            }
                            p { class: if ready { "nf-item-note" } else { "nf-item-note is-failed" }, "{readiness_text}" }
                            if let Some(build) = &build {
                                p { class: "nf-item-note",
                                    "{phase.label()}"
                                    if let Some(size) = build.size_bytes {
                                        ", {format_size(size)}"
                                    }
                                }
                                if let Some(problem) = &build.error {
                                    p { class: "nf-item-note is-failed", "{problem}" }
                                }
                                if !log.is_empty() {
                                    ul { class: "nf-log",
                                        for (index, line) in log.into_iter().enumerate() {
                                            li { key: "{index}", title: "{line}", "{line}" }
                                        }
                                    }
                                }
                            }
                            div { class: "nf-actions",
                                if active {
                                    if let Some(build_id) = build.as_ref().map(|build| build.id) {
                                        button {
                                            class: "nf-button is-danger",
                                            r#type: "button",
                                            disabled: actions.locked(),
                                            onclick: move |_| {
                                                actions.run("cancel", async move {
                                                    build_bridge::cancel(build_id).await.map_err(|problem| explain_build(&problem))
                                                })
                                            },
                                            "Скасувати"
                                        }
                                    }
                                } else {
                                    button {
                                        class: "nf-button is-primary",
                                        r#type: "button",
                                        disabled: actions.locked() || !can_build || !ready,
                                        onclick: {
                                            let key = start_key.clone();
                                            move |_| {
                                                actions.run(key.clone(), async move {
                                                    build_bridge::start(project_id, &[target])
                                                        .await
                                                        .map(|_| ())
                                                        .map_err(|problem| explain_build(&problem))
                                                })
                                            }
                                        },
                                        if actions.pending(&start_key) {
                                            span { class: "nf-spinner" }
                                        }
                                        if build.is_some() { "Зібрати знову" } else { "Зібрати" }
                                    }
                                }
                                if let Some(build) = build.as_ref().filter(|build| build.status == BuildStatus::Failed) {
                                    button {
                                        class: "nf-button",
                                        r#type: "button",
                                        title: "Агент прочитає лог падіння й виправить код",
                                        disabled: actions.locked(),
                                        onclick: {
                                            let build_id = build.id;
                                            move |_| {
                                                actions.run("fix", async move {
                                                    build_bridge::fix(build_id).await.map_err(|problem| explain_build(&problem))
                                                })
                                            }
                                        },
                                        "Виправити збірку"
                                    }
                                }
                                if let Some(build) = build.as_ref().filter(|build| build.status == BuildStatus::Done) {
                                    button {
                                        class: "nf-button is-quiet",
                                        r#type: "button",
                                        onclick: {
                                            let build_id = build.id;
                                            move |_| {
                                                actions.run("reveal", async move {
                                                    build_bridge::reveal(build_id).await.map_err(|problem| explain_build(&problem))
                                                })
                                            }
                                        },
                                        ExternalIcon {}
                                        "Показати файл"
                                    }
                                }
                                if let Some(build) = &build {
                                    button {
                                        class: "nf-button is-quiet",
                                        r#type: "button",
                                        onclick: {
                                            let build_id = build.id;
                                            move |_| {
                                                actions.run("log", async move {
                                                    build_bridge::open_log(build_id).await.map_err(|problem| explain_build(&problem))
                                                })
                                            }
                                        },
                                        "Лог"
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

#[component]
fn ClassicLink(id: NodeId, project_id: Option<i64>) -> Element {
    let studio = use_studio();
    let mode = use_factory_mode();
    let (section, label) = match id {
        NodeId::Prompt => (Section::Pipeline, "Відкрити «Конвеєр гри»"),
        NodeId::Plan | NodeId::Screens | NodeId::Foundation | NodeId::Skills => {
            (Section::ClaudeDesign, "Відкрити в Claude Design")
        }
        NodeId::References => (Section::Pinterest, "Відкрити Pinterest"),
        NodeId::Code | NodeId::Builds | NodeId::Build(_) => (Section::Build, "Відкрити «Збірку»"),
        NodeId::Assets => (Section::ComfyUi, "Відкрити ComfyUI"),
    };

    rsx! {
        div { class: "nf-actions",
            button {
                class: "nf-button is-quiet",
                r#type: "button",
                title: "Перемкнутись у класичний режим на цій сторінці",
                onclick: move |_| {
                    mode.set(FactoryMode::Classic);
                    match (section, project_id) {
                        (Section::ClaudeDesign, Some(project)) => studio.open_project(project),
                        _ => studio.open(section),
                    }
                },
                ExternalIcon {}
                "{label}"
            }
        }
    }
}
