use dioxus::prelude::*;
use noctics_shared::{
    INTERFACE_STAGE, OutputStatus, ProjectDetail, ProjectStatus, ScreenStatus, ToolSlot,
};

use crate::bridge;
use crate::icons::{
    ArrowRightIcon, BoxIcon, CpuIcon, FolderIcon, GridIcon, PanelsIcon, RouteIcon, StarIcon,
};
use crate::studio::{Section, output_status_label, use_studio};
use crate::tools::use_tools;

const VIEW_WIDTH: f64 = 1000.0;
const VIEW_HEIGHT: f64 = 540.0;
const NODE_WIDTH: f64 = 220.0;
const NODE_HEIGHT: f64 = 76.0;
const STAGE_NAMES: [&str; 6] = ["Промпт", "Концепт", "Модель", "Матеріал", "Анімація", "Збірка"];
const RECENT_LIMIT: usize = 3;
const PINTEREST_TOOL: &str = "pinterest";
const ARROW_COLORS: [(&str, &str); 3] = [
    ("online", "#3b7bff"),
    ("link", "#6a7286"),
    ("waiting", "#f2b444"),
];

#[derive(Clone, Copy, PartialEq)]
enum Glyph {
    Skills,
    References,
    Images,
    Interface,
    Models,
    Projects,
}

#[derive(Clone, PartialEq)]
enum Health {
    Online,
    Link,
    Waiting(&'static str),
}

impl Health {
    fn label(&self) -> &'static str {
        match self {
            Self::Online => "Онлайн",
            Self::Link => "За посиланням",
            Self::Waiting(reason) => reason,
        }
    }

    fn tone(&self) -> &'static str {
        match self {
            Self::Online => "is-online",
            Self::Link => "is-link",
            Self::Waiting(_) => "is-waiting",
        }
    }
}

#[derive(Clone, PartialEq)]
struct SystemNode {
    section: Section,
    glyph: Glyph,
    title: String,
    note: String,
    health: Health,
    center: (f64, f64),
}

struct Link {
    from: (f64, f64),
    to: (f64, f64),
    label: &'static str,
    label_at: (f64, f64),
    target: usize,
}

const SKILLS: usize = 0;
const REFERENCES: usize = 1;
const IMAGES: usize = 2;
const INTERFACE: usize = 3;
const MODELS: usize = 4;
const PROJECTS: usize = 5;

const LINKS: [Link; 7] = [
    Link { from: (500.0, 135.0), to: (500.0, 92.0), label: "інструкції", label_at: (548.0, 114.0), target: SKILLS },
    Link { from: (380.0, 200.0), to: (264.0, 200.0), label: "референси", label_at: (322.0, 186.0), target: REFERENCES },
    Link { from: (380.0, 340.0), to: (264.0, 340.0), label: "концепти", label_at: (322.0, 326.0), target: IMAGES },
    Link { from: (620.0, 200.0), to: (736.0, 200.0), label: "екрани UI", label_at: (678.0, 186.0), target: INTERFACE },
    Link { from: (620.0, 340.0), to: (736.0, 340.0), label: "моделі", label_at: (678.0, 326.0), target: MODELS },
    Link { from: (500.0, 405.0), to: (500.0, 448.0), label: "збірка", label_at: (536.0, 427.0), target: PROJECTS },
    Link { from: (150.0, 240.0), to: (150.0, 298.0), label: "піни", label_at: (178.0, 269.0), target: IMAGES },
];

struct Activity {
    stage: Option<u8>,
    headline: String,
    detail: String,
    progress: Option<u32>,
}

#[component]
pub fn CorePage() -> Element {
    let studio = use_studio();
    let tools = use_tools();

    let skills = use_resource(move || async move {
        let _ = (studio.revision)();
        bridge::list_skills().await.ok()
    });
    let detail = use_resource(move || async move {
        let _ = (studio.revision)();
        let project_id = studio.focused_project().map(|project| project.id)?;
        bridge::project_detail(project_id).await.ok()
    });

    let projects = studio.projects.read().clone();
    let focused = studio.focused_project();
    let detail = detail.read().clone().flatten();
    let checked = studio.settings.read().is_some();
    let connected = studio.connected();
    let enabled_skills = skills
        .read()
        .clone()
        .flatten()
        .map(|list| list.iter().filter(|skill| skill.enabled).count());

    let core_health = |integrated: bool| match (integrated, connected) {
        (false, _) => Health::Link,
        (true, true) => Health::Online,
        (true, false) => Health::Waiting("Ядро офлайн"),
    };
    let tool_name = |slot: ToolSlot, fallback: &str| {
        tools
            .active(slot)
            .map_or_else(|| fallback.to_owned(), |tool| tool.name)
    };
    let integrated = |slot: ToolSlot| tools.active(slot).is_some_and(|tool| tool.integrated);
    let reference_count = focused.as_ref().map_or(0, |project| project.references.len());
    let references_health = match tools.active(ToolSlot::References) {
        Some(tool) if tool.id != PINTEREST_TOOL => Health::Link,
        _ if focused.is_none() => Health::Waiting("Чекає проєкт"),
        _ if reference_count == 0 => Health::Waiting("Очікує синхронізації"),
        _ => core_health(true),
    };

    let nodes = vec![
        SystemNode {
            section: Section::Skills,
            glyph: Glyph::Skills,
            title: "Skills".to_owned(),
            note: enabled_skills.map_or_else(
                || "Навички AI-ядра".to_owned(),
                |count| format!("Навички AI-ядра · {count}"),
            ),
            health: match enabled_skills {
                Some(0) => Health::Waiting("Немає навичок"),
                _ => core_health(true),
            },
            center: (500.0, 54.0),
        },
        SystemNode {
            section: Section::Pinterest,
            glyph: Glyph::References,
            title: tool_name(ToolSlot::References, "Pinterest"),
            note: if reference_count > 0 {
                format!("Референси й настрій · {reference_count}")
            } else {
                "Референси й настрій".to_owned()
            },
            health: references_health,
            center: (150.0, 200.0),
        },
        SystemNode {
            section: Section::ComfyUi,
            glyph: Glyph::Images,
            title: tool_name(ToolSlot::Images, "ComfyUI"),
            note: "Генерація зображень".to_owned(),
            health: core_health(integrated(ToolSlot::Images)),
            center: (150.0, 340.0),
        },
        SystemNode {
            section: Section::ClaudeDesign,
            glyph: Glyph::Interface,
            title: tool_name(ToolSlot::Interface, "Claude Design"),
            note: "UI та екрани гри".to_owned(),
            health: core_health(integrated(ToolSlot::Interface)),
            center: (850.0, 200.0),
        },
        SystemNode {
            section: Section::Models,
            glyph: Glyph::Models,
            title: tool_name(ToolSlot::Models, "3D Models"),
            note: "Бібліотека моделей".to_owned(),
            health: core_health(integrated(ToolSlot::Models)),
            center: (850.0, 340.0),
        },
        SystemNode {
            section: Section::Projects,
            glyph: Glyph::Projects,
            title: "Projects".to_owned(),
            note: format!("Проєкти ігор · {}", projects.len()),
            health: Health::Online,
            center: (500.0, 486.0),
        },
    ];

    let online = nodes
        .iter()
        .filter(|node| node.health == Health::Online)
        .count();
    let total = nodes.len();
    let activity = activity_of(focused.as_ref().map(|project| project.status), detail.as_ref());
    let queued: u32 = projects
        .iter()
        .map(|project| project.pending_count + u32::from(project.status.is_planning()))
        .sum();

    let (core_label, core_tone) = match (checked, connected) {
        (false, _) => ("перевіряється…".to_owned(), "is-waiting"),
        (true, true) => (format!("онлайн · {online}/{total} систем"), "is-online"),
        (true, false) => ("офлайн — підключи Claude".to_owned(), "is-waiting"),
    };
    let core_stage = activity
        .stage
        .map_or_else(|| "Noctics Core".to_owned(), |stage| format!("Noctics Core · етап {stage:02}"));

    let recent: Vec<(String, String)> = detail
        .as_ref()
        .map(|detail| {
            detail
                .outputs
                .iter()
                .filter(|output| output.status == OutputStatus::Done)
                .rev()
                .take(RECENT_LIMIT)
                .map(|output| {
                    (
                        format!("{:02}", output.stage),
                        format!("{} · {}", output.title, output_status_label(output.status)),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let focused_id = focused.as_ref().map(|project| project.id);
    let focused_name = focused.as_ref().map(|project| project.name.clone());

    rsx! {
        section { class: "page core-page",
            header { class: "page-head has-action",
                div { class: "page-head-text",
                    p { class: "mono-label", "Система · AI-ядро" }
                    h1 { class: "page-title", "AI-ядро" }
                    p { class: "page-lead", "Центральна система: ядро роздає роботу шести системам і збирає результат у гру." }
                }
                div { class: "head-actions",
                    span { class: "core-chip {core_tone}",
                        span { class: "core-chip-dot" }
                        "Ядро {core_label}"
                    }
                    button {
                        class: "primary-button",
                        r#type: "button",
                        onclick: move |_| studio.open(Section::Pipeline),
                        "Відкрити конвеєр"
                    }
                }
            }
            div { class: "core-map",
                div { class: "core-hub",
                    svg {
                        class: "core-lines",
                        view_box: "0 0 {VIEW_WIDTH} {VIEW_HEIGHT}",
                        "aria-hidden": "true",
                        defs {
                            for (tone, color) in ARROW_COLORS {
                                marker {
                                    key: "{tone}",
                                    id: "core-arrow-{tone}",
                                    view_box: "0 0 10 10",
                                    ref_x: "8",
                                    ref_y: "5",
                                    marker_width: "7",
                                    marker_height: "7",
                                    orient: "auto-start-reverse",
                                    path { d: "M0 0 10 5 0 10z", fill: "{color}" }
                                }
                            }
                        }
                        ellipse { class: "core-orbit", cx: "500", cy: "270", rx: "440", ry: "250" }
                        ellipse { class: "core-orbit is-inner", cx: "500", cy: "270", rx: "300", ry: "170" }
                        for (cx, cy, tone) in [(268.0, 64.0, "is-violet"), (732.0, 64.0, "is-blue"), (268.0, 476.0, "is-cyan"), (732.0, 476.0, "is-violet")] {
                            circle { class: "core-orbit-dot {tone}", cx: "{cx}", cy: "{cy}", r: "3.5" }
                        }
                        for link in LINKS.iter() {
                            line {
                                class: "core-link is-{link_tone(&nodes[link.target].health)}",
                                x1: "{link.from.0}",
                                y1: "{link.from.1}",
                                x2: "{link.to.0}",
                                y2: "{link.to.1}",
                                marker_end: "url(#core-arrow-{link_tone(&nodes[link.target].health)})",
                            }
                        }
                    }
                    for link in LINKS.iter() {
                        span {
                            class: "core-link-label is-{link_tone(&nodes[link.target].health)}",
                            style: "left: {percent_x(link.label_at.0)}%; top: {percent_y(link.label_at.1)}%",
                            "{link.label}"
                        }
                    }
                    div { class: "core-center",
                        span { class: "core-center-mark", CpuIcon {} }
                        p { class: "core-center-title", "AI-ядро" }
                        p { class: "core-center-note", "{core_stage}" }
                        p { class: "core-center-status {core_tone}", "{core_label}" }
                    }
                    for node in nodes.iter().cloned() {
                        SystemCard { key: "{node.title}", node }
                    }
                }
            }
            div { class: "core-stats",
                article { class: "core-stat",
                    p { class: "mono-label", "Активний етап" }
                    p { class: "core-stat-title", "{activity.headline}" }
                    p { class: "core-stat-note", "{activity.detail}" }
                    if let Some(progress) = activity.progress {
                        div {
                            class: "core-progress",
                            role: "progressbar",
                            aria_valuemin: "0",
                            aria_valuemax: "100",
                            aria_valuenow: "{progress}",
                            div { class: "core-progress-fill", style: "width: {progress}%" }
                        }
                    }
                }
                article { class: "core-stat",
                    p { class: "mono-label", "Черга ядра" }
                    p { class: "core-stat-title", "{queue_label(queued)}" }
                    p { class: "core-stat-note",
                        match &focused_name {
                            Some(name) if queued > 0 => format!("Зараз: {name} · {}", activity.detail),
                            Some(_) => "Claude нічого не генерує".to_owned(),
                            None => "Проєктів ще немає".to_owned(),
                        }
                    }
                    button {
                        class: "core-stat-link",
                        r#type: "button",
                        onclick: move |_| studio.open(Section::Pipeline),
                        "Відкрити конвеєр гри"
                    }
                }
                article { class: "core-stat",
                    p { class: "mono-label", "Останні результати" }
                    if recent.is_empty() {
                        p { class: "core-stat-note", "Готові результати навичок з’являться тут." }
                    } else {
                        ul { class: "core-recent",
                            for (stage, title) in recent {
                                li { key: "{stage}-{title}",
                                    span { class: "core-recent-stage", "{stage}" }
                                    span { "{title}" }
                                }
                            }
                        }
                    }
                    if let Some(project_id) = focused_id {
                        button {
                            class: "core-stat-link",
                            r#type: "button",
                            onclick: move |_| studio.open_project(project_id),
                            "Відкрити в Claude Design"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn SystemCard(node: SystemNode) -> Element {
    let studio = use_studio();
    let left = percent_x(node.center.0 - NODE_WIDTH / 2.0);
    let top = percent_y(node.center.1 - NODE_HEIGHT / 2.0);
    let width = percent_x(NODE_WIDTH);
    let height = percent_y(NODE_HEIGHT);
    let target = node.section;

    rsx! {
        button {
            class: "core-node",
            r#type: "button",
            style: "left: {left}%; top: {top}%; width: {width}%; height: {height}%",
            onclick: move |_| studio.open(target),
            span { class: "core-node-icon",
                match node.glyph {
                    Glyph::Skills => rsx! { StarIcon {} },
                    Glyph::References => rsx! { GridIcon {} },
                    Glyph::Images => rsx! { RouteIcon {} },
                    Glyph::Interface => rsx! { PanelsIcon {} },
                    Glyph::Models => rsx! { BoxIcon {} },
                    Glyph::Projects => rsx! { FolderIcon {} },
                }
            }
            span { class: "core-node-text",
                span { class: "core-node-title", "{node.title}" }
                span { class: "core-node-note", "{node.note}" }
                span { class: "core-node-status {node.health.tone()}", "{node.health.label()}" }
            }
            span { class: "core-node-go", ArrowRightIcon {} }
        }
    }
}

fn activity_of(status: Option<ProjectStatus>, detail: Option<&ProjectDetail>) -> Activity {
    let Some(status) = status else {
        return Activity {
            stage: None,
            headline: "Немає проєкту".to_owned(),
            detail: "Запусти конвеєр гри".to_owned(),
            progress: None,
        };
    };
    if status.is_planning() {
        return Activity {
            stage: Some(1),
            headline: stage_headline(1),
            detail: "Claude планує гру".to_owned(),
            progress: None,
        };
    }
    let Some(detail) = detail else {
        return Activity {
            stage: None,
            headline: "Завантажуємо…".to_owned(),
            detail: String::new(),
            progress: None,
        };
    };
    let total = detail.outputs.len() + detail.screens.len();
    let done = detail
        .outputs
        .iter()
        .filter(|output| output.status == OutputStatus::Done)
        .count()
        + detail
            .screens
            .iter()
            .filter(|screen| matches!(screen.status, ScreenStatus::Draft | ScreenStatus::Approved))
            .count();
    let progress = u32::try_from(done * 100 / total.max(1)).ok();
    let busy_output = detail.outputs.iter().find(|output| output.status.is_busy());
    let busy_screens = detail.screens.iter().filter(|screen| screen.status.is_busy()).count();
    match (busy_output, busy_screens) {
        (Some(output), _) => Activity {
            stage: Some(output.stage),
            headline: stage_headline(output.stage),
            detail: format!("{} · {}%", output.title, progress.unwrap_or_default()),
            progress,
        },
        (None, count) if count > 0 => Activity {
            stage: Some(INTERFACE_STAGE),
            headline: stage_headline(INTERFACE_STAGE),
            detail: format!("Claude Design · екранів у черзі: {count}"),
            progress,
        },
        _ => Activity {
            stage: None,
            headline: "Усе готово".to_owned(),
            detail: format!("Готово {done} з {total} результатів"),
            progress,
        },
    }
}

fn stage_headline(stage: u8) -> String {
    let name = STAGE_NAMES
        .get(usize::from(stage.saturating_sub(1)))
        .copied()
        .unwrap_or("Етап");
    format!("{stage:02} · {name}")
}

fn queue_label(queued: u32) -> String {
    let word = match (queued % 10, queued % 100) {
        (1, rest) if rest != 11 => "завдання",
        (2..=4, rest) if !(12..=14).contains(&rest) => "завдання",
        _ => "завдань",
    };
    format!("{queued} {word}")
}

fn link_tone(health: &Health) -> &'static str {
    match health {
        Health::Online => "online",
        Health::Link => "link",
        Health::Waiting(_) => "waiting",
    }
}

fn percent_x(value: f64) -> f64 {
    value / VIEW_WIDTH * 100.0
}

fn percent_y(value: f64) -> f64 {
    value / VIEW_HEIGHT * 100.0
}
