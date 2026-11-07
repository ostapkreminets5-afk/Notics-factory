use dioxus::prelude::*;
use noctics_shared::{AssetFind, AssetHunt, FindKind, HuntStatus, MAX_HUNT_REQUEST_CHARS};

use crate::asset_hunt_bridge;
use crate::icons::{ArrowRightIcon, ExternalIcon, RefreshIcon, SearchIcon};
use crate::journal::plural;
use crate::node_factory::{now_seconds, pause, stopwatch};
use crate::studio::{Section, explain, use_studio};

pub const HUNT_TITLE: &str = "Пошук ассетів";
pub const HUNT_NOTE: &str = "itch.io та інтернет";
const TICK_MILLIS: i32 = 1000;
const FIND_WORDS: [&str; 3] = ["ассет", "ассети", "ассетів"];
const FREE_WORDS: [&str; 3] = ["безкоштовний", "безкоштовні", "безкоштовних"];
const SEARCH_SITES: &str = "itch.io, OpenGameArt, Kenney та інших сайтах";
const REQUEST_PLACEHOLDER: &str = "Наприклад: тайли печер 16×16, звуки кроків, піксельний шрифт";

pub fn use_asset_hunt(focus: Memo<Option<i64>>) -> Signal<Option<AssetHunt>> {
    let studio = use_studio();
    let mut hunt = use_signal(|| None::<AssetHunt>);
    use_resource(move || async move {
        let _ = (studio.revision)();
        let Some(project_id) = focus() else {
            hunt.set(None);
            return;
        };
        if let Ok(fresh) = asset_hunt_bridge::hunt(project_id).await {
            hunt.set(fresh);
        }
    });
    hunt
}

pub fn found_summary(hunt: &AssetHunt) -> String {
    let total = plural(amount(hunt.finds.len()), FIND_WORDS);
    let free = hunt.free_count();
    if free == hunt.finds.len() {
        format!("Знайдено {total}, усі безкоштовні")
    } else {
        format!("Знайдено {total}, з них {}", plural(amount(free), FREE_WORDS))
    }
}

pub fn searching_note() -> String {
    format!("Claude шукає на {SEARCH_SITES}")
}

fn amount(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn button_label(hunt: Option<&AssetHunt>, typed: bool) -> &'static str {
    match (hunt, typed) {
        (None, _) => "Знайти ассети",
        (Some(_), true) => "Знайти й додати",
        (Some(_), false) => "Шукати заново",
    }
}

fn credit(find: &AssetFind) -> String {
    if find.author.is_empty() {
        find.site.clone()
    } else {
        format!("{} · {}", find.site, find.author)
    }
}

fn license_of(find: &AssetFind) -> &str {
    if find.license.is_empty() { "не вказано" } else { &find.license }
}

fn cost_class(find: &AssetFind, base: &str) -> String {
    let tone = if find.cost.downloadable() { "is-free" } else { "is-paid" };
    format!("{base} {tone}")
}

#[component]
pub fn AssetHuntPage() -> Element {
    let studio = use_studio();
    let focus = use_memo(move || studio.focused_project().map(|project| project.id));
    let loaded = use_asset_hunt(focus);
    let mut request = use_signal(String::new);
    let mut starting = use_signal(|| false);
    let mut notice = use_signal(|| None::<String>);
    let mut filter = use_signal(|| None::<FindKind>);

    let project = studio.focused_project();
    let hunt = loaded
        .read()
        .clone()
        .filter(|hunt| Some(hunt.project_id) == focus());

    let mut launch = move |project_id: i64| {
        if starting() {
            return;
        }
        starting.set(true);
        notice.set(None);
        let text = request.peek().trim().to_owned();
        spawn(async move {
            match asset_hunt_bridge::start(project_id, &text).await {
                Ok(_) => {
                    request.set(String::new());
                    studio.changed();
                }
                Err(problem) => notice.set(Some(explain(&problem))),
            }
            starting.set(false);
        });
    };

    let open = move |project_id: i64, url: String| {
        spawn(async move {
            if let Err(problem) = asset_hunt_bridge::open(project_id, &url).await {
                notice.set(Some(explain(&problem)));
            }
        });
    };

    let lead = match &project {
        Some(project) => format!(
            "Claude шукає на {SEARCH_SITES} готові ассети під стиль «{}» — агент коду отримає їх разом із брифом",
            project.name
        ),
        None => format!("Claude шукає на {SEARCH_SITES} готові ассети під стиль кожної гри"),
    };

    let body = match project {
        None => rsx! {
            div { class: "page-empty",
                p { class: "mono-label", "Ще немає проєкту" }
                p { "Запусти конвеєр гри — щойно Claude складе план, він сам піде шукати ассети на itch.io та інших сайтах." }
                button {
                    class: "ghost-button is-start",
                    r#type: "button",
                    onclick: move |_| studio.open(Section::Pipeline),
                    "Конвеєр гри"
                    ArrowRightIcon {}
                }
            }
        },
        Some(project) if project.status.is_planning() => rsx! {
            div { class: "page-empty",
                p { class: "mono-label", "Claude ще планує гру" }
                p { "Пошук стартує сам, щойно будуть готові назва, стиль і дизайн-документ." }
            }
        },
        Some(project) => {
            let project_id = project.id;
            let active = hunt.as_ref().is_some_and(|hunt| hunt.status.is_active());
            let typed = !request.read().trim().is_empty();
            let length = request.read().chars().count();
            let kinds: Vec<(FindKind, usize)> = hunt
                .as_ref()
                .map(|hunt| {
                    hunt.kinds()
                        .into_iter()
                        .map(|kind| (kind, hunt.finds.iter().filter(|find| find.kind == kind).count()))
                        .collect()
                })
                .unwrap_or_default();
            let chosen = filter().filter(|kind| kinds.iter().any(|(present, _)| present == kind));
            let finds: Vec<AssetFind> = hunt
                .as_ref()
                .map(|hunt| {
                    hunt.finds
                        .iter()
                        .filter(|find| chosen.is_none_or(|kind| find.kind == kind))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            let total = hunt.as_ref().map_or(0, |hunt| hunt.finds.len());
            let sites = hunt.as_ref().map(site_counts).unwrap_or_default();
            rsx! {
                form {
                    class: "hunt-composer",
                    novalidate: true,
                    onsubmit: move |event| {
                        event.prevent_default();
                        launch(project_id);
                    },
                    div { class: "hunt-field",
                        div { class: "hunt-field-head",
                            label { class: "mono-label", r#for: "hunt-request", "Що шукати окремо" }
                            span { class: "mono-label", "{length} / {MAX_HUNT_REQUEST_CHARS}" }
                        }
                        input {
                            class: "input",
                            id: "hunt-request",
                            maxlength: "{MAX_HUNT_REQUEST_CHARS}",
                            placeholder: REQUEST_PLACEHOLDER,
                            value: "{request}",
                            oninput: move |event| request.set(event.value()),
                        }
                    }
                    button {
                        class: "primary-button",
                        r#type: "submit",
                        disabled: starting() || active,
                        title: "Порожнє поле — новий підбір під усю гру. З текстом — Claude шукає саме це й додає до знайденого.",
                        if starting() || active {
                            span { class: "spinner" }
                            "Шукає…"
                        } else {
                            if hunt.is_some() {
                                RefreshIcon {}
                            } else {
                                SearchIcon {}
                            }
                            "{button_label(hunt.as_ref(), typed)}"
                        }
                    }
                }
                if let Some(message) = notice() {
                    div { class: "page-alert", role: "alert", span { "{message}" } }
                }
                HuntStatusLine { hunt: hunt.clone() }
                if let Some(summary) = hunt.as_ref().map(|hunt| hunt.summary.clone()).filter(|text| !text.is_empty()) {
                    p { class: "hunt-summary", "{summary}" }
                }
                if !sites.is_empty() {
                    div { class: "hunt-sites", aria_label: "Сайти, де знайдено ассети",
                        for (site, count) in sites {
                            span { key: "{site}", class: "hunt-site",
                                "{site}"
                                span { class: "hunt-site-count", "{count}" }
                            }
                        }
                    }
                }
                if kinds.len() > 1 {
                    div { class: "chip-row hunt-filters", role: "group", aria_label: "Категорії ассетів",
                        button {
                            class: if chosen.is_none() { "chip is-active" } else { "chip" },
                            r#type: "button",
                            aria_pressed: if chosen.is_none() { "true" } else { "false" },
                            onclick: move |_| filter.set(None),
                            "Усі"
                            span { class: "chip-count", "{total}" }
                        }
                        for (kind, count) in kinds {
                            button {
                                key: "{kind.code()}",
                                class: if chosen == Some(kind) { "chip is-active" } else { "chip" },
                                r#type: "button",
                                aria_pressed: if chosen == Some(kind) { "true" } else { "false" },
                                onclick: move |_| filter.set(Some(kind)),
                                "{kind.label()}"
                                span { class: "chip-count", "{count}" }
                            }
                        }
                    }
                }
                if finds.is_empty() {
                    if !active {
                        div { class: "page-empty hunt-empty-grid",
                            p { class: "mono-label", "Поки порожньо" }
                            p {
                                "Натисни «Знайти ассети» — Claude перегляне itch.io, OpenGameArt, Kenney, Poly Haven, Freesound та інші сайти й підбере паки під перспективу, палітру й настрій гри."
                            }
                        }
                    }
                } else {
                    div { class: "hunt-grid",
                        for find in finds {
                            article { key: "{find.url}", class: "hunt-card",
                                FindCover { find: find.clone(), compact: false }
                                div { class: "hunt-card-body",
                                    p { class: "hunt-card-meta", title: "{credit(&find)}", "{credit(&find)}" }
                                    h3 { class: "hunt-card-title", title: "{find.title}", "{find.title}" }
                                    if !find.fit.is_empty() {
                                        p { class: "hunt-card-fit", "{find.fit}" }
                                    }
                                    p { class: "hunt-card-license", "Ліцензія: {license_of(&find)}" }
                                    div { class: "hunt-card-foot",
                                        button {
                                            class: "ghost-button is-small",
                                            r#type: "button",
                                            title: "{find.url}",
                                            onclick: {
                                                let url = find.url.clone();
                                                move |_| open(project_id, url.clone())
                                            },
                                            "Відкрити сторінку"
                                            ExternalIcon {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        section { class: "page hunt-page",
            header { class: "page-head",
                p { class: "mono-label", "Система · Готові ассети" }
                h1 { class: "page-title", "{HUNT_TITLE}" }
                p { class: "page-lead", "{lead}" }
            }
            {body}
        }
    }
}

fn site_counts(hunt: &AssetHunt) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for find in &hunt.finds {
        match counts.iter_mut().find(|(site, _)| *site == find.site) {
            Some((_, count)) => *count += 1,
            None => counts.push((find.site.clone(), 1)),
        }
    }
    counts.sort_by(|left, right| right.1.cmp(&left.1));
    counts
}

#[component]
fn HuntStatusLine(hunt: Option<AssetHunt>) -> Element {
    let Some(hunt) = hunt else {
        return rsx! {
            p { class: "hunt-status",
                span { class: "hunt-dot" }
                "Ще не шукали для цієї гри"
            }
        };
    };
    let text = match hunt.status {
        HuntStatus::Queued => "У черзі на пошук".to_owned(),
        HuntStatus::Searching => searching_note(),
        HuntStatus::Done => found_summary(&hunt),
        HuntStatus::Failed => hunt
            .error
            .as_ref()
            .map_or_else(|| "Пошук не вдався".to_owned(), explain),
    };
    let since = hunt.started_at.filter(|_| hunt.status == HuntStatus::Searching);

    rsx! {
        p { class: "hunt-status", role: "status",
            span { class: "hunt-dot is-{hunt.status.code()}" }
            span { "{text}" }
            if let Some(since) = since {
                HuntClock { since }
            }
        }
    }
}

#[component]
fn HuntClock(since: i64) -> Element {
    let mut now = use_signal(now_seconds);

    use_future(move || async move {
        loop {
            pause(TICK_MILLIS).await;
            now.set(now_seconds());
        }
    });

    rsx! {
        span { class: "hunt-status-time", title: "Від старту пошуку", "{stopwatch(now() - since)}" }
    }
}

#[component]
pub fn FindCover(find: AssetFind, compact: bool) -> Element {
    let mut broken = use_signal(|| false);
    let image = find.image.clone().filter(|_| !broken());

    if compact {
        return match image {
            Some(image) => rsx! {
                img {
                    class: "nf-find-thumb",
                    src: "{image}",
                    alt: "{find.title}",
                    draggable: "false",
                    "loading": "lazy",
                    "referrerpolicy": "no-referrer",
                    onerror: move |_| broken.set(true),
                }
            },
            None => rsx! {
                span { class: "nf-find-thumb is-empty", "{find.site}" }
            },
        };
    }

    rsx! {
        div { class: "hunt-cover",
            if let Some(image) = image {
                img {
                    src: "{image}",
                    alt: "{find.title}",
                    draggable: "false",
                    "loading": "lazy",
                    "referrerpolicy": "no-referrer",
                    onerror: move |_| broken.set(true),
                }
            } else {
                div { class: "hunt-cover-empty", "{find.site}" }
            }
            span { class: "hunt-badge is-kind", "{find.kind.label()}" }
            span { class: cost_class(&find, "hunt-badge is-cost"), "{find.cost_label()}" }
        }
    }
}

#[component]
pub fn FindsPanel(project_id: i64, hunt: Option<AssetHunt>) -> Element {
    let studio = use_studio();
    let mut request = use_signal(String::new);
    let mut starting = use_signal(|| false);
    let mut notice = use_signal(|| None::<String>);
    let active = hunt.as_ref().is_some_and(|hunt| hunt.status.is_active());
    let typed = !request.read().trim().is_empty();
    let length = request.read().chars().count();

    let mut launch = move || {
        if starting() {
            return;
        }
        starting.set(true);
        notice.set(None);
        let text = request.peek().trim().to_owned();
        spawn(async move {
            match asset_hunt_bridge::start(project_id, &text).await {
                Ok(_) => {
                    request.set(String::new());
                    studio.changed();
                }
                Err(problem) => notice.set(Some(explain(&problem))),
            }
            starting.set(false);
        });
    };

    let open = move |url: String| {
        spawn(async move {
            if let Err(problem) = asset_hunt_bridge::open(project_id, &url).await {
                notice.set(Some(explain(&problem)));
            }
        });
    };

    rsx! {
        if let Some(text) = notice() {
            p { class: "nf-notice", role: "alert", "{text}" }
        }
        form {
            class: "nf-section",
            novalidate: true,
            onsubmit: move |event| {
                event.prevent_default();
                launch();
            },
            div { class: "nf-field",
                label { class: "nf-label", r#for: "nf-hunt-request",
                    span { "Що шукати окремо" }
                    span { class: "nf-label-count", "{length} / {MAX_HUNT_REQUEST_CHARS}" }
                }
                input {
                    class: "nf-input",
                    id: "nf-hunt-request",
                    maxlength: "{MAX_HUNT_REQUEST_CHARS}",
                    placeholder: "Порожньо — підбір під усю гру",
                    value: "{request}",
                    oninput: move |event| request.set(event.value()),
                }
            }
            div { class: "nf-actions",
                button {
                    class: "nf-button is-primary",
                    r#type: "submit",
                    disabled: starting() || active,
                    if starting() || active {
                        span { class: "nf-spinner" }
                        "Шукає"
                    } else {
                        if hunt.is_some() {
                            RefreshIcon {}
                        } else {
                            SearchIcon {}
                        }
                        "{button_label(hunt.as_ref(), typed)}"
                    }
                }
            }
        }
        if let Some(hunt) = &hunt {
            if !hunt.summary.is_empty() {
                section { class: "nf-section",
                    h3 { class: "nf-section-title", "Підсумок" }
                    p { class: "nf-text", "{hunt.summary}" }
                }
            }
            div { class: "nf-items",
                for find in hunt.finds.iter().cloned() {
                    div { key: "{find.url}", class: "nf-item",
                        div { class: "nf-item-head",
                            span { class: "nf-item-title", title: "{find.title}", "{find.title}" }
                            span { class: "nf-item-tag", "{find.kind.label()}" }
                        }
                        div { class: "nf-find-body",
                            FindCover { find: find.clone(), compact: true }
                            div { class: "nf-find-facts",
                                span { class: "nf-find-site", title: "{credit(&find)}", "{credit(&find)}" }
                                span { "{license_of(&find)}" }
                                span { class: cost_class(&find, "nf-find-cost"), "{find.cost_label()}" }
                            }
                        }
                        if !find.fit.is_empty() {
                            p { class: "nf-item-note", "{find.fit}" }
                        }
                        div { class: "nf-actions",
                            button {
                                class: "nf-button is-quiet",
                                r#type: "button",
                                title: "{find.url}",
                                onclick: {
                                    let url = find.url.clone();
                                    move |_| open(url.clone())
                                },
                                ExternalIcon {}
                                "Відкрити сторінку"
                            }
                        }
                    }
                }
            }
        }
    }
}
