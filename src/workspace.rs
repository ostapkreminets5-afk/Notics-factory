use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use noctics_shared::{BuildTarget, GameEngine, MAX_PROMPT_CHARS, Session, StudioError, ToolSlot};

use crate::bridge;
use crate::build::BuildPage;
use crate::comfy::{Comfy, ComfyPage, provide_comfy, use_comfy};
use crate::design::ClaudeDesignPage;
use crate::engine_switcher::EngineSwitcher;
use crate::factory_mode::{FactoryMode, ModeSwitch, provide_factory_mode};
use crate::home::CorePage;
use crate::journal::JournalPage;
use crate::icons::{
    ArrowRightIcon, BoxIcon, ChevronsIcon, CpuIcon, FolderIcon, GridIcon, ListIcon, LogoMark,
    PackageIcon, PanelsIcon, RouteIcon, SettingsIcon, SidebarIcon, StarIcon,
};
use crate::node_factory::NodeFactory;
use crate::pipeline::PipelineCanvas;
use crate::project_switcher::ProjectSwitcher;
use crate::projects::ProjectsPage;
use crate::references::ReferencesPage;
use crate::settings::SettingsPage;
use crate::sidebar_layout::{SidebarResizer, provide_sidebar_layout, use_sidebar_layout};
use crate::skills::SkillsPage;
use crate::studio::{Section, Studio, explain, local_storage, provide_studio, use_studio};
use crate::tools::{ToolPage, ToolPicker, Tools, picker_placement, provide_tools, slot_for, use_tools};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Badge {
    Online,
    Count(&'static str),
}

#[derive(Clone, PartialEq, Eq)]
enum BadgeView {
    Online,
    Attention,
    Busy,
    Count(String),
}

struct SystemEntry {
    section: Section,
    title: &'static str,
    note: &'static str,
    badge: Badge,
}

struct UtilityEntry {
    section: Section,
    title: &'static str,
    note: &'static str,
}

const SYSTEMS: [SystemEntry; 7] = [
    SystemEntry {
        section: Section::Core,
        title: "AI-ядро",
        note: "Центр усіх систем",
        badge: Badge::Online,
    },
    SystemEntry {
        section: Section::Skills,
        title: "SKILLS",
        note: "Навички AI-ядра",
        badge: Badge::Online,
    },
    SystemEntry {
        section: Section::ComfyUi,
        title: "ComfyUI",
        note: "Арт, 3D і звук",
        badge: Badge::Online,
    },
    SystemEntry {
        section: Section::ClaudeDesign,
        title: "Claude Design",
        note: "UI та екрани гри",
        badge: Badge::Online,
    },
    SystemEntry {
        section: Section::Pinterest,
        title: "Pinterest",
        note: "Референси й настрій",
        badge: Badge::Online,
    },
    SystemEntry {
        section: Section::Models,
        title: "3D Models",
        note: "Бібліотека моделей",
        badge: Badge::Online,
    },
    SystemEntry {
        section: Section::Build,
        title: "Збірка",
        note: "Білди гри",
        badge: Badge::Count("06"),
    },
];

const UTILITIES: [UtilityEntry; 2] = [
    UtilityEntry {
        section: Section::Journal,
        title: "Журнал ядра",
        note: "Події та рішення AI-ядра",
    },
    UtilityEntry {
        section: Section::Settings,
        title: "Налаштування",
        note: "Студія, підписка Claude і акаунт",
    },
];

const NAV_ORDER_KEY: &str = "noctics.nav-order";
const NAV_ORDER_SEPARATOR: &str = "\n";

const COLLAPSE_LABEL: &str = "Згорнути панель";
const EXPAND_LABEL: &str = "Розгорнути панель";

const PIPELINE_TITLE: &str = "Конвеєр гри";
const PIPELINE_NOTE: &str = "Опиши гру — Claude складе дизайн-документ, стиль і спроєктує екрани в Claude Design.";

#[component]
pub fn Workspace(session: Session, on_sign_out: EventHandler<()>) -> Element {
    let studio = provide_studio();
    let tools = provide_tools();
    provide_comfy();
    let layout = provide_sidebar_layout();
    let mode = provide_factory_mode();
    if mode.get() == FactoryMode::Nodes {
        return rsx! {
            NodeFactory {}
        };
    }
    let section = (studio.section)();

    let view = match section {
        Section::Core if !tools.runs_builtin(ToolSlot::Core) => rsx! {
            ToolPage { slot: ToolSlot::Core }
        },
        Section::ClaudeDesign if !tools.runs_builtin(ToolSlot::Interface) => rsx! {
            ToolPage { slot: ToolSlot::Interface }
        },
        Section::Build if tools.runs_builtin(ToolSlot::Build) => rsx! {
            BuildPage {}
        },
        Section::Core => rsx! {
            CorePage {}
        },
        Section::Skills => rsx! {
            SkillsPage {}
        },
        Section::Pipeline => rsx! {
            PipelineView {}
        },
        Section::ClaudeDesign => rsx! {
            ClaudeDesignPage {}
        },
        Section::Projects => rsx! {
            ProjectsPage {}
        },
        Section::Pinterest => rsx! {
            ReferencesPage {}
        },
        Section::Journal => rsx! {
            JournalPage {}
        },
        Section::Settings => rsx! {
            SettingsPage { session: session.clone(), on_sign_out }
        },
        Section::ComfyUi if tools.runs_comfy() => rsx! {
            ComfyPage {}
        },
        other => match slot_for(other) {
            Some(slot) => rsx! {
                ToolPage { slot }
            },
            None => {
                let (title, note) = heading_of(other);
                rsx! {
                    Placeholder { title, note }
                }
            }
        },
    };

    rsx! {
        div { class: if layout.resizing() { "workspace is-resizing" } else { "workspace" },
            Sidebar {}
            SidebarResizer {}
            div { class: "workspace-view", key: "{section:?}", {view} }
        }
    }
}

#[component]
fn Sidebar() -> Element {
    let studio = use_studio();
    let layout = use_sidebar_layout();
    let collapsed = layout.collapsed();
    let current = (studio.section)();
    let pipeline_class = if current == Section::Pipeline {
        "pipeline-button is-active"
    } else {
        "pipeline-button"
    };

    let tools = use_tools();
    let comfy = use_comfy();
    let mut picker = use_signal(|| None::<(ToolSlot, String)>);
    let mut order = use_signal(load_nav_order);
    let mut held = use_signal(|| None::<usize>);
    let mut dragging = use_signal(|| false);
    let mut suppress_click = use_signal(|| false);

    let mut drag_onto = move |target: usize| {
        let Some(moving) = held() else {
            return;
        };
        if moving == target {
            return;
        }
        dragging.set(true);
        let mut sequence = order.write();
        let from = sequence.iter().position(|&index| index == moving);
        let to = sequence.iter().position(|&index| index == target);
        if let (Some(from), Some(to)) = (from, to) {
            let item = sequence.remove(from);
            sequence.insert(to, item);
        }
    };

    let mut finish_drag = move || {
        if held().is_none() {
            return;
        }
        held.set(None);
        if dragging() {
            dragging.set(false);
            suppress_click.set(true);
            save_nav_order(&order.read());
        }
    };

    rsx! {
        aside { class: if collapsed { "sidebar is-collapsed" } else { "sidebar" },
            div { class: "sidebar-brand", "data-tauri-drag-region": "true",
                LogoMark {}
                span { class: "wordmark", "NOCTICS" }
                span { class: "brand-sub", "factory" }
            }
            ModeSwitch {}
            ProjectSwitcher {}
            EngineSwitcher {}
            button {
                class: pipeline_class,
                r#type: "button",
                title: collapsed.then_some(PIPELINE_TITLE),
                aria_label: PIPELINE_TITLE,
                onclick: move |_| studio.open(Section::Pipeline),
                span { "{PIPELINE_TITLE}" }
                ArrowRightIcon {}
            }
            p { class: "mono-label sidebar-label", "Системи" }
            nav {
                class: if dragging() { "sidebar-nav is-sorting" } else { "sidebar-nav" },
                aria_label: "Системи",
                onpointerup: move |_| finish_drag(),
                onpointerleave: move |_| finish_drag(),
                for (index, entry) in order().into_iter().map(|index| (index, &SYSTEMS[index])) {
                    div { key: "{entry.title}", class: "nav-row",
                        button {
                            class: nav_item_class(entry.section == current, dragging() && held() == Some(index)),
                            r#type: "button",
                            title: collapsed.then(|| format!("{} · {}", nav_title(entry, tools), nav_note(entry, studio))),
                            aria_current: if entry.section == current { "page" } else { "false" },
                            onpointerdown: move |event| {
                                suppress_click.set(false);
                                if event.trigger_button() == Some(MouseButton::Primary) {
                                    held.set(Some(index));
                                }
                            },
                            onpointerenter: move |_| drag_onto(index),
                            onclick: {
                                let target = entry.section;
                                move |_| {
                                    if suppress_click() {
                                        suppress_click.set(false);
                                    } else {
                                        studio.open(target);
                                    }
                                }
                            },
                            NavIcon { section: entry.section }
                            span { class: "nav-text",
                                span { class: "nav-title", "{nav_title(entry, tools)}" }
                                span { class: "nav-note", "{nav_note(entry, studio)}" }
                            }
                            {
                                match badge_for(entry, studio, comfy, tools) {
                                    BadgeView::Online => rsx! {
                                        span { class: "nav-dot is-online", title: "Працює" }
                                    },
                                    BadgeView::Attention => rsx! {
                                        span { class: "nav-dot is-attention", title: "Потребує уваги" }
                                    },
                                    BadgeView::Busy => rsx! {
                                        span { class: "nav-dot is-busy", title: "Claude генерує" }
                                    },
                                    BadgeView::Count(count) => rsx! {
                                        span { class: "nav-count", "{count}" }
                                    },
                                }
                            }
                        }
                        if let Some(slot) = slot_for(entry.section) {
                            button {
                                class: if picker.read().as_ref().is_some_and(|(open, _)| *open == slot) { "nav-switch is-open" } else { "nav-switch" },
                                r#type: "button",
                                title: "Змінити або додати інструмент",
                                aria_label: "Інструмент для «{entry.note}»",
                                aria_haspopup: "dialog",
                                onpointerdown: move |event| event.stop_propagation(),
                                onclick: move |event| {
                                    event.stop_propagation();
                                    let opened = picker.read().as_ref().is_some_and(|(open, _)| *open == slot);
                                    if opened {
                                        picker.set(None);
                                    } else {
                                        picker.set(Some((slot, picker_placement(&event))));
                                    }
                                },
                                ChevronsIcon {}
                            }
                        }
                    }
                }
            }
            if let Some((slot, placement)) = picker() {
                ToolPicker {
                    key: "{slot:?}",
                    slot,
                    placement,
                    on_close: move |_| picker.set(None),
                }
            }
            nav { class: "sidebar-foot", aria_label: "Сервіс",
                for entry in UTILITIES.iter() {
                    button {
                        key: "{entry.title}",
                        class: if entry.section == current { "nav-item is-compact is-active" } else { "nav-item is-compact" },
                        r#type: "button",
                        title: collapsed.then_some(entry.title),
                        aria_current: if entry.section == current { "page" } else { "false" },
                        onclick: {
                            let target = entry.section;
                            move |_| studio.open(target)
                        },
                        NavIcon { section: entry.section }
                        span { class: "nav-title", "{entry.title}" }
                    }
                }
                button {
                    class: "nav-item is-compact sidebar-toggle",
                    r#type: "button",
                    title: if collapsed { EXPAND_LABEL } else { COLLAPSE_LABEL },
                    aria_expanded: if collapsed { "false" } else { "true" },
                    onclick: move |_| layout.toggle(),
                    SidebarIcon {}
                    span { class: "nav-title", if collapsed { "{EXPAND_LABEL}" } else { "{COLLAPSE_LABEL}" } }
                }
            }
        }
    }
}

fn nav_title(entry: &SystemEntry, tools: Tools) -> String {
    slot_for(entry.section)
        .and_then(|slot| tools.active(slot))
        .map_or_else(|| entry.title.to_owned(), |tool| tool.name)
}

fn nav_note(entry: &SystemEntry, studio: Studio) -> String {
    if entry.section != Section::Build {
        return entry.note.to_owned();
    }
    let engine = studio
        .focused_project()
        .map_or_else(|| (studio.draft_engine)(), |project| project.engine);
    format!("{} · {}", entry.note, engine.label())
}

fn nav_item_class(active: bool, dragged: bool) -> &'static str {
    match (active, dragged) {
        (true, true) => "nav-item is-active is-dragged",
        (true, false) => "nav-item is-active",
        (false, true) => "nav-item is-dragged",
        (false, false) => "nav-item",
    }
}

fn load_nav_order() -> Vec<usize> {
    let stored = local_storage()
        .and_then(|storage| storage.get_item(NAV_ORDER_KEY).ok().flatten())
        .unwrap_or_default();
    let remembered = stored
        .split(NAV_ORDER_SEPARATOR)
        .filter_map(|title| SYSTEMS.iter().position(|entry| entry.title == title));
    let mut order = Vec::with_capacity(SYSTEMS.len());
    for index in remembered.chain(0..SYSTEMS.len()) {
        if !order.contains(&index) {
            order.push(index);
        }
    }
    order
}

fn save_nav_order(order: &[usize]) {
    let titles = order
        .iter()
        .map(|&index| SYSTEMS[index].title)
        .collect::<Vec<_>>()
        .join(NAV_ORDER_SEPARATOR);
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(NAV_ORDER_KEY, &titles);
    }
}

fn badge_for(entry: &SystemEntry, studio: Studio, comfy: Comfy, tools: Tools) -> BadgeView {
    let projects = studio.projects.read();
    match entry.section {
        Section::ComfyUi if tools.runs_comfy() && comfy.busy() => BadgeView::Busy,
        Section::ClaudeDesign
            if studio.connected()
                && projects
                    .iter()
                    .any(|project| project.status.is_planning() || project.pending_count > 0) =>
        {
            BadgeView::Busy
        }
        Section::Core if studio.settings.read().is_some() && !studio.connected() => BadgeView::Attention,
        _ => match entry.badge {
            Badge::Online => BadgeView::Online,
            Badge::Count(count) => BadgeView::Count(count.to_owned()),
        },
    }
}

#[component]
fn NavIcon(section: Section) -> Element {
    match section {
        Section::Core => rsx! {
            CpuIcon {}
        },
        Section::Skills => rsx! {
            StarIcon {}
        },
        Section::ComfyUi => rsx! {
            RouteIcon {}
        },
        Section::ClaudeDesign => rsx! {
            PanelsIcon {}
        },
        Section::Projects => rsx! {
            FolderIcon {}
        },
        Section::Pinterest => rsx! {
            GridIcon {}
        },
        Section::Models => rsx! {
            BoxIcon {}
        },
        Section::Build => rsx! {
            PackageIcon {}
        },
        Section::Journal => rsx! {
            ListIcon {}
        },
        Section::Settings => rsx! {
            SettingsIcon {}
        },
        Section::Pipeline => rsx! {
            ArrowRightIcon {}
        },
    }
}

#[component]
fn PipelineView() -> Element {
    let studio = use_studio();
    let mut prompt = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<StudioError>);

    let mut launch = move || {
        if pending() || prompt.read().trim().is_empty() {
            return;
        }
        pending.set(true);
        error.set(None);
        spawn(async move {
            let targets = studio.build_plan.read().clone();
            match bridge::start_project(&prompt(), (studio.draft_engine)(), &targets).await {
                Ok(project) => {
                    prompt.set(String::new());
                    studio.changed();
                    studio.open_project(project.id);
                }
                Err(problem) => {
                    error.set(Some(problem));
                    pending.set(false);
                }
            }
        });
    };

    let length = prompt.read().chars().count();
    let engine = (studio.draft_engine)();
    let planned: Vec<&str> = BuildTarget::ALL
        .into_iter()
        .filter(|target| studio.plans_build(*target))
        .map(BuildTarget::label)
        .collect();
    let plan_summary = if planned.is_empty() {
        "не обрано".to_owned()
    } else {
        planned.join(", ")
    };

    rsx! {
        section { class: "pipeline-view",
            PipelineCanvas {}
            header { class: "page-head is-floating",
                p { class: "mono-label", "Система · Конвеєр" }
                h1 { class: "page-title", "{PIPELINE_TITLE}" }
                p { class: "page-lead", "{PIPELINE_NOTE}" }
            }
            form {
                class: "composer",
                novalidate: true,
                onsubmit: move |event| {
                    event.prevent_default();
                    launch();
                },
                div { class: "field-head",
                    label { class: "mono-label", r#for: "game-prompt", "Промпт гри" }
                    span { class: "mono-label", "{length} / {MAX_PROMPT_CHARS}" }
                }
                textarea {
                    class: "input is-area composer-input",
                    id: "game-prompt",
                    rows: "3",
                    maxlength: "{MAX_PROMPT_CHARS}",
                    placeholder: "Наприклад: стелс про злодія, який краде світло з ліхтарів нічного міста, щоб урятувати сестру",
                    value: "{prompt}",
                    oninput: move |event| prompt.set(event.value()),
                    onkeydown: move |event| {
                        if event.key() == Key::Enter && event.modifiers().ctrl() {
                            event.prevent_default();
                            launch();
                        }
                    },
                }
                div { class: "engine-field",
                    span { class: "mono-label", "Рушій · {engine.stack()}" }
                    div { class: "chip-row engine-chips", role: "radiogroup", aria_label: "Рушій гри",
                        for choice in GameEngine::ALL {
                            button {
                                key: "{choice.code()}",
                                class: if choice == engine { "chip is-active" } else { "chip" },
                                r#type: "button",
                                role: "radio",
                                aria_checked: if choice == engine { "true" } else { "false" },
                                title: "{choice.label()} · {choice.stack()}",
                                onclick: move |_| studio.choose_draft_engine(choice),
                                "{choice.label()}"
                            }
                        }
                    }
                }
                div { class: "engine-field",
                    span { class: "mono-label", "Білд під · {plan_summary}" }
                    div { class: "chip-row engine-chips", role: "group", aria_label: "Платформи білду",
                        for target in BuildTarget::ALL {
                            button {
                                key: "{target.code()}",
                                class: if studio.plans_build(target) { "chip is-active" } else { "chip" },
                                r#type: "button",
                                aria_pressed: if studio.plans_build(target) { "true" } else { "false" },
                                title: "{target.label()} · {target.package()}",
                                onclick: move |_| studio.toggle_build_target(target),
                                "{target.label()}"
                            }
                        }
                    }
                }
                if let Some(problem) = error() {
                    div { class: "composer-alert", role: "alert",
                        span { "{explain(&problem)}" }
                        if matches!(problem, StudioError::ClaudeCodeMissing | StudioError::ClaudeSignedOut) {
                            button {
                                class: "ghost-button is-small",
                                r#type: "button",
                                onclick: move |_| studio.open(Section::Settings),
                                "Налаштування"
                            }
                        }
                    }
                }
                div { class: "composer-foot",
                    span { class: "mono-label",
                        "Навички → план → GDD → стиль → екрани в Claude Design → проєкт"
                    }
                    button {
                        class: "primary-button",
                        r#type: "submit",
                        disabled: pending() || prompt.read().trim().is_empty(),
                        if pending() {
                            span { class: "spinner" }
                            "Запускаємо…"
                        } else {
                            "Запустити конвеєр"
                            ArrowRightIcon {}
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Placeholder(title: &'static str, note: &'static str) -> Element {
    rsx! {
        section { class: "page",
            header { class: "page-head",
                p { class: "mono-label", "Система · {title}" }
                h1 { class: "page-title", "{title}" }
                p { class: "page-lead", "{note}" }
            }
            div { class: "page-empty",
                p { class: "mono-label", "Розділ у розробці" }
                p {
                    "Цей розділ з’явиться в наступних збірках. Уже працюють SKILLS, Claude Design і Projects."
                }
            }
        }
    }
}

fn heading_of(section: Section) -> (&'static str, &'static str) {
    SYSTEMS
        .iter()
        .map(|entry| (entry.section, entry.title, entry.note))
        .chain(UTILITIES.iter().map(|entry| (entry.section, entry.title, entry.note)))
        .find(|(candidate, _, _)| *candidate == section)
        .map_or((PIPELINE_TITLE, PIPELINE_NOTE), |(_, title, note)| (title, note))
}
