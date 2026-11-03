use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use noctics_shared::{BuildOverview, PinterestStatus, Project, ProjectDetail, ProjectStatus};

use crate::comfy::use_comfy;
use crate::factory_mode::ModeSwitch;
use crate::icons::{ChevronDownIcon, FolderIcon, LogoMark, PlusIcon};
use crate::node_canvas::NodeCanvas;
use crate::node_graph::{Draft, Node, NodeId, Phase, Snapshot, build_graph};
use crate::node_inspector::NodeInspector;
use crate::studio::{project_status_label, use_studio};
use crate::{bridge, build_bridge};

const BUILDS_CHANGED: &str = "builds-changed";
const TICK_MILLIS: i32 = 1000;
const MILLIS_PER_SECOND: f64 = 1000.0;
const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_HOUR: i64 = 3600;

#[component]
pub fn NodeFactory() -> Element {
    let studio = use_studio();
    let comfy = use_comfy();
    let mut drafting = use_signal(|| false);
    let mut selected = use_signal(|| None::<NodeId>);
    let mut dismissed = use_signal(|| false);
    let mut reveal = use_signal(|| 0_u64);
    let draft_prompt = use_signal(String::new);
    let mut detail = use_signal(|| None::<ProjectDetail>);
    let mut overview = use_signal(|| None::<BuildOverview>);
    let mut pinterest = use_signal(|| None::<PinterestStatus>);
    let mut builds_revision = use_signal(|| 0_u64);
    let subscription = use_hook(|| Rc::new(RefCell::new(None::<bridge::Subscription>)));

    let focus = use_memo(move || {
        if drafting() {
            None
        } else {
            studio.focused_project().map(|project| project.id)
        }
    });

    use_future(move || {
        let slot = Rc::clone(&subscription);
        async move {
            let listening = bridge::listen(BUILDS_CHANGED, move || {
                if let Ok(mut value) = builds_revision.try_write() {
                    *value += 1;
                }
            })
            .await;
            *slot.borrow_mut() = listening;
        }
    });

    use_resource(move || async move {
        let _ = (studio.revision)();
        let Some(project_id) = focus() else {
            detail.set(None);
            return;
        };
        if let Ok(fresh) = bridge::project_detail(project_id).await {
            detail.set(Some(fresh));
        }
    });

    use_resource(move || async move {
        let _ = builds_revision();
        let _ = (studio.revision)();
        let Some(project_id) = focus() else {
            overview.set(None);
            return;
        };
        if let Ok(fresh) = build_bridge::overview(project_id, false).await {
            overview.set(Some(fresh));
        }
    });

    use_resource(move || async move {
        let _ = (studio.revision)();
        if let Ok(fresh) = bridge::pinterest_status().await {
            pinterest.set(Some(fresh));
        }
    });

    let focused = focus();
    let snapshot = Snapshot {
        detail: detail
            .read()
            .clone()
            .filter(|loaded| Some(loaded.project.id) == focused),
        overview: overview
            .read()
            .clone()
            .filter(|loaded| Some(loaded.project_id) == focused),
        comfy: comfy.overview.read().clone(),
        pinterest: pinterest.read().clone(),
    };
    let loading = focused.is_some() && snapshot.detail.is_none();
    let typed = draft_prompt.read().clone();
    let planned_targets = studio.build_plan.read().clone();
    let graph = build_graph(
        &snapshot,
        &Draft {
            prompt: &typed,
            engine: (studio.draft_engine)(),
            targets: &planned_targets,
        },
    );

    let fallback = (focused.is_none() && !loading && !dismissed()).then_some(NodeId::Prompt);
    let shown = selected().or(fallback);
    let inspected = shown.and_then(|id| graph.node(id).cloned());
    let live = graph.live().cloned();
    let live_id = live.as_ref().map(|node| node.id);
    let idle_note = if loading {
        "Завантажую гру…"
    } else if snapshot.detail.is_none() {
        "Опиши гру у вузлі «Промпт гри»"
    } else {
        "Фабрика відпочиває: усе, що можна було зробити, зроблено"
    };

    let mut chosen = studio.selected;
    let mut start_new = move || {
        drafting.set(true);
        dismissed.set(false);
        selected.set(Some(NodeId::Prompt));
    };

    rsx! {
        div { class: "node-factory",
            header { class: "nf-rail", "data-tauri-drag-region": "true",
                div { class: "nf-brand",
                    LogoMark {}
                    span { class: "wordmark", "NOCTICS" }
                }
                ProjectPicker {
                    drafting: drafting(),
                    on_pick: move |project_id: i64| {
                        drafting.set(false);
                        chosen.set(Some(project_id));
                        selected.set(None);
                        dismissed.set(false);
                    },
                    on_new: move |_| start_new(),
                }
                Ticker {
                    live,
                    note: idle_note,
                    on_pick: move |id: NodeId| {
                        dismissed.set(false);
                        selected.set(Some(id));
                        reveal += 1;
                    },
                }
                ModeSwitch {}
            }
            div { class: "nf-floor",
                NodeCanvas {
                    graph: graph.clone(),
                    selected: inspected.as_ref().map(|node| node.id),
                    focus: live_id,
                    reveal: reveal(),
                    inspector_open: inspected.is_some(),
                    on_select: move |id: Option<NodeId>| {
                        dismissed.set(id.is_none());
                        selected.set(id);
                    },
                }
                if let Some(node) = inspected {
                    NodeInspector {
                        key: "{node.id.key()}",
                        node,
                        snapshot,
                        draft_prompt,
                        on_close: move |_| {
                            dismissed.set(true);
                            selected.set(None);
                        },
                        on_started: move |project_id: i64| {
                            drafting.set(false);
                            chosen.set(Some(project_id));
                            dismissed.set(false);
                            selected.set(Some(NodeId::Plan));
                        },
                        on_new: move |_| start_new(),
                    }
                }
            }
        }
    }
}

#[component]
fn ProjectPicker(drafting: bool, on_pick: EventHandler<i64>, on_new: EventHandler<()>) -> Element {
    let studio = use_studio();
    let mut open = use_signal(|| false);
    let projects = studio.projects.read().clone();
    let current = if drafting { None } else { studio.focused_project() };
    let current_id = current.as_ref().map(|project| project.id);
    let title = current
        .as_ref()
        .map_or_else(|| "Нова гра".to_owned(), |project| project.name.clone());

    rsx! {
        div {
            class: if open() { "nf-project is-open" } else { "nf-project" },
            onkeydown: move |event| {
                if event.key() == Key::Escape {
                    open.set(false);
                }
            },
            button {
                class: "nf-project-button",
                r#type: "button",
                title: "Вибрати гру",
                aria_haspopup: "menu",
                aria_expanded: if open() { "true" } else { "false" },
                onclick: move |_| open.set(!open()),
                FolderIcon {}
                span { class: "nf-project-name", "{title}" }
                ChevronDownIcon {}
            }
            if open() {
                div { class: "nf-backdrop", onclick: move |_| open.set(false) }
                div { class: "nf-project-panel", role: "menu", aria_label: "Ігри",
                    button {
                        class: if drafting { "nf-project-item is-current" } else { "nf-project-item" },
                        r#type: "button",
                        role: "menuitem",
                        onclick: move |_| {
                            open.set(false);
                            on_new.call(());
                        },
                        PlusIcon {}
                        span { class: "nf-project-item-name", "Нова гра" }
                    }
                    if !projects.is_empty() {
                        div { class: "nf-project-divider" }
                    }
                    for project in projects {
                        button {
                            key: "{project.id}",
                            class: if Some(project.id) == current_id { "nf-project-item is-current" } else { "nf-project-item" },
                            r#type: "button",
                            role: "menuitem",
                            title: "{project.name}: {project_status_label(project.status)}",
                            onclick: {
                                let project_id = project.id;
                                move |_| {
                                    open.set(false);
                                    on_pick.call(project_id);
                                }
                            },
                            span { class: "nf-led is-{project_phase(&project).code()}" }
                            span { class: "nf-project-item-name", "{project.name}" }
                            span { class: "nf-project-item-meta", "{project.approved_count}/{project.screen_count}" }
                        }
                    }
                }
            }
        }
    }
}

fn project_phase(project: &Project) -> Phase {
    match project.status {
        ProjectStatus::Failed => Phase::Failed,
        status if status.is_planning() || project.pending_count > 0 => Phase::Working,
        ProjectStatus::Ready => Phase::Done,
        _ => Phase::Review,
    }
}

#[component]
fn Ticker(live: Option<Node>, note: &'static str, on_pick: EventHandler<NodeId>) -> Element {
    let Some(node) = live else {
        return rsx! {
            button { class: "nf-ticker", r#type: "button", disabled: true,
                span { class: "nf-led" }
                span { class: "nf-ticker-text", "{note}" }
            }
        };
    };
    let id = node.id;
    let phase = node.phase;
    let since = node.since.filter(|_| phase == Phase::Working);

    rsx! {
        button {
            class: "nf-ticker",
            r#type: "button",
            title: "Показати вузол «{node.title}»",
            aria_live: "polite",
            onclick: move |_| on_pick.call(id),
            span { class: "nf-led is-{phase.code()}" }
            span { class: "nf-ticker-title", "{node.title}" }
            span { class: "nf-ticker-text", "{node.summary}" }
            if let Some(since) = since {
                Clock { since }
            }
        }
    }
}

#[component]
fn Clock(since: i64) -> Element {
    let mut now = use_signal(now_seconds);

    use_future(move || async move {
        loop {
            pause(TICK_MILLIS).await;
            now.set(now_seconds());
        }
    });

    rsx! {
        span { class: "nf-ticker-clock", title: "Від старту задачі", "{stopwatch(now() - since)}" }
    }
}

fn stopwatch(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let hours = seconds / SECONDS_PER_HOUR;
    let minutes = seconds % SECONDS_PER_HOUR / SECONDS_PER_MINUTE;
    let rest = seconds % SECONDS_PER_MINUTE;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{rest:02}")
    } else {
        format!("{minutes:02}:{rest:02}")
    }
}

fn now_seconds() -> i64 {
    (js_sys::Date::now() / MILLIS_PER_SECOND) as i64
}

async fn pause(millis: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, millis);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
