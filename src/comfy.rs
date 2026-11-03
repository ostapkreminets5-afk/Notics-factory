use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_logger::tracing;
use noctics_shared::{
    AssetDraft, AssetKind, AssetTask, COMFY_CHANGED_EVENT, ComfyError, ComfyModel, ComfyOverview,
    MAX_ASSET_BRIEF_CHARS, Medium, ModelRun, RosterSource, RunStatus, TaskOrigin, TaskStatus,
};

use crate::bridge;
use crate::comfy_bridge;
use crate::icons::{
    ArrowRightIcon, CheckIcon, CloseIcon, ExternalIcon, FolderIcon, RefreshIcon, TrashIcon,
};
use crate::studio::use_studio;

const COMFY_TOOL: &str = "comfyui";
const MILLIS_PER_SECOND: f64 = 1000.0;
const TICK_MILLIS: i32 = 1000;
const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_HOUR: i64 = 3600;
const SECONDS_PER_DAY: i64 = 86_400;
const JUST_NOW_SECONDS: i64 = 45;
const MEGABYTES_PER_GIGABYTE: u64 = 1024;

#[derive(Clone, Copy, PartialEq)]
pub struct Comfy {
    pub overview: Signal<Option<ComfyOverview>>,
    revision: Signal<u64>,
}

impl Comfy {
    pub fn changed(self) {
        let mut revision = self.revision;
        if let Ok(mut value) = revision.try_write() {
            *value += 1;
        }
    }

    pub fn replace(self, fresh: ComfyOverview) {
        let mut overview = self.overview;
        overview.set(Some(fresh));
    }

    pub fn busy(self) -> bool {
        self.overview
            .read()
            .as_ref()
            .is_some_and(ComfyOverview::busy)
    }
}

pub fn provide_comfy() -> Comfy {
    let comfy = use_context_provider(|| Comfy {
        overview: Signal::new(None),
        revision: Signal::new(0),
    });
    let subscription = use_hook(|| Rc::new(RefCell::new(None::<bridge::Subscription>)));

    use_future(move || {
        let slot = Rc::clone(&subscription);
        async move {
            let listening = bridge::listen(COMFY_CHANGED_EVENT, move || comfy.changed()).await;
            *slot.borrow_mut() = listening;
        }
    });

    let mut overview = comfy.overview;
    use_resource(move || async move {
        let _ = (comfy.revision)();
        if let Ok(fresh) = comfy_bridge::overview().await {
            overview.set(Some(fresh));
        }
    });

    comfy
}

pub fn use_comfy() -> Comfy {
    use_context::<Comfy>()
}

#[derive(Clone, PartialEq)]
struct Viewing {
    url: String,
    medium: Medium,
    title: String,
    model: String,
    run_id: i64,
    index: usize,
}

#[component]
pub fn ComfyPage() -> Element {
    let comfy = use_comfy();
    let mut notice = use_signal(|| None::<String>);
    let mut filter = use_signal(|| None::<AssetKind>);
    let mut viewing = use_signal(|| None::<Viewing>);
    let mut launching = use_signal(|| false);
    let mut now = use_signal(now_seconds);

    use_future(move || async move {
        loop {
            pause(TICK_MILLIS).await;
            if comfy.busy() {
                now.set(now_seconds());
            }
        }
    });

    let Some(overview) = comfy.overview.read().clone() else {
        return rsx! {
            section { class: "page comfy-page",
                PageHead {}
                div { class: "comfy-loading", span { class: "spinner" } "Шукаємо ComfyUI і моделі…" }
            }
        };
    };
    let status = overview.status.clone();

    let launch = move |_| {
        launching.set(true);
        spawn(async move {
            if let Err(problem) = comfy_bridge::launch().await {
                notice.set(Some(explain(&problem)));
            }
            launching.set(false);
        });
    };

    let open_comfy = move |_| {
        spawn(async move {
            if let Err(problem) = bridge::open_tool(COMFY_TOOL).await {
                notice.set(Some(crate::tools::explain_tool(&problem)));
            }
        });
    };

    let present: Vec<AssetKind> = AssetKind::ALL
        .into_iter()
        .filter(|kind| overview.tasks.iter().any(|task| task.kind == *kind))
        .collect();
    let active_filter = filter().filter(|kind| present.contains(kind));
    let visible: Vec<AssetTask> = overview
        .tasks
        .iter()
        .filter(|task| active_filter.is_none_or(|kind| task.kind == kind))
        .cloned()
        .collect();
    let base = status.media_base.clone();
    let tick = now();

    rsx! {
        if !base.is_empty() {
            document::Script { r#type: "module", src: "{base}/vendor/model-viewer.min.js" }
        }
        section { class: "page comfy-page",
            header { class: "page-head has-action",
                PageHead {}
                div { class: "head-actions",
                    StatusPill { status: status.clone() }
                    if status.launchable || status.launching {
                        button {
                            class: "primary-button",
                            r#type: "button",
                            disabled: launching() || status.launching,
                            onclick: launch,
                            if launching() || status.launching {
                                span { class: "spinner" }
                                "Запускаємо…"
                            } else {
                                "Запустити ComfyUI"
                            }
                        }
                    }
                    button { class: "ghost-button", r#type: "button", onclick: open_comfy,
                        "Відкрити ComfyUI"
                        ExternalIcon {}
                    }
                }
            }
            if let Some(message) = notice() {
                div { class: "page-alert", role: "alert",
                    span { "{message}" }
                    button {
                        class: "icon-button",
                        r#type: "button",
                        aria_label: "Сховати",
                        onclick: move |_| notice.set(None),
                        CloseIcon {}
                    }
                }
            }
            div { class: "comfy-top",
                Composer { overview: overview.clone(), on_notice: move |message| notice.set(Some(message)) }
                aside { class: "comfy-roster-panel",
                    ModelRoster { compact: false }
                }
            }
            div { class: "gallery-head",
                div { class: "gallery-title",
                    h2 { "Галерея" }
                    span { class: "mono-label", "Одна задача — кожна модель поруч" }
                }
                if present.len() > 1 {
                    div { class: "chip-row gallery-filter", role: "radiogroup", aria_label: "Тип ассета",
                        button {
                            class: if active_filter.is_none() { "chip is-active" } else { "chip" },
                            r#type: "button",
                            role: "radio",
                            aria_checked: if active_filter.is_none() { "true" } else { "false" },
                            onclick: move |_| filter.set(None),
                            "Усі"
                        }
                        for kind in present.iter().copied() {
                            button {
                                key: "{kind.code()}",
                                class: if active_filter == Some(kind) { "chip is-active" } else { "chip" },
                                r#type: "button",
                                role: "radio",
                                aria_checked: if active_filter == Some(kind) { "true" } else { "false" },
                                onclick: move |_| filter.set(Some(kind)),
                                "{kind.label()}"
                            }
                        }
                    }
                }
            }
            if visible.is_empty() {
                div { class: "page-empty",
                    p { class: "mono-label", "Галерея порожня" }
                    p {
                        "Опиши ассет і вибери типи вище — Claude напише промпт, а ComfyUI згенерує його кожною встановленою моделлю. Результати стануть поруч, щоб одразу вибрати кращий."
                    }
                }
            }
            div { class: "task-list",
                for task in visible {
                    TaskCard {
                        key: "{task.id}",
                        task: task.clone(),
                        now: tick,
                        on_view: move |view| viewing.set(Some(view)),
                        on_notice: move |message| notice.set(Some(message)),
                    }
                }
            }
            ApiNote { base: base.clone() }
        }
        if let Some(view) = viewing() {
            Lightbox {
                view,
                on_close: move |_| viewing.set(None),
                on_notice: move |message| notice.set(Some(message)),
            }
        }
    }
}

#[component]
fn PageHead() -> Element {
    rsx! {
        div { class: "page-head-text",
            p { class: "mono-label", "Система · Арт, 3D і звук" }
            h1 { class: "page-title", "ComfyUI" }
            p { class: "page-lead",
                "Одна задача — усі моделі поруч. Claude пише промпт, ComfyUI проганяє його через кожну встановлену модель."
            }
        }
    }
}

#[component]
fn StatusPill(status: noctics_shared::ComfyStatus) -> Element {
    let (tone, label) = if status.online {
        ("is-online", "Працює")
    } else if status.launching {
        ("is-busy", "Запускається")
    } else {
        ("is-offline", "Вимкнено")
    };
    let detail = status.online.then(|| {
        let mut parts = Vec::new();
        if let Some(gpu) = &status.gpu {
            parts.push(gpu.replace("NVIDIA GeForce ", ""));
        }
        if let Some(vram) = status.vram_mb {
            parts.push(format!("{} ГБ", (vram + MEGABYTES_PER_GIGABYTE / 2) / MEGABYTES_PER_GIGABYTE));
        }
        if let Some(version) = &status.version {
            parts.push(format!("v{version}"));
        }
        parts.join(" · ")
    });
    rsx! {
        div { class: "comfy-status {tone}", title: "{status.url}",
            span { class: "comfy-status-dot" }
            span { class: "comfy-status-label", "{label}" }
            if let Some(detail) = detail.filter(|text| !text.is_empty()) {
                span { class: "comfy-status-detail", "{detail}" }
            }
        }
    }
}

#[component]
fn Composer(overview: ComfyOverview, on_notice: EventHandler<String>) -> Element {
    let comfy = use_comfy();
    let studio = use_studio();
    let mut brief = use_signal(String::new);
    let mut kinds = use_signal(|| vec![AssetKind::Concept]);
    let mut styled = use_signal(|| true);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let project = studio
        .focused_project()
        .filter(|project| !project.status.is_planning());
    let project_id = project.as_ref().map(|project| project.id);
    let available: Vec<AssetKind> = AssetKind::ALL
        .into_iter()
        .filter(|kind| overview.runs_for(*kind) > 0)
        .collect();
    let chosen: Vec<AssetKind> = kinds()
        .into_iter()
        .filter(|kind| available.contains(kind))
        .collect();
    let total_runs: usize = chosen
        .iter()
        .map(|kind| overview.runs_for(*kind) + usize::from(*kind == AssetKind::Model))
        .sum();
    let length = brief.read().chars().count();
    let ready = !pending() && !chosen.is_empty() && !brief.read().trim().is_empty();

    let mut submit = move || {
        if pending() {
            return;
        }
        let draft = AssetDraft {
            brief: brief(),
            kinds: kinds(),
            project_id: if styled() { project_id } else { None },
        };
        if let Err(problem) = draft.normalized() {
            error.set(Some(explain(&problem)));
            return;
        }
        pending.set(true);
        error.set(None);
        spawn(async move {
            match comfy_bridge::create(&draft).await {
                Ok(fresh) => {
                    comfy.replace(fresh);
                    brief.set(String::new());
                }
                Err(problem) => error.set(Some(explain(&problem))),
            }
            pending.set(false);
        });
    };

    let select_all = {
        let available = available.clone();
        move |_| kinds.set(available.clone())
    };

    rsx! {
        form {
            class: "comfy-composer",
            novalidate: true,
            onsubmit: move |event| {
                event.prevent_default();
                submit();
            },
            div { class: "field-head",
                label { class: "mono-label", r#for: "asset-brief", "Задача для ComfyUI" }
                span { class: "mono-label", "{length} / {MAX_ASSET_BRIEF_CHARS}" }
            }
            textarea {
                class: "input is-area comfy-brief",
                id: "asset-brief",
                rows: "2",
                maxlength: "{MAX_ASSET_BRIEF_CHARS}",
                placeholder: "Наприклад: крижаний голем-охоронець печери, що світиться зсередини",
                value: "{brief}",
                oninput: move |event| brief.set(event.value()),
                onkeydown: move |event| {
                    if event.key() == Key::Enter && event.modifiers().ctrl() {
                        event.prevent_default();
                        submit();
                    }
                },
            }
            div { class: "kind-grid", role: "group", aria_label: "Типи ассетів",
                for kind in AssetKind::ALL {
                    {
                        let count = overview.runs_for(kind);
                        let selected = chosen.contains(&kind);
                        let class = match (selected, count == 0) {
                            (_, true) => "kind-option is-empty",
                            (true, false) => "kind-option is-active",
                            (false, false) => "kind-option",
                        };
                        rsx! {
                            button {
                                key: "{kind.code()}",
                                class,
                                r#type: "button",
                                aria_pressed: if selected { "true" } else { "false" },
                                disabled: count == 0,
                                title: if count == 0 { "Немає увімкнених моделей для цього типу" } else { kind.hint() },
                                onclick: move |_| {
                                    let mut list = kinds.write();
                                    if let Some(position) = list.iter().position(|item| *item == kind) {
                                        list.remove(position);
                                    } else {
                                        list.push(kind);
                                    }
                                },
                                span { class: "kind-name", "{kind.label()}" }
                                span { class: "kind-hint", "{kind.hint()}" }
                                span { class: "kind-count",
                                    if count == 0 { "немає моделей" } else { "{models_label(count)}" }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "composer-options",
                button { class: "ghost-button is-small", r#type: "button", onclick: select_all,
                    "Усе для гри"
                }
                button {
                    class: "ghost-button is-small",
                    r#type: "button",
                    onclick: move |_| kinds.set(Vec::new()),
                    "Очистити"
                }
                if let Some(project) = project.clone() {
                    label { class: "style-toggle",
                        input {
                            r#type: "checkbox",
                            checked: styled(),
                            onchange: move |event| styled.set(event.checked()),
                        }
                        span { "У стилі гри «{project.name}»" }
                    }
                }
            }
            if let Some(message) = error() {
                div { class: "composer-alert", role: "alert", span { "{message}" } }
            }
            div { class: "composer-foot",
                span { class: "mono-label",
                    if chosen.is_empty() {
                        "Вибери хоча б один тип"
                    } else {
                        "Claude → промпт → {runs_label(total_runs)} у ComfyUI"
                    }
                }
                button { class: "primary-button", r#type: "submit", disabled: !ready,
                    if pending() {
                        span { class: "spinner" }
                        "Ставимо в чергу…"
                    } else {
                        "Згенерувати всіма моделями"
                        ArrowRightIcon {}
                    }
                }
            }
        }
    }
}

#[component]
pub fn ModelRoster(compact: bool) -> Element {
    let comfy = use_comfy();
    let mut error = use_signal(|| None::<String>);
    let Some(overview) = comfy.overview.read().clone() else {
        return rsx! {
            div { class: "model-roster", p { class: "roster-empty", "Шукаємо моделі ComfyUI…" } }
        };
    };
    let enabled = overview.models.iter().filter(|model| model.usable()).count();
    let source = match overview.status.source {
        RosterSource::Server => "наживо з ComfyUI",
        RosterSource::Disk => "з диска · ComfyUI вимкнено",
        RosterSource::Unknown => "ComfyUI не знайдено",
    };

    let toggle = move |model: ComfyModel| {
        spawn(async move {
            match comfy_bridge::set_enabled(&model.id, !model.enabled).await {
                Ok(fresh) => {
                    comfy.replace(fresh);
                    error.set(None);
                }
                Err(problem) => error.set(Some(explain(&problem))),
            }
        });
    };

    rsx! {
        div { class: if compact { "model-roster is-compact" } else { "model-roster" },
            div { class: "roster-head",
                if !compact {
                    p { class: "mono-label", "Моделі ComfyUI · {enabled}/{overview.models.len()}" }
                }
                span { class: if overview.status.online { "roster-source is-live" } else { "roster-source" },
                    span { class: "roster-source-dot" }
                    "{source}"
                }
            }
            if overview.models.is_empty() {
                p { class: "roster-empty",
                    "Моделей не знайдено. Встанови їх у ComfyUI — список оновлюється сам кожні кілька секунд."
                }
            }
            for medium in Medium::ALL {
                {
                    let group: Vec<ComfyModel> = overview
                        .models
                        .iter()
                        .filter(|model| model.medium == medium)
                        .cloned()
                        .collect();
                    let on = group.iter().filter(|model| model.usable()).count();
                    rsx! {
                        if !group.is_empty() {
                            div { key: "{medium.label()}", class: "roster-group",
                                div { class: "roster-group-head",
                                    span { class: "roster-group-label", "{medium.label()}" }
                                    span { class: "roster-group-count", "{on}/{group.len()}" }
                                }
                                for model in group {
                                    ModelRow {
                                        key: "{model.id}",
                                        model: model.clone(),
                                        compact,
                                        on_toggle: move |picked| toggle(picked),
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !compact && !overview.models.is_empty() {
                p { class: "roster-hint", "Натисни на модель, щоб прибрати її з порівняння або повернути." }
            }
            if let Some(message) = error() {
                p { class: "tool-error", role: "alert", "{message}" }
            }
        }
    }
}

#[component]
fn ModelRow(model: ComfyModel, compact: bool, on_toggle: EventHandler<ComfyModel>) -> Element {
    let flipped = model.clone();
    let state = match (model.ready, model.enabled) {
        (false, _) => "is-missing",
        (true, false) => "is-off",
        (true, true) => "is-on",
    };
    let missing = format!("Бракує: {}", model.missing.join(", "));
    let hint = match (model.ready, model.enabled) {
        (false, _) => missing.clone(),
        (true, true) => format!("{} · {} — бере участь у порівнянні. Натисни, щоб вимкнути.", model.name, model.note),
        (true, false) => format!("{} · {} — вимкнено. Натисни, щоб увімкнути.", model.name, model.note),
    };
    let detail = if model.ready {
        (!compact).then(|| model.note.clone())
    } else {
        Some(missing)
    };
    rsx! {
        button {
            class: "model-row {state}",
            r#type: "button",
            role: "switch",
            aria_checked: if model.usable() { "true" } else { "false" },
            disabled: !model.ready,
            title: "{hint}",
            onclick: move |_| on_toggle.call(flipped.clone()),
            span { class: "model-check",
                if model.usable() {
                    CheckIcon {}
                } else if !model.ready {
                    "!"
                }
            }
            span { class: "model-text",
                span { class: "model-name", "{model.name}" }
                if let Some(detail) = detail {
                    span { class: "model-note", "{detail}" }
                }
            }
            if !model.ready {
                span { class: "model-tag is-warning", "неповна" }
            } else if let Some(precision) = &model.precision {
                span { class: "model-tag", "{precision}" }
            }
        }
    }
}

#[component]
fn TaskCard(task: AssetTask, now: i64, on_view: EventHandler<Viewing>, on_notice: EventHandler<String>) -> Element {
    let mut armed = use_signal(|| false);
    let task_id = task.id;
    let (status_label, status_tone) = task_status(task.status);
    let done = task.runs.iter().filter(|run| run.status == RunStatus::Done && !run.reference).count();
    let total = task.runs.iter().filter(|run| !run.reference).count();
    let medium_class = match task.kind.medium() {
        Medium::Image => "run-grid is-image",
        Medium::Mesh => "run-grid is-mesh",
        Medium::Audio => "run-grid is-audio",
    };

    let remove = move |_| {
        if !armed() {
            armed.set(true);
            return;
        }
        spawn(async move {
            if let Err(problem) = comfy_bridge::delete(task_id).await {
                on_notice.call(explain(&problem));
                armed.set(false);
            }
        });
    };

    rsx! {
        article { class: "task-card",
            header { class: "task-head",
                div { class: "task-heading",
                    div { class: "task-title-row",
                        span { class: "kind-badge", "{task.kind.label()}" }
                        h3 { class: "task-title", "{task.title}" }
                        span { class: "run-status {status_tone}", "{status_label}" }
                    }
                    p { class: "task-meta",
                        span { "{task.brief}" }
                        if let Some(name) = &task.project_name {
                            span { class: "task-meta-item", "Гра «{name}»" }
                        }
                        if task.origin == TaskOrigin::Api {
                            span { class: "task-meta-item is-api", "від Claude Code" }
                        }
                        span { class: "task-meta-item", "{done}/{total} моделей" }
                        span { class: "task-meta-item", "{ago(now, task.created_at)}" }
                    }
                }
                button {
                    class: if armed() { "icon-button is-danger is-armed" } else { "icon-button is-danger" },
                    r#type: "button",
                    title: if armed() { "Натисни ще раз, щоб видалити" } else { "Видалити задачу й файли" },
                    aria_label: "Видалити задачу «{task.title}»",
                    onclick: remove,
                    onmouseleave: move |_| armed.set(false),
                    TrashIcon {}
                }
            }
            if let Some(note) = &task.note {
                p { class: "task-note", "{note}" }
            }
            if !task.prompt.is_empty() {
                details { class: "task-prompt",
                    summary { "Промпт від Claude" }
                    p { "{task.prompt}" }
                }
            }
            div { class: medium_class,
                for run in task.runs.clone() {
                    RunCard {
                        key: "{run.id}",
                        run: run.clone(),
                        kind: task.kind,
                        title: task.title.clone(),
                        now,
                        on_view,
                        on_notice,
                    }
                }
            }
        }
    }
}

#[component]
fn RunCard(
    run: ModelRun,
    kind: AssetKind,
    title: String,
    now: i64,
    on_view: EventHandler<Viewing>,
    on_notice: EventHandler<String>,
) -> Element {
    let run_id = run.id;
    let (status_label, status_tone) = run_status(run.status);
    let file = run.files.first().cloned();
    let finished = run.status == RunStatus::Done && file.is_some();
    let card_class = format!(
        "run-card {status_tone}{}",
        if run.reference { " is-reference" } else { "" }
    );
    let elapsed = match (run.status, run.started_at, run.seconds()) {
        (RunStatus::Running, Some(started), _) => Some(stopwatch(now - started)),
        (_, _, Some(seconds)) => Some(duration(seconds)),
        _ => None,
    };
    let media_class = match (kind, run.reference) {
        (_, true) => "run-media is-plain",
        (AssetKind::Sprite | AssetKind::Interface, _) => "run-media is-checker",
        (AssetKind::Effect, _) => "run-media is-black",
        _ => "run-media is-plain",
    };

    let open = move |_| {
        spawn(async move {
            if let Err(problem) = comfy_bridge::open_file(run_id, 0).await {
                on_notice.call(explain(&problem));
            }
        });
    };
    let reveal = move |_| {
        spawn(async move {
            if let Err(problem) = comfy_bridge::reveal_file(run_id, 0).await {
                on_notice.call(explain(&problem));
            }
        });
    };
    let retry = move |_| {
        spawn(async move {
            if let Err(problem) = comfy_bridge::retry(run_id).await {
                on_notice.call(explain(&problem));
            }
        });
    };

    let media = match (&file, run.status) {
        (Some(file), RunStatus::Done) => {
            let view = Viewing {
                url: file.url.clone(),
                medium: file.medium,
                title: title.clone(),
                model: run.model_name.clone(),
                run_id,
                index: 0,
            };
            match file.medium {
                Medium::Image if kind.tiles() && !run.reference => rsx! {
                    button {
                        class: "run-tiles",
                        r#type: "button",
                        title: "Плитка 2×2 — видно шви. Натисни, щоб збільшити",
                        style: "background-image: url('{file.url}')",
                        onclick: move |_| on_view.call(view.clone()),
                    }
                },
                Medium::Image => rsx! {
                    button {
                        class: "run-image",
                        r#type: "button",
                        title: "Збільшити",
                        onclick: move |_| on_view.call(view.clone()),
                        img { src: "{file.url}", alt: "{run.model_name}: {title}", "loading": "lazy", draggable: "false" }
                    }
                },
                Medium::Mesh => rsx! {
                    div { class: "run-mesh", dangerous_inner_html: mesh_markup(&file.url, false) }
                    button {
                        class: "run-expand",
                        r#type: "button",
                        title: "Відкрити великим",
                        onclick: move |_| on_view.call(view.clone()),
                        "⤢"
                    }
                },
                Medium::Audio => rsx! {
                    div { class: "run-audio",
                        audio { controls: true, preload: "metadata", src: "{file.url}" }
                    }
                },
            }
        }
        (_, RunStatus::Running) => rsx! {
            div { class: "run-wait is-running",
                span { class: "spinner" }
                span { "Генерується" }
            }
        },
        (_, RunStatus::Queued) => rsx! {
            div { class: "run-wait", span { "У черзі" } }
        },
        _ => {
            let problem = run.error.clone().unwrap_or_else(|| status_label.to_owned());
            rsx! {
                div { class: "run-wait is-problem", span { "{problem}" } }
            }
        }
    };

    rsx! {
        div { class: "{card_class}",
            div { class: media_class, {media} }
            footer { class: "run-foot",
                div { class: "run-text",
                    span { class: "run-model",
                        "{run.model_name}"
                        if run.reference {
                            span { class: "run-tag", "Референс" }
                        }
                    }
                    span { class: "run-meta",
                        span { class: "run-status-dot {status_tone}" }
                        "{status_label}"
                        if let Some(time) = &elapsed {
                            " · {time}"
                        }
                    }
                }
                div { class: "run-actions",
                    if finished {
                        button { class: "icon-button", r#type: "button", title: "Відкрити файл", aria_label: "Відкрити файл", onclick: open,
                            ExternalIcon {}
                        }
                        button { class: "icon-button", r#type: "button", title: "Показати в папці", aria_label: "Показати в папці", onclick: reveal,
                            FolderIcon {}
                        }
                    }
                    if matches!(run.status, RunStatus::Done | RunStatus::Failed | RunStatus::Skipped) {
                        button { class: "icon-button", r#type: "button", title: "Згенерувати ще раз", aria_label: "Згенерувати ще раз", onclick: retry,
                            RefreshIcon {}
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Lightbox(view: Viewing, on_close: EventHandler<()>, on_notice: EventHandler<String>) -> Element {
    let run_id = view.run_id;
    let index = view.index;
    let open = move |_| {
        spawn(async move {
            if let Err(problem) = comfy_bridge::open_file(run_id, index).await {
                on_notice.call(explain(&problem));
            }
        });
    };
    let reveal = move |_| {
        spawn(async move {
            if let Err(problem) = comfy_bridge::reveal_file(run_id, index).await {
                on_notice.call(explain(&problem));
            }
        });
    };
    rsx! {
        div {
            class: "pin-viewer comfy-viewer",
            role: "dialog",
            aria_label: "{view.model}: {view.title}",
            tabindex: "-1",
            onclick: move |_| on_close.call(()),
            onkeydown: move |event| {
                if event.key() == Key::Escape {
                    on_close.call(());
                }
            },
            onmounted: move |event| async move {
                let _ = event.data().set_focus(true).await;
            },
            figure { class: "pin-viewer-frame comfy-viewer-frame", onclick: move |event| event.stop_propagation(),
                if view.medium == Medium::Mesh {
                    div { class: "comfy-viewer-mesh", dangerous_inner_html: mesh_markup(&view.url, true) }
                } else {
                    img { src: "{view.url}", alt: "{view.model}: {view.title}" }
                }
                figcaption { class: "pin-viewer-caption",
                    p { class: "mono-label", "{view.model}" }
                    p { class: "pin-viewer-title", "{view.title}" }
                    div { class: "pin-viewer-actions",
                        button { class: "ghost-button is-small", r#type: "button", onclick: open,
                            "Відкрити файл"
                            ExternalIcon {}
                        }
                        button { class: "ghost-button is-small", r#type: "button", onclick: reveal,
                            "Показати в папці"
                            FolderIcon {}
                        }
                        button { class: "ghost-button is-small", r#type: "button", onclick: move |_| on_close.call(()),
                            "Закрити"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ApiNote(base: String) -> Element {
    if base.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "pinterest-api comfy-api",
            p { class: "mono-label", "API для Claude Code" }
            p { class: "pinterest-api-text",
                "Будь-яка сесія Claude Code може сама поставити задачу в ComfyUI — результати всіх моделей з’являться тут, у галереї."
            }
            code { class: "pinterest-api-address", "POST {base}/tasks" }
            code { class: "pinterest-api-address",
                "{{\"kind\": \"sprite\", \"brief\": \"кристал мани\"}}"
            }
            p { class: "pinterest-api-hint",
                "Типи: concept, sprite, texture, effect, interface, model, music, sound або all. Довідка — GET {base}/"
            }
        }
    }
}

fn mesh_markup(url: &str, large: bool) -> String {
    let safe = url.replace(['"', '<', '>'], "");
    let rotate = if large { "" } else { " auto-rotate" };
    format!(
        "<model-viewer src=\"{safe}\" camera-controls{rotate} rotation-per-second=\"20deg\" interaction-prompt=\"none\" environment-image=\"neutral\" shadow-intensity=\"0.9\" exposure=\"1.05\" loading=\"lazy\" style=\"width:100%;height:100%;background:transparent\"></model-viewer>"
    )
}

fn task_status(status: TaskStatus) -> (&'static str, &'static str) {
    match status {
        TaskStatus::Writing => ("Claude пише промпт", "is-busy"),
        TaskStatus::Queued => ("У черзі", "is-queued"),
        TaskStatus::Running => ("Генерується", "is-busy"),
        TaskStatus::Done => ("Готово", "is-done"),
        TaskStatus::Failed => ("Помилка", "is-failed"),
    }
}

fn run_status(status: RunStatus) -> (&'static str, &'static str) {
    match status {
        RunStatus::Queued => ("У черзі", "is-queued"),
        RunStatus::Running => ("Генерується", "is-busy"),
        RunStatus::Done => ("Готово", "is-done"),
        RunStatus::Failed => ("Помилка", "is-failed"),
        RunStatus::Skipped => ("Пропущено", "is-skipped"),
    }
}

fn plural(count: usize, one: &str, few: &str, many: &str) -> String {
    let tens = count % 100;
    let units = count % 10;
    let word = if (11..=14).contains(&tens) {
        many
    } else if units == 1 {
        one
    } else if (2..=4).contains(&units) {
        few
    } else {
        many
    };
    format!("{count} {word}")
}

fn models_label(count: usize) -> String {
    plural(count, "модель", "моделі", "моделей")
}

fn runs_label(count: usize) -> String {
    plural(count, "запуск", "запуски", "запусків")
}

fn ago(now: i64, then: i64) -> String {
    let seconds = (now - then).max(0);
    if seconds < JUST_NOW_SECONDS {
        "щойно".to_owned()
    } else if seconds < SECONDS_PER_HOUR {
        format!("{} хв тому", (seconds / SECONDS_PER_MINUTE).max(1))
    } else if seconds < SECONDS_PER_DAY {
        format!("{} год тому", seconds / SECONDS_PER_HOUR)
    } else {
        format!("{} дн тому", seconds / SECONDS_PER_DAY)
    }
}

fn duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds < SECONDS_PER_MINUTE {
        format!("{seconds} с")
    } else {
        format!("{} хв {:02} с", seconds / SECONDS_PER_MINUTE, seconds % SECONDS_PER_MINUTE)
    }
}

fn stopwatch(seconds: i64) -> String {
    let seconds = seconds.max(0);
    format!("{:02}:{:02}", seconds / SECONDS_PER_MINUTE, seconds % SECONDS_PER_MINUTE)
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

pub fn explain(problem: &ComfyError) -> String {
    match problem {
        ComfyError::SignedOut => "Сесія завершилась — увійди знову.".to_owned(),
        ComfyError::BriefMissing => "Опиши, що згенерувати, — хоча б 3 символи.".to_owned(),
        ComfyError::BriefTooLong => format!("Опис задовгий — максимум {MAX_ASSET_BRIEF_CHARS} символів."),
        ComfyError::NoKinds => "Обери хоча б один тип ассета.".to_owned(),
        ComfyError::NoModels => "Для вибраних типів немає увімкнених моделей у ComfyUI.".to_owned(),
        ComfyError::NotFound => "Цієї задачі чи файлу вже немає.".to_owned(),
        ComfyError::NotInstalled => {
            "Не знайдено локальний ComfyUI. Запусти його сам — Noctics підхопить його автоматично.".to_owned()
        }
        ComfyError::Launch(detail) => format!("ComfyUI не запустився: {detail}"),
        ComfyError::Storage(detail) => {
            tracing::error!("comfy gallery failed: {detail}");
            "Не вдалося звернутися до галереї ComfyUI.".to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_in_ukrainian() {
        assert_eq!(models_label(1), "1 модель");
        assert_eq!(models_label(3), "3 моделі");
        assert_eq!(models_label(5), "5 моделей");
        assert_eq!(models_label(12), "12 моделей");
        assert_eq!(models_label(21), "21 модель");
        assert_eq!(runs_label(22), "22 запуски");
    }

    #[test]
    fn formats_times() {
        assert_eq!(ago(1000, 990), "щойно");
        assert_eq!(ago(10_000, 10_000 - 300), "5 хв тому");
        assert_eq!(duration(9), "9 с");
        assert_eq!(duration(125), "2 хв 05 с");
        assert_eq!(stopwatch(75), "01:15");
    }
}
