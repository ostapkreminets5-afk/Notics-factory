use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_logger::tracing;
use noctics_shared::{
    GameEngine, MAX_MOTION_NAME_CHARS, MAX_MOTION_QUERY_CHARS, MIXAMO_API_PORT, MIXAMO_CHANGED_EVENT,
    MOTION_FPS_CHOICES, MixamoError, MixamoOverview, MixamoStatus, MotionClip, MotionDraft, MotionOptions, MotionOrder,
    MotionStatus, engine_takes_motions, format_size,
};

use crate::bridge;
use crate::icons::{ArrowRightIcon, FolderIcon, PlusIcon, RefreshIcon, SearchIcon, TrashIcon};
use crate::journal::plural;
use crate::mixamo_bridge;
use crate::node_graph::motion_phase;
use crate::studio::use_studio;

const ORDER_WORDS: [&str; 3] = ["анімація", "анімації", "анімацій"];
const RESULT_WORDS: [&str; 3] = ["знахідка", "знахідки", "знахідок"];
const INSPECTOR_RESULTS: usize = 8;

type Task = Pin<Box<dyn Future<Output = Result<(), MixamoError>>>>;

fn task(future: impl Future<Output = Result<(), MixamoError>> + 'static) -> Task {
    Box::pin(future)
}

#[derive(Clone, Copy, PartialEq)]
pub struct Mixamo {
    pub overview: Signal<Option<MixamoOverview>>,
    revision: Signal<u64>,
}

impl Mixamo {
    pub fn changed(self) {
        let mut revision = self.revision;
        if let Ok(mut value) = revision.try_write() {
            *value += 1;
        }
    }

    pub fn status(self) -> MixamoStatus {
        self.overview
            .read()
            .as_ref()
            .map(|overview| overview.status.clone())
            .unwrap_or_default()
    }

    pub fn busy(self) -> bool {
        self.overview.read().as_ref().is_some_and(MixamoOverview::busy)
    }
}

pub fn provide_mixamo() -> Mixamo {
    let mixamo = use_context_provider(|| Mixamo {
        overview: Signal::new(None),
        revision: Signal::new(0),
    });
    let subscription = use_hook(|| Rc::new(RefCell::new(None::<bridge::Subscription>)));

    use_future(move || {
        let slot = Rc::clone(&subscription);
        async move {
            let listening = bridge::listen(MIXAMO_CHANGED_EVENT, move || mixamo.changed()).await;
            *slot.borrow_mut() = listening;
        }
    });

    let mut overview = mixamo.overview;
    use_resource(move || async move {
        let _ = (mixamo.revision)();
        if let Ok(fresh) = mixamo_bridge::overview().await {
            overview.set(Some(fresh));
        }
    });

    mixamo
}

pub fn use_mixamo() -> Mixamo {
    use_context::<Mixamo>()
}

pub fn clip_draft(clip: &MotionClip, query: &str, project_id: Option<i64>, options: MotionOptions) -> MotionDraft {
    let label = if clip.name.trim().is_empty() { clip.title() } else { clip.name.as_str() };
    MotionDraft {
        name: label.chars().take(MAX_MOTION_NAME_CHARS).collect(),
        query: query.chars().take(MAX_MOTION_QUERY_CHARS).collect(),
        clip_id: Some(clip.id.clone()),
        project_id,
        options,
    }
}

pub fn file_name(order: &MotionOrder) -> Option<String> {
    order
        .file
        .as_deref()
        .and_then(|path| path.rsplit(['\\', '/']).next())
        .map(str::to_owned)
}

#[component]
pub fn MixamoPage() -> Element {
    let studio = use_studio();
    let mixamo = use_mixamo();
    let mut notice = use_signal(|| None::<String>);
    let mut query = use_signal(String::new);
    let mut options = use_signal(MotionOptions::default);
    let mut results = use_signal(|| None::<(String, Vec<MotionClip>)>);
    let mut busy = use_signal(|| None::<String>);

    let project = studio.focused_project();
    let project_id = project.as_ref().map(|project| project.id);
    let status = mixamo.status();
    let overview = mixamo.overview.read().clone();
    let orders: Vec<MotionOrder> = overview
        .as_ref()
        .map(|overview| match project_id {
            Some(id) => overview.for_project(id).into_iter().cloned().collect(),
            None => overview.orders.clone(),
        })
        .unwrap_or_default();
    let flat_engine = project
        .as_ref()
        .map(|project| project.engine)
        .filter(|engine| !engine_takes_motions(*engine));
    let connected = status.signed_in == Some(true);

    let mut run = move |(key, work): (String, Task)| {
        if busy.peek().is_some() {
            return;
        }
        busy.set(Some(key));
        notice.set(None);
        spawn(async move {
            if let Err(problem) = work.await {
                notice.set(Some(explain_mixamo(&problem)));
            }
            mixamo.changed();
            busy.set(None);
        });
    };

    let mut search = move || {
        let text = query.peek().trim().to_owned();
        if text.is_empty() {
            return;
        }
        run((
            "search".to_owned(),
            task(async move {
                let found = mixamo_bridge::search(&text).await?;
                results.set(Some((text, found)));
                Ok(())
            }),
        ));
    };

    let order_best = move |_| {
        let text = query.peek().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let draft = MotionDraft {
            name: text.chars().take(MAX_MOTION_NAME_CHARS).collect(),
            query: text,
            clip_id: None,
            project_id,
            options: *options.peek(),
        };
        run(("best".to_owned(), task(async move { mixamo_bridge::order(&draft).await.map(drop) })));
    };

    let lead = match &project {
        Some(project) => format!(
            "Анімації з mixamo.com для «{}». Claude-агент замовляє їх, коли пише код гри, а Noctics знаходить найближчі, запікає й кладе FBX.",
            project.name
        ),
        None => "Анімації з mixamo.com: пошук, найближчий збіг, запікання з налаштуваннями й FBX для гри.".to_owned(),
    };
    let searching = busy.read().as_deref() == Some("search");
    let planning = busy.read().as_deref() == Some("plan");
    let locked = busy.read().is_some();
    let typed = !query.read().trim().is_empty();
    let chosen = options();

    rsx! {
        section { class: "page mixamo-page",
            header { class: "page-head has-action",
                div { class: "page-head-text",
                    p { class: "mono-label", "Система · Анімації персонажів" }
                    h1 { class: "page-title", "Mixamo" }
                    p { class: "page-lead", "{lead}" }
                }
                div { class: "head-actions",
                    MixamoBadge { status: status.clone() }
                    if let Some(project_id) = project_id {
                        button {
                            class: "ghost-button",
                            r#type: "button",
                            title: "Розібрати дизайн-документ гри й поставити в чергу потрібні анімації",
                            disabled: locked,
                            onclick: move |_| {
                                run(("plan".to_owned(), task(async move { mixamo_bridge::plan(project_id).await.map(drop) })))
                            },
                            if planning {
                                span { class: "spinner" }
                                "Підбираємо…"
                            } else {
                                "Підібрати під гру"
                            }
                        }
                    }
                }
            }
            if let Some(message) = notice() {
                div { class: "page-alert", role: "alert", span { "{message}" } }
            }
            if !connected {
                div { class: "pinterest-link",
                    div { class: "pinterest-link-text",
                        p { class: "pinterest-link-title",
                            if status.signed_in.is_none() { "Перевіряю сесію Mixamo…" } else { "Підключи свій Mixamo" }
                        }
                        p {
                            "Увійди один раз в Adobe у вікні mixamo.com — сесія збережеться в Noctics. Далі пошук, запікання й завантаження FBX ідуть самі, а замовлення чекають у черзі, поки ти не ввійдеш."
                        }
                    }
                    button {
                        class: "primary-button",
                        r#type: "button",
                        onclick: move |_| run(("login".to_owned(), task(mixamo_bridge::login()))),
                        "Увійти в Mixamo"
                        ArrowRightIcon {}
                    }
                }
            }
            if let Some(engine) = flat_engine {
                div { class: "page-alert is-info",
                    span {
                        "{engine.label()} — 2D-рушій, тож агент не бере анімації скелета. Замовити їх вручну все одно можна."
                    }
                }
            }
            form {
                class: "motion-finder",
                novalidate: true,
                onsubmit: move |event| {
                    event.prevent_default();
                    search();
                },
                div { class: "motion-finder-row",
                    input {
                        class: "input",
                        r#type: "search",
                        maxlength: "{MAX_MOTION_QUERY_CHARS}",
                        placeholder: "Що шукати на Mixamo англійською: sword slash, crouch walk, dying…",
                        aria_label: "Пошук анімацій Mixamo",
                        value: "{query}",
                        oninput: move |event| query.set(event.value()),
                    }
                    button {
                        class: "ghost-button",
                        r#type: "submit",
                        disabled: locked || !typed,
                        if searching {
                            span { class: "spinner" }
                        } else {
                            SearchIcon {}
                        }
                        "Знайти"
                    }
                    button {
                        class: "primary-button",
                        r#type: "button",
                        title: "Noctics сам обере найближчу анімацію до запиту",
                        disabled: locked || !typed,
                        onclick: order_best,
                        PlusIcon {}
                        "Замовити найближчу"
                    }
                }
                div { class: "motion-options", role: "group", aria_label: "Як запекти анімацію",
                    span { class: "mono-label", "Запікання" }
                    OptionChip {
                        label: "На місці",
                        hint: "Без руху вперед — персонажа рухає код гри",
                        on: chosen.in_place,
                        onclick: move |_| options.with_mut(|value| value.in_place = !value.in_place),
                    }
                    OptionChip {
                        label: "Дзеркально",
                        hint: "Віддзеркалити ліву й праву сторону",
                        on: chosen.mirror,
                        onclick: move |_| options.with_mut(|value| value.mirror = !value.mirror),
                    }
                    OptionChip {
                        label: "Зі скіном",
                        hint: "FBX разом із моделлю персонажа Mixamo",
                        on: chosen.skin,
                        onclick: move |_| options.with_mut(|value| value.skin = !value.skin),
                    }
                    span { class: "motion-options-gap" }
                    for fps in MOTION_FPS_CHOICES {
                        OptionChip {
                            key: "{fps}",
                            label: "{fps} fps",
                            hint: "Кадрів на секунду в FBX",
                            on: chosen.fps == fps,
                            onclick: move |_| options.with_mut(|value| value.fps = fps),
                        }
                    }
                }
                p { class: "motion-finder-hint",
                    "Знайдені анімації відсортовано за схожістю з запитом. Повтор того самого замовлення не створює дубль."
                }
            }
            if let Some((searched, found)) = results() {
                section { class: "motion-section", aria_label: "Знайдено на Mixamo",
                    div { class: "motion-section-head",
                        p { class: "mono-label", "Mixamo · «{searched}» · {plural(found.len() as u32, RESULT_WORDS)}" }
                    }
                    if found.is_empty() {
                        div { class: "page-empty",
                            p { class: "mono-label", "Нічого не знайдено" }
                            p { "Спробуй коротший запит англійською: run, idle, punch, climb." }
                        }
                    } else {
                        div { class: "motion-grid",
                            for clip in found {
                                {
                                    let key = format!("pick-{}", clip.id);
                                    let draft = clip_draft(&clip, &searched, project_id, chosen);
                                    let pending = busy.read().as_deref() == Some(key.as_str());
                                    rsx! {
                                        article { key: "{clip.id}", class: "motion-card",
                                            div { class: "motion-preview",
                                                if clip.preview().is_empty() {
                                                    span { class: "motion-preview-empty", "без прев’ю" }
                                                } else {
                                                    img {
                                                        src: "{clip.preview()}",
                                                        alt: "{clip.title()}",
                                                        loading: "lazy",
                                                        referrerpolicy: "no-referrer",
                                                        draggable: "false",
                                                    }
                                                }
                                            }
                                            div { class: "motion-body",
                                                p { class: "motion-title", title: "{clip.name}", "{clip.name}" }
                                                if !clip.description.is_empty() && clip.description != clip.name {
                                                    p { class: "motion-note", "{clip.description}" }
                                                }
                                                div { class: "motion-actions",
                                                    button {
                                                        class: "primary-button is-small",
                                                        r#type: "button",
                                                        disabled: locked,
                                                        onclick: move |_| {
                                                            let draft = draft.clone();
                                                            run((key.clone(), task(async move { mixamo_bridge::order(&draft).await.map(drop) })))
                                                        },
                                                        if pending {
                                                            span { class: "spinner" }
                                                        } else {
                                                            PlusIcon {}
                                                        }
                                                        if project_id.is_some() { "Додати в гру" } else { "Замовити" }
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
            }
            section { class: "motion-section", aria_label: "Замовлені анімації",
                div { class: "motion-section-head",
                    p { class: "mono-label",
                        match &project {
                            Some(project) => format!("Анімації «{}» · {}", project.name, plural(orders.len() as u32, ORDER_WORDS)),
                            None => format!("Усі анімації · {}", plural(orders.len() as u32, ORDER_WORDS)),
                        }
                    }
                    if let Some(character) = status.character.clone() {
                        span { class: "motion-section-note", "Скелет персонажа Mixamo: {character}" }
                    }
                }
                if orders.is_empty() {
                    div { class: "page-empty",
                        p { class: "mono-label", "Ще немає анімацій" }
                        p {
                            "Натисни «Підібрати під гру» — Noctics розбере дизайн-документ і замовить idle, ходьбу, біг, стрибок, удари й решту під жанр. Або знайди потрібну вище. Claude-агент теж замовляє їх сам, коли пише код."
                        }
                    }
                } else {
                    div { class: "motion-grid",
                        for order in orders {
                            MotionOrderCard { key: "{order.id}", order, locked, on_run: run }
                        }
                    }
                }
            }
            LocalMixamoApi {}
        }
    }
}

#[component]
fn OptionChip(label: String, hint: &'static str, on: bool, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button {
            class: if on { "chip is-active" } else { "chip" },
            r#type: "button",
            title: hint,
            aria_pressed: if on { "true" } else { "false" },
            onclick: move |event| onclick.call(event),
            "{label}"
        }
    }
}

#[component]
fn MotionOrderCard(
    order: MotionOrder,
    locked: bool,
    on_run: EventHandler<(String, Task)>,
) -> Element {
    let id = order.id;
    let status = order.status;
    let clip = order.clip.clone().unwrap_or_default();
    let preview = clip.preview().to_owned();
    let note = if clip.id.is_empty() || clip.title().is_empty() {
        format!("Шукає «{}»", order.query)
    } else {
        clip.title().to_owned()
    };
    let card_class = match status {
        MotionStatus::Failed => "motion-card is-failed",
        state if state.is_active() => "motion-card is-working",
        _ => "motion-card",
    };
    let file = file_name(&order);
    let size = order.size_bytes.map(format_size);
    let finished = matches!(status, MotionStatus::Done | MotionStatus::Failed);

    rsx! {
        article { class: card_class,
            div { class: "motion-preview",
                if preview.is_empty() {
                    span { class: "motion-preview-empty", "{status.label()}" }
                } else {
                    img {
                        src: "{preview}",
                        alt: "{note}",
                        loading: "lazy",
                        referrerpolicy: "no-referrer",
                        draggable: "false",
                    }
                }
                span { class: "motion-state is-{status.code()}", "{status.label()}" }
            }
            div { class: "motion-body",
                p { class: "motion-title", title: "{order.name}", "{order.name}" }
                p { class: "motion-note", title: "{note}", "{note}" }
                div { class: "motion-tags",
                    for tag in order.options.tags() {
                        span { key: "{tag}", class: "motion-tag", "{tag}" }
                    }
                    if let Some(file) = &file {
                        span { class: "motion-tag is-file", title: "{file}",
                            "{file}"
                            if let Some(size) = &size {
                                " · {size}"
                            }
                        }
                    }
                }
                if let Some(problem) = &order.error {
                    p { class: "motion-error", "{problem}" }
                }
                div { class: "motion-actions",
                    if status == MotionStatus::Done {
                        button {
                            class: "ghost-button is-small",
                            r#type: "button",
                            onclick: move |_| on_run.call((format!("reveal-{id}"), task(mixamo_bridge::reveal(id)))),
                            FolderIcon {}
                            "Показати FBX"
                        }
                    }
                    if finished {
                        button {
                            class: "ghost-button is-small",
                            r#type: "button",
                            title: "Запекти й завантажити ще раз",
                            disabled: locked,
                            onclick: move |_| on_run.call((format!("retry-{id}"), task(mixamo_bridge::retry(id)))),
                            RefreshIcon {}
                            "Ще раз"
                        }
                    }
                    if !status.is_active() {
                        button {
                            class: "ghost-button is-small",
                            r#type: "button",
                            title: "Прибрати замовлення і файл",
                            aria_label: "Видалити «{order.name}»",
                            disabled: locked,
                            onclick: move |_| on_run.call((format!("delete-{id}"), task(mixamo_bridge::delete(id)))),
                            TrashIcon {}
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn MixamoBadge(status: MixamoStatus) -> Element {
    let (class, label, detail) = match (status.signed_in, status.working) {
        (Some(true), true) => ("mixamo-status is-busy", "Запікає анімацію", status.character.clone()),
        (Some(true), false) => ("mixamo-status is-online", "Mixamo підключено", status.character.clone()),
        (Some(false), _) => ("mixamo-status is-off", "Не підключено", None),
        (None, _) => ("mixamo-status", "Перевіряю вхід", None),
    };
    rsx! {
        span { class,
            span { class: "mixamo-status-dot" }
            span { class: "mixamo-status-label", "{label}" }
            if let Some(detail) = detail {
                span { class: "mixamo-status-detail", "{detail}" }
            }
        }
    }
}

#[component]
fn LocalMixamoApi() -> Element {
    let address = format!("http://127.0.0.1:{MIXAMO_API_PORT}");
    rsx! {
        section { class: "local-api motion-section", aria_label: "Локальний Mixamo API",
            p { class: "mono-label", "Mixamo API для Claude-агента · локально" }
            p { class: "local-api-text",
                "Через нього Claude-агент замовляє анімації, коли пише код гри: шукає, бере найближчу, чекає запікання й копіює FBX у гру. Твої скрипти теж можуть ним користуватися."
            }
            code { class: "local-api-address", "{address}/order?name=run&q=running&in_place=1&project_id=1" }
            p { class: "local-api-hint",
                "Довідка: {address}/ · пошук: /search?q=… · набір під гру: /plan?project_id=… · стан: /animations/<id>. Лише запити з цього комп’ютера."
            }
        }
    }
}

#[component]
pub fn AnimationsPanel(project_id: i64, engine: GameEngine, orders: Vec<MotionOrder>, status: MixamoStatus) -> Element {
    let mixamo = use_mixamo();
    let mut busy = use_signal(|| None::<String>);
    let mut notice = use_signal(|| None::<String>);
    let mut query = use_signal(String::new);
    let mut in_place = use_signal(|| true);
    let mut results = use_signal(|| None::<(String, Vec<MotionClip>)>);

    let mut run = move |(key, work): (String, Task)| {
        if busy.peek().is_some() {
            return;
        }
        busy.set(Some(key));
        notice.set(None);
        spawn(async move {
            if let Err(problem) = work.await {
                notice.set(Some(explain_mixamo(&problem)));
            }
            mixamo.changed();
            busy.set(None);
        });
    };
    let mut search = move || {
        let text = query.peek().trim().to_owned();
        if text.is_empty() {
            return;
        }
        run((
            "search".to_owned(),
            task(async move {
                let found = mixamo_bridge::search(&text).await?;
                results.set(Some((text, found)));
                Ok(())
            }),
        ));
    };

    let connected = status.signed_in == Some(true);
    let locked = busy.read().is_some();
    let pending = |key: &str| busy.read().as_deref() == Some(key);
    let options = MotionOptions {
        in_place: in_place(),
        ..MotionOptions::default()
    };

    rsx! {
        if let Some(text) = notice() {
            p { class: "nf-notice", role: "alert", "{text}" }
        }
        if !engine_takes_motions(engine) {
            p { class: "nf-inspector-lead",
                "{engine.label()} — 2D-рушій, тож агент не бере анімації скелета. Замовити вручну все одно можна."
            }
        }
        if !connected {
            p { class: "nf-inspector-lead",
                "Mixamo не підключено. Увійди один раз в Adobe — і агент сам братиме анімації, коли писатиме код."
            }
        }
        div { class: "nf-actions",
            if !connected {
                button {
                    class: "nf-button is-primary",
                    r#type: "button",
                    onclick: move |_| run(("login".to_owned(), task(mixamo_bridge::login()))),
                    "Увійти в Mixamo"
                }
            }
            button {
                class: if connected { "nf-button is-primary" } else { "nf-button" },
                r#type: "button",
                title: "Розібрати дизайн-документ гри й поставити в чергу потрібні анімації",
                disabled: locked,
                onclick: move |_| run(("plan".to_owned(), task(async move { mixamo_bridge::plan(project_id).await.map(drop) }))),
                if pending("plan") {
                    span { class: "nf-spinner" }
                } else {
                    PlusIcon {}
                }
                "Підібрати під гру"
            }
        }
        section { class: "nf-section",
            h3 { class: "nf-section-title", "Знайти на Mixamo" }
            form {
                class: "nf-motion-search",
                novalidate: true,
                onsubmit: move |event| {
                    event.prevent_default();
                    search();
                },
                input {
                    class: "nf-motion-input",
                    r#type: "search",
                    maxlength: "{MAX_MOTION_QUERY_CHARS}",
                    placeholder: "sword slash, crouch walk…",
                    aria_label: "Пошук анімацій Mixamo",
                    value: "{query}",
                    oninput: move |event| query.set(event.value()),
                }
                button {
                    class: "nf-button",
                    r#type: "submit",
                    disabled: locked || query.read().trim().is_empty(),
                    if pending("search") {
                        span { class: "nf-spinner" }
                    } else {
                        SearchIcon {}
                    }
                    "Знайти"
                }
            }
            div { class: "nf-toggles", role: "group", aria_label: "Як запекти",
                button {
                    class: if in_place() { "nf-toggle is-on" } else { "nf-toggle" },
                    r#type: "button",
                    title: "Без руху вперед — персонажа рухає код гри",
                    aria_pressed: if in_place() { "true" } else { "false" },
                    onclick: move |_| in_place.set(!in_place()),
                    "На місці"
                }
            }
            if let Some((searched, found)) = results() {
                if found.is_empty() {
                    p { class: "nf-item-note", "Mixamo нічого не знайшов за «{searched}»." }
                } else {
                    div { class: "nf-motion-grid",
                        for clip in found.into_iter().take(INSPECTOR_RESULTS) {
                            {
                                let key = format!("pick-{}", clip.id);
                                let draft = clip_draft(&clip, &searched, Some(project_id), options);
                                rsx! {
                                    button {
                                        key: "{clip.id}",
                                        class: "nf-motion-clip",
                                        r#type: "button",
                                        title: "Додати «{clip.title()}» у гру",
                                        disabled: locked,
                                        onclick: move |_| {
                                            let draft = draft.clone();
                                            run((key.clone(), task(async move { mixamo_bridge::order(&draft).await.map(drop) })))
                                        },
                                        if clip.preview().is_empty() {
                                            span { class: "nf-motion-still" }
                                        } else {
                                            img {
                                                src: "{clip.preview()}",
                                                alt: "{clip.title()}",
                                                loading: "lazy",
                                                referrerpolicy: "no-referrer",
                                                draggable: "false",
                                            }
                                        }
                                        span { class: "nf-motion-caption", "{clip.name}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if !orders.is_empty() {
            section { class: "nf-section",
                h3 { class: "nf-section-title", "Анімації гри · {plural(orders.len() as u32, ORDER_WORDS)}" }
                div { class: "nf-items",
                    for order in orders {
                        {
                            let id = order.id;
                            let clip = order.clip.clone().unwrap_or_default();
                            let note = if clip.title().is_empty() { format!("Шукає «{}»", order.query) } else { clip.title().to_owned() };
                            let tags = order.options.tags().join(" · ");
                            let file = file_name(&order);
                            let finished = matches!(order.status, MotionStatus::Done | MotionStatus::Failed);
                            rsx! {
                                div { key: "{id}", class: "nf-item nf-motion-order",
                                    if clip.preview().is_empty() {
                                        span { class: "nf-motion-still" }
                                    } else {
                                        img {
                                            class: "nf-motion-still",
                                            src: "{clip.preview()}",
                                            alt: "{note}",
                                            loading: "lazy",
                                            referrerpolicy: "no-referrer",
                                            draggable: "false",
                                        }
                                    }
                                    div { class: "nf-motion-order-text",
                                        div { class: "nf-item-head",
                                            span { class: "nf-led is-{motion_phase(order.status).code()}" }
                                            span { class: "nf-item-title", title: "{order.name}", "{order.name}" }
                                            span { class: "nf-item-status", "{order.status.label()}" }
                                        }
                                        p { class: "nf-item-note", "{note}" }
                                        p { class: "nf-item-note", "{tags}" }
                                        if let Some(file) = &file {
                                            p { class: "nf-item-note", title: "{file}", "{file}" }
                                        }
                                        if let Some(problem) = &order.error {
                                            p { class: "nf-item-note is-failed", "{problem}" }
                                        }
                                        div { class: "nf-actions",
                                            if order.status == MotionStatus::Done {
                                                button {
                                                    class: "nf-button is-quiet",
                                                    r#type: "button",
                                                    onclick: move |_| run((format!("reveal-{id}"), task(mixamo_bridge::reveal(id)))),
                                                    FolderIcon {}
                                                    "FBX"
                                                }
                                            }
                                            if finished {
                                                button {
                                                    class: "nf-button is-quiet",
                                                    r#type: "button",
                                                    title: "Запекти й завантажити ще раз",
                                                    disabled: locked,
                                                    onclick: move |_| run((format!("retry-{id}"), task(mixamo_bridge::retry(id)))),
                                                    RefreshIcon {}
                                                    "Ще раз"
                                                }
                                            }
                                            if !order.status.is_active() {
                                                button {
                                                    class: "nf-button is-quiet is-danger",
                                                    r#type: "button",
                                                    aria_label: "Видалити «{order.name}»",
                                                    disabled: locked,
                                                    onclick: move |_| run((format!("delete-{id}"), task(mixamo_bridge::delete(id)))),
                                                    TrashIcon {}
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
        }
    }
}

pub fn explain_mixamo(problem: &MixamoError) -> String {
    match problem {
        MixamoError::SignedOut => "Mixamo не підключено — увійди в Adobe у вікні Mixamo.".to_owned(),
        MixamoError::NotFound => "Цієї анімації чи гри вже немає.".to_owned(),
        MixamoError::NameMissing => "Дай анімації назву.".to_owned(),
        MixamoError::NameTooLong => format!("Назва задовга — максимум {MAX_MOTION_NAME_CHARS} символів."),
        MixamoError::QueryMissing => "Напиши, що шукати на Mixamo.".to_owned(),
        MixamoError::QueryTooLong => format!("Запит задовгий — максимум {MAX_MOTION_QUERY_CHARS} символів."),
        MixamoError::NoMatch => "Mixamo нічого не знайшов — спробуй інший запит англійською.".to_owned(),
        MixamoError::Busy => "Анімація ще запікається — зачекай, поки вона завершиться.".to_owned(),
        MixamoError::Network(detail) => {
            tracing::warn!("mixamo request failed: {detail}");
            "Mixamo не відповів. Перевір інтернет і спробуй ще раз.".to_owned()
        }
        MixamoError::Browser(detail) => {
            tracing::warn!("mixamo window failed: {detail}");
            "Не вдалося відкрити вікно Mixamo.".to_owned()
        }
        MixamoError::Storage(detail) => {
            tracing::error!("mixamo storage failed: {detail}");
            "Не вдалося зберегти анімацію.".to_owned()
        }
    }
}
