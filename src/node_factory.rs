use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use noctics_shared::{BuildOverview, MotionStatus, PinterestStatus, Project, ProjectDetail, ProjectStatus, SourceStatus};

use crate::asset_hunt::use_asset_hunt;
use crate::build::explain_build;
use crate::comfy::use_comfy;
use crate::factory_mode::ModeSwitch;
use crate::mixamo::{explain_mixamo, use_mixamo};
use crate::icons::{ChevronDownIcon, FolderIcon, LogoMark, PlusIcon, RefreshIcon};
use crate::inspector_layout::provide_inspector_layout;
use crate::node_canvas::NodeCanvas;
use crate::node_graph::{Draft, Graph, Node, NodeId, Phase, Snapshot, build_graph};
use crate::node_inspector::NodeInspector;
use crate::open_design::use_open_design;
use crate::studio::{explain, project_status_label, use_studio};
use crate::{bridge, build_bridge, mixamo_bridge};

const BUILDS_CHANGED: &str = "builds-changed";
const TICK_MILLIS: i32 = 1000;
const MILLIS_PER_SECOND: f64 = 1000.0;
const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_HOUR: i64 = 3600;

#[derive(Clone, Copy, PartialEq)]
enum Restart {
    Studio,
    Code,
    Motions,
}

#[component]
pub fn NodeFactory() -> Element {
    let studio = use_studio();
    let comfy = use_comfy();
    let mixamo = use_mixamo();
    provide_inspector_layout();
    let mut drafting = use_signal(|| false);
    let mut selected = use_signal(|| None::<NodeId>);
    let mut dismissed = use_signal(|| false);
    let mut reveal = use_signal(|| 0_u64);
    let draft_prompt = use_signal(String::new);
    let mut detail = use_signal(|| None::<ProjectDetail>);
    let mut overview = use_signal(|| None::<BuildOverview>);
    let mut pinterest = use_signal(|| None::<PinterestStatus>);
    let mut builds_revision = use_signal(|| 0_u64);
    let mut resuming = use_signal(|| false);
    let mut resume_problem = use_signal(|| None::<(i64, String)>);
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

    let hunt = use_asset_hunt(focus);
    let open_design = use_open_design(focus);

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
        hunt: hunt
            .read()
            .clone()
            .filter(|loaded| Some(loaded.project_id) == focused),
        mixamo: mixamo.overview.read().clone(),
        open_design: open_design
            .read()
            .clone()
            .filter(|loaded| Some(loaded.project_id) == focused),
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

    let code_broken = snapshot
        .overview
        .as_ref()
        .and_then(|loaded| loaded.source.as_ref())
        .is_some_and(|source| source.status == SourceStatus::Failed);
    let stalled = stalled_nodes(&graph, code_broken);
    let stalled_names = stalled
        .iter()
        .filter_map(|id| graph.node(*id))
        .map(|node| format!("«{}»", node.title))
        .collect::<Vec<_>>()
        .join(", ");
    let problem = resume_problem()
        .filter(|(project_id, _)| Some(*project_id) == focused)
        .map(|(_, text)| text);

    let mut chosen = studio.selected;
    let mut start_new = move || {
        drafting.set(true);
        dismissed.set(false);
        selected.set(Some(NodeId::Prompt));
    };
    let mut resume = move |ids: Vec<NodeId>| {
        let Some(project_id) = focused else {
            return;
        };
        if *resuming.peek() {
            return;
        }
        let part = |kind: Restart| ids.iter().any(|id| restart_of(*id) == Some(kind));
        let parts = Parts {
            studio: part(Restart::Studio),
            code: part(Restart::Code),
            motions: part(Restart::Motions),
        };
        resuming.set(true);
        resume_problem.set(None);
        spawn(async move {
            let outcome = restart(project_id, parts).await;
            studio.changed();
            resume_problem.set(outcome.err().map(|text| (project_id, text)));
            resuming.set(false);
        });
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
                div { class: "nf-status",
                    Ticker {
                        live,
                        note: idle_note,
                        on_pick: move |id: NodeId| {
                            dismissed.set(false);
                            selected.set(Some(id));
                            reveal += 1;
                        },
                    }
                    if !stalled.is_empty() {
                        button {
                            class: "nf-button is-primary nf-resume",
                            r#type: "button",
                            title: "Запустити знову з місця, де зупинились: {stalled_names}",
                            disabled: resuming(),
                            onclick: {
                                let ids = stalled.clone();
                                move |_| resume(ids.clone())
                            },
                            if resuming() {
                                span { class: "nf-spinner" }
                            } else {
                                RefreshIcon {}
                            }
                            "Продовжити"
                        }
                    }
                    if let Some(problem) = problem {
                        span { class: "nf-resume-problem", role: "alert", title: "{problem}", "{problem}" }
                    }
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
                    stalled: stalled.clone(),
                    resuming: resuming(),
                    on_select: move |id: Option<NodeId>| {
                        dismissed.set(id.is_none());
                        selected.set(id);
                    },
                    on_resume: move |id: NodeId| resume(vec![id]),
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

#[derive(Clone, Copy)]
struct Parts {
    studio: bool,
    code: bool,
    motions: bool,
}

fn restart_of(id: NodeId) -> Option<Restart> {
    match id {
        NodeId::Plan | NodeId::Foundation | NodeId::Skills | NodeId::Screens => Some(Restart::Studio),
        NodeId::Code => Some(Restart::Code),
        NodeId::Animations => Some(Restart::Motions),
        _ => None,
    }
}

fn stalled_nodes(graph: &Graph, code_broken: bool) -> Vec<NodeId> {
    graph
        .nodes
        .iter()
        .filter(|node| node.phase == Phase::Failed)
        .map(|node| node.id)
        .filter(|id| match restart_of(*id) {
            Some(Restart::Studio | Restart::Motions) => true,
            Some(Restart::Code) => code_broken,
            None => false,
        })
        .collect()
}

async fn restart(project_id: i64, parts: Parts) -> Result<(), String> {
    if parts.studio {
        bridge::sync_project(project_id)
            .await
            .map_err(|problem| explain(&problem))?;
    }
    if parts.code {
        build_bridge::generate(project_id, None)
            .await
            .map_err(|problem| explain_build(&problem))?;
    }
    if parts.motions {
        retry_failed_motions(project_id).await?;
    }
    Ok(())
}

async fn retry_failed_motions(project_id: i64) -> Result<(), String> {
    let overview = mixamo_bridge::overview()
        .await
        .map_err(|problem| explain_mixamo(&problem))?;
    let failed: Vec<i64> = overview
        .for_project(project_id)
        .into_iter()
        .filter(|order| order.status == MotionStatus::Failed)
        .map(|order| order.id)
        .collect();
    for id in failed {
        mixamo_bridge::retry(id)
            .await
            .map_err(|problem| explain_mixamo(&problem))?;
    }
    Ok(())
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

pub(crate) fn stopwatch(seconds: i64) -> String {
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

pub(crate) fn now_seconds() -> i64 {
    (js_sys::Date::now() / MILLIS_PER_SECOND) as i64
}

pub(crate) async fn pause(millis: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, millis);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
