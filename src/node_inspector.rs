use std::future::Future;

use dioxus::prelude::*;
use noctics_shared::{
    AssetTask, BuildOverview, BuildStatus, BuildTarget, DesignVariation, GameEngine, MAX_PROMPT_CHARS, Medium,
    MotionOrder, OutputStatus,
    PinterestStatus, ProjectDetail, Readiness, Screen, ScreenStatus, SourceOrigin, SourceStatus, StudioError,
    format_size,
};

use crate::asset_hunt::FindsPanel;
use crate::mixamo::AnimationsPanel;
use crate::build::explain_build;
use crate::build_bridge::{self, PLAYTEST_FOLDER, SOURCE_FOLDER};
use crate::bridge;
use crate::factory_mode::{FactoryMode, use_factory_mode};
use crate::icons::{ArrowRightIcon, CheckIcon, CloseIcon, ExpandIcon, ExternalIcon, PlusIcon, RefreshIcon};
use crate::inspector_layout::{InspectorResizer, use_inspector_layout};
use crate::journal::plural;
use crate::node_graph::{
    Node, NodeId, Phase, Snapshot, build_phase, latest_build, output_phase, run_phase, screen_phase, stage_outputs,
    task_phase,
};
use crate::artstation_bridge;
use crate::open_design::{OpenDesignPanel, VariationBoard, variation_slides};
use crate::references::{explain_artstation, explain_pinterest};
use crate::screen_gallery::{ScreenGallery, Slide};
use crate::studio::{Section, explain, output_status_label, screen_status_label, use_studio};

const ACTIVITY_LINES: usize = 14;
const BUILD_LOG_LINES: usize = 6;
const PIN_PREVIEWS: usize = 4;
const ASSET_PREVIEWS: usize = 6;
const MOSAIC_TILES: usize = 6;
const SCREEN_WORDS: [&str; 3] = ["екран", "екрани", "екранів"];

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
    let layout = use_inspector_layout();
    let mut viewing = use_signal(|| None::<i64>);
    let mut board = use_signal(|| false);
    let mut variation_open = use_signal(|| None::<usize>);
    let mut variant_viewing = use_signal(|| None::<i64>);
    let notice = (actions.notice)();
    let phase = node.phase;
    let id = node.id;
    let project_id = snapshot.project_id();
    let resizing = if layout.resizing() { " is-resizing" } else { "" };
    let gallery = viewing().zip(
        snapshot
            .detail
            .as_ref()
            .map(|detail| detail.screens.iter().map(Slide::from_screen).collect::<Vec<Slide>>()),
    );
    let board_detail = snapshot.detail.clone().filter(|_| board() && id == NodeId::Screens);
    let plan_screens: Vec<Screen> = snapshot
        .detail
        .as_ref()
        .map(|detail| detail.screens.clone())
        .unwrap_or_default();
    let opened_variation: Option<DesignVariation> = variation_open()
        .filter(|_| id == NodeId::OpenDesign)
        .zip(snapshot.open_design.as_ref())
        .and_then(|(number, design)| {
            design
                .variations()
                .into_iter()
                .find(|variation| variation.number == number)
        });
    let variant_gallery = variant_viewing().zip(
        opened_variation
            .as_ref()
            .map(|variation| variation_slides(variation, &plan_screens)),
    );

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
            ScreensPanel { detail: detail.clone(), board }
        },
        (NodeId::Code, Some(detail)) => rsx! {
            CodePanel { project_id: detail.project.id, overview: snapshot.overview.clone() }
        },
        (NodeId::Finds, Some(detail)) => rsx! {
            FindsPanel { project_id: detail.project.id, hunt: snapshot.hunt.clone() }
        },
        (NodeId::Animations, Some(detail)) => rsx! {
            AnimationsPanel {
                project_id: detail.project.id,
                engine: detail.project.engine,
                orders: snapshot.project_motions().into_iter().cloned().collect::<Vec<MotionOrder>>(),
                status: snapshot.mixamo.as_ref().map(|mixamo| mixamo.status.clone()).unwrap_or_default(),
            }
        },
        (NodeId::OpenDesign, Some(detail)) => rsx! {
            OpenDesignPanel {
                project_id: detail.project.id,
                board: snapshot.open_design.clone(),
                opened: variation_open,
            }
        },
        (NodeId::Assets, Some(_)) => rsx! {
            AssetsPanel {
                tasks: snapshot.project_tasks().into_iter().cloned().collect::<Vec<AssetTask>>(),
                online: snapshot.comfy.as_ref().map(|comfy| comfy.status.online),
            }
        },
        (NodeId::Playtest, Some(detail)) => rsx! {
            PlaytestPanel { project_id: detail.project.id, overview: snapshot.overview.clone() }
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
            class: "nf-inspector {node.tint.tint()}{resizing}",
            aria_label: "Вузол «{node.title}»",
            onkeydown: move |event| {
                if event.key() == Key::Escape {
                    on_close.call(());
                }
            },
            InspectorResizer {}
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
        if let Some(detail) = board_detail {
            ScreenBoard { detail, viewing, on_close: move |_| board.set(false) }
        }
        if let Some(variation) = opened_variation {
            VariationBoard {
                project_id: project_id.unwrap_or_default(),
                variation,
                screens: plan_screens.clone(),
                on_view: move |slide_id: i64| variant_viewing.set(Some(slide_id)),
                on_close: move |_| {
                    variant_viewing.set(None);
                    variation_open.set(None);
                },
            }
        }
        if let Some((current, slides)) = variant_gallery {
            ScreenGallery {
                slides,
                current,
                on_pick: move |slide_id: i64| variant_viewing.set(Some(slide_id)),
                on_close: move |_| variant_viewing.set(None),
            }
        }
        if let Some((current, slides)) = gallery {
            ScreenGallery {
                slides,
                current,
                on_pick: move |screen_id: i64| viewing.set(Some(screen_id)),
                on_close: move |_| viewing.set(None),
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
    let studio = use_studio();
    let actions = use_context::<Actions>();
    let project_id = project.id;
    let explained: Vec<(String, String)> = style
        .palette
        .iter()
        .enumerate()
        .filter_map(|(index, color)| style.palette_note(index).map(|note| (color.clone(), note.to_owned())))
        .collect();

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
                    for (index, color) in style.palette.iter().enumerate() {
                        span {
                            key: "{index}",
                            class: "nf-palette-chip",
                            title: style.palette_note(index).unwrap_or(color.as_str()),
                            span { class: "nf-palette-color", style: "background: {color};" }
                            "{color}"
                        }
                    }
                }
                if !explained.is_empty() {
                    ul { class: "nf-palette-legend", aria_label: "Звідки кожен колір",
                        for (index, (color, note)) in explained.into_iter().enumerate() {
                            li { key: "{index}",
                                span { class: "nf-palette-dot", style: "background: {color};" }
                                span { "{note}" }
                            }
                        }
                    }
                }
            }
        }
        if !style.look.trim().is_empty() {
            section { class: "nf-section",
                h3 { class: "nf-section-title", "Вигляд світу" }
                p { class: "nf-text", "{style.look}" }
            }
        }
        div { class: "nf-actions",
            button {
                class: "nf-button",
                r#type: "button",
                title: "Claude заново виведе палітру, шрифти й вигляд зі світу гри, а Claude Design перемалює всі екрани",
                disabled: actions.locked(),
                onclick: move |_| {
                    actions.run("restyle", async move {
                        bridge::restyle_project(project_id).await.map_err(|problem| explain(&problem))?;
                        studio.changed();
                        Ok(())
                    })
                },
                if actions.pending("restyle") {
                    span { class: "nf-spinner" }
                } else {
                    RefreshIcon {}
                }
                "Новий стиль і екрани"
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
            button {
                class: "nf-button is-quiet",
                r#type: "button",
                disabled: actions.locked(),
                onclick: move |_| {
                    actions.run("artworks", async move {
                        artstation_bridge::load_reference_artworks(project_id)
                            .await
                            .map_err(|problem| explain_artstation(&problem))?;
                        studio.changed();
                        Ok(())
                    })
                },
                "Підтягнути ArtStation"
            }
        }
        if signed_out {
            p { class: "nf-inspector-lead",
                "Pinterest не підключено. Увійди в розділі Pinterest класичного режиму — і піни підтягнуться. ArtStation працює без входу."
            }
        }
        div { class: "nf-items",
            for (index, reference) in detail.project.references.iter().enumerate() {
                div { key: "{index}", class: "nf-item",
                    div { class: "nf-item-head",
                        span { class: if reference.pins.is_empty() && reference.artworks.is_empty() && !reference.topic.is_audio() { "nf-led" } else { "nf-led is-done" } }
                        span { class: "nf-item-title", title: "{reference.title}", "{reference.title}" }
                        span { class: "nf-item-tag", "{reference.topic.label()}" }
                    }
                    if !reference.note.is_empty() {
                        p { class: "nf-item-note", "{reference.note}" }
                    }
                    if !reference.pins.is_empty() || !reference.artworks.is_empty() {
                        div { class: "nf-pins",
                            for (image, title) in reference
                                .pins
                                .iter()
                                .map(|pin| (pin.image.clone(), pin.title.clone()))
                                .chain(reference.artworks.iter().map(|artwork| (artwork.image.clone(), artwork.title.clone())))
                                .take(PIN_PREVIEWS)
                            {
                                img {
                                    key: "{image}",
                                    src: "{image}",
                                    alt: "{title}",
                                    draggable: "false",
                                    referrerpolicy: "no-referrer",
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
fn ScreensPanel(detail: ProjectDetail, board: Signal<bool>) -> Element {
    let count = detail.screens.len();
    if count == 0 {
        return rsx! {};
    }
    let waiting = detail
        .screens
        .iter()
        .filter(|screen| screen.status == ScreenStatus::Draft)
        .count();
    let hidden = count.saturating_sub(MOSAIC_TILES);
    let more = format!("+{}", hidden + 1);
    let shown: Vec<Screen> = detail.screens.iter().take(MOSAIC_TILES).cloned().collect();
    let total = plural(count as u32, SCREEN_WORDS);
    let hint = if waiting > 0 {
        format!("{total} · {waiting} чекають апруву")
    } else {
        format!("{total} · сіткою по порядку")
    };

    rsx! {
        button {
            class: "nf-screens-link",
            r#type: "button",
            title: "Відкрити всі екрани гри сіткою",
            onclick: move |_| board.set(true),
            div { class: "nf-screens-mosaic",
                for (at, screen) in shown.iter().enumerate() {
                    div { key: "{screen.id}", class: "nf-screens-thumb",
                        if let Some(html) = &screen.html {
                            iframe {
                                title: "{screen.title}",
                                tabindex: "-1",
                                "sandbox": "",
                                "scrolling": "no",
                                "srcdoc": "{html}",
                            }
                        }
                        if hidden > 0 && at + 1 == MOSAIC_TILES {
                            span { class: "nf-screens-more", "{more}" }
                        }
                    }
                }
            }
            span { class: "nf-screens-link-text",
                span { class: "nf-screens-link-title", "Галерея екранів" }
                span { class: "nf-screens-link-sub", "{hint}" }
            }
            span { class: "nf-screens-link-go", ArrowRightIcon {} }
        }
    }
}

#[component]
fn ScreenBoard(detail: ProjectDetail, viewing: Signal<Option<i64>>, on_close: EventHandler<()>) -> Element {
    let name = detail.project.name.clone();
    let total = plural(detail.screens.len() as u32, SCREEN_WORDS);
    let tiles: Vec<(String, Screen)> = detail
        .screens
        .iter()
        .enumerate()
        .map(|(index, screen)| (format!("{:02}", index + 1), screen.clone()))
        .collect();

    rsx! {
        div {
            class: "nf-board",
            role: "dialog",
            aria_modal: "true",
            aria_label: "Екрани гри «{name}»",
            tabindex: "-1",
            onmounted: move |event| async move {
                let _ = event.data().set_focus(true).await;
            },
            onkeydown: move |event| {
                if event.key() == Key::Escape {
                    event.prevent_default();
                    event.stop_propagation();
                    on_close.call(());
                }
            },
            header { class: "nf-board-head",
                h2 { class: "nf-board-title", title: "{name}", "Екрани гри «{name}»" }
                span { class: "nf-board-count", "{total}" }
                button {
                    class: "nf-inspector-close",
                    r#type: "button",
                    title: "Закрити (Esc)",
                    aria_label: "Закрити галерею екранів",
                    onclick: move |_| on_close.call(()),
                    CloseIcon {}
                }
            }
            div { class: "nf-board-grid",
                for (number, screen) in tiles {
                    article { key: "{screen.id}", class: "nf-board-tile",
                        if let Some(html) = &screen.html {
                            button {
                                class: "nf-preview",
                                r#type: "button",
                                title: "Відкрити «{screen.title}» на весь екран",
                                aria_label: "Відкрити «{screen.title}» на весь екран",
                                onclick: {
                                    let screen_id = screen.id;
                                    let mut viewing = viewing;
                                    move |_| viewing.set(Some(screen_id))
                                },
                                iframe {
                                    title: "{screen.title}",
                                    tabindex: "-1",
                                    "sandbox": "",
                                    "scrolling": "no",
                                    "srcdoc": "{html}",
                                }
                                span { class: "nf-board-number", "{number}" }
                                span { class: "nf-preview-hint",
                                    ExpandIcon {}
                                    "На весь екран"
                                }
                            }
                        } else {
                            div { class: "nf-preview",
                                p { class: "nf-viewer-empty", "{blank_label(screen.status)}" }
                                span { class: "nf-board-number", "{number}" }
                            }
                        }
                        div { class: "nf-item-head",
                            span { class: "nf-led is-{screen_phase(screen.status).code()}" }
                            span { class: "nf-item-title", title: "{screen.title}", "{screen.title}" }
                            span { class: "nf-item-status", "{screen_status_label(screen.status)}" }
                        }
                        if !screen.summary.is_empty() {
                            p { class: "nf-board-note", title: "{screen.summary}", "{screen.summary}" }
                        }
                        if let Some(problem) = &screen.error {
                            p { class: "nf-item-note is-failed", "{explain(problem)}" }
                        }
                        ScreenActions { screen: screen.clone() }
                    }
                }
            }
        }
    }
}

#[component]
fn ScreenActions(screen: Screen) -> Element {
    let studio = use_studio();
    let actions = use_context::<Actions>();
    let screen_id = screen.id;
    let redo_key = format!("redo-{screen_id}");
    let can_redo = matches!(
        screen.status,
        ScreenStatus::Draft | ScreenStatus::Approved | ScreenStatus::Failed
    );

    rsx! {
        div { class: "nf-actions",
            if screen.status == ScreenStatus::Draft {
                button {
                    class: "nf-button is-primary",
                    r#type: "button",
                    disabled: actions.locked(),
                    onclick: move |_| {
                        actions.run(format!("approve-{screen_id}"), async move {
                            bridge::approve_screen(screen_id)
                                .await
                                .map_err(|problem| explain(&problem))?;
                            studio.changed();
                            Ok(())
                        })
                    },
                    CheckIcon {}
                    "Апрувнути"
                }
            }
            if can_redo {
                button {
                    class: "nf-button is-quiet",
                    r#type: "button",
                    disabled: actions.locked(),
                    onclick: move |_| {
                        actions.run(format!("redo-{screen_id}"), async move {
                            bridge::regenerate_screen(screen_id)
                                .await
                                .map_err(|problem| explain(&problem))?;
                            studio.changed();
                            Ok(())
                        })
                    },
                    if actions.pending(&redo_key) {
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

fn blank_label(status: ScreenStatus) -> &'static str {
    match status {
        ScreenStatus::Queued => "Чекає в черзі",
        ScreenStatus::Generating => "Малюється…",
        ScreenStatus::Failed => "Не вдалося намалювати",
        ScreenStatus::Draft | ScreenStatus::Approved => "Цей екран ще не намальовано",
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
fn PlaytestPanel(project_id: i64, overview: Option<BuildOverview>) -> Element {
    let actions = use_context::<Actions>();
    let Some(overview) = overview else {
        return rsx! {
            p { class: "nf-inspector-lead", "Завантажую стан збірки…" }
        };
    };
    let playtest = overview.playtest.clone();
    let run = playtest.run.clone();
    let active = playtest.is_active();
    let source_ready = overview
        .source
        .as_ref()
        .is_some_and(|source| source.valid && source.status != SourceStatus::Generating);
    let can_start = playtest.blocked.is_none() && source_ready && !active;
    let scenes = run
        .as_ref()
        .map(|run| run.scenes.join(" → "))
        .unwrap_or_default();
    let activity: Vec<String> = run
        .as_ref()
        .map(|run| run.activity.iter().rev().take(ACTIVITY_LINES).rev().cloned().collect())
        .unwrap_or_default();

    rsx! {
        if let Some(reason) = &playtest.blocked {
            p { class: "nf-notice is-warning", "{reason}" }
        }
        if run.is_none() && playtest.blocked.is_none() {
            p { class: "nf-inspector-lead",
                "Бот студії відкриває гру у вікні 1280×720, сам тисне «Почати», бігає, стрибає, ставить паузу й знімає кадри. "
                "Claude порівнює їх із макетами Claude Design, виправляє, що криво, і бот перевіряє гру ще раз. Потім ідуть білди."
            }
        }
        if let Some(run) = &run {
            if let Some(report) = &run.report {
                section { class: "nf-section",
                    h3 { class: "nf-section-title", "{report.verdict.label()}" }
                    if !report.summary.is_empty() {
                        p { class: "nf-text", "{report.summary}" }
                    }
                    if !report.issues.is_empty() {
                        ul { class: "nf-rows",
                            for (index, issue) in report.issues.iter().enumerate() {
                                li { key: "{index}", class: "nf-row nf-issue",
                                    span {
                                        class: if issue.fixed { "nf-led is-done" } else { "nf-led is-failed" },
                                        title: if issue.fixed { "Виправлено" } else { "Лишилось" },
                                    }
                                    span { class: "nf-issue-text", "{issue.text}" }
                                }
                            }
                        }
                    }
                }
            }
            if run.finished_early {
                p { class: "nf-item-note is-failed", "Гра закрилась сама раніше, ніж бот дограв." }
            }
            if let Some(problem) = &run.error {
                p { class: "nf-item-note is-failed", "{problem}" }
            }
            if !run.shots.is_empty() {
                section { class: "nf-section",
                    h3 { class: "nf-section-title", "Кадри бота" }
                    if !scenes.is_empty() {
                        p { class: "nf-item-note", "Сцени: {scenes}" }
                    }
                    div { class: "nf-shots",
                        for (index, shot) in run.shots.iter().enumerate() {
                            figure { key: "{index}", class: if shot.blank { "nf-shot is-blank" } else { "nf-shot" },
                                img { src: "{shot.thumb}", alt: "{shot.label}", draggable: "false", "loading": "lazy" }
                                figcaption { class: "nf-shot-caption",
                                    span { class: "nf-shot-label", title: "{shot.scene}", "{shot.label}" }
                                    if shot.blank {
                                        span { class: "nf-shot-flag", "порожній" }
                                    }
                                    span { class: "nf-shot-time", "{shot.second} с" }
                                }
                            }
                        }
                    }
                }
            }
            if !run.errors.is_empty() {
                section { class: "nf-section",
                    h3 { class: "nf-section-title", "Помилки Godot" }
                    ul { class: "nf-log is-errors",
                        for (index, line) in run.errors.iter().enumerate() {
                            li { key: "{index}", title: "{line}", "{line}" }
                        }
                    }
                }
            }
            if !activity.is_empty() {
                section { class: "nf-section",
                    h3 { class: "nf-section-title", "Хід роботи Claude" }
                    ul { class: "nf-log",
                        for (index, line) in activity.into_iter().enumerate() {
                            li { key: "{index}", title: "{line}", "{line}" }
                        }
                    }
                }
            }
        }
        div { class: "nf-actions",
            if active {
                button {
                    class: "nf-button is-danger",
                    r#type: "button",
                    disabled: actions.locked(),
                    onclick: move |_| {
                        actions.run("stop-playtest", async move {
                            build_bridge::stop_playtest(project_id)
                                .await
                                .map_err(|problem| explain_build(&problem))
                        })
                    },
                    "Зупинити плейтест"
                }
            } else {
                button {
                    class: "nf-button is-primary",
                    r#type: "button",
                    disabled: actions.locked() || !can_start,
                    onclick: move |_| {
                        actions.run("playtest", async move {
                            build_bridge::start_playtest(project_id)
                                .await
                                .map(|_| ())
                                .map_err(|problem| explain_build(&problem))
                        })
                    },
                    if actions.pending("playtest") {
                        span { class: "nf-spinner" }
                    }
                    if run.is_some() { "Зіграти ще раз" } else { "Запустити плейтест" }
                }
            }
            if run.is_some() {
                button {
                    class: "nf-button is-quiet",
                    r#type: "button",
                    onclick: move |_| {
                        actions.run("playtest-folder", async move {
                            build_bridge::open_folder(project_id, PLAYTEST_FOLDER)
                                .await
                                .map_err(|problem| explain_build(&problem))
                        })
                    },
                    ExternalIcon {}
                    "Відкрити кадри"
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
        NodeId::Plan | NodeId::Screens | NodeId::Foundation | NodeId::Skills | NodeId::OpenDesign => {
            (Section::ClaudeDesign, "Відкрити в Claude Design")
        }
        NodeId::References => (Section::Pinterest, "Відкрити Pinterest"),
        NodeId::Code | NodeId::Playtest | NodeId::Builds | NodeId::Build(_) => (Section::Build, "Відкрити «Збірку»"),
        NodeId::Assets => (Section::ComfyUi, "Відкрити ComfyUI"),
        NodeId::Finds => (Section::AssetHunt, "Відкрити «Пошук ассетів»"),
        NodeId::Animations => (Section::Mixamo, "Відкрити Mixamo"),
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
