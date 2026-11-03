use dioxus::prelude::*;
use dioxus_logger::tracing;
use noctics_shared::{
    DETAIL_PIN_SIZE, PINS_PER_REFERENCE, PINTEREST_API_PORT, Pin, PinterestError, PinterestStatus, Project,
    PromptAnalysis, Reference, ReferenceTopic, StyleGuide, ToolSlot,
};

use crate::bridge;
use crate::icons::{ArrowRightIcon, CloseIcon, ExternalIcon};
use crate::journal::plural;
use crate::studio::{Section, explain, use_studio};
use crate::tools::{explain_tool, use_tools};

const DEFAULT_TOOL_NAME: &str = "Pinterest";
const PICTURE_WORDS: [&str; 3] = ["картинка", "картинки", "картинок"];
const SEARCH_WORDS: [&str; 3] = ["пошук", "пошуки", "пошуків"];
const TOPIC_WORDS: [&str; 3] = ["тема", "теми", "тем"];

#[derive(Clone, PartialEq)]
struct Viewed {
    pin: Pin,
    reference: String,
    index: usize,
}

#[derive(Clone, PartialEq)]
struct BoardPin {
    pin: Pin,
    reference: String,
    topic: ReferenceTopic,
    index: usize,
}

#[derive(Clone, PartialEq)]
struct SearchEntry {
    index: usize,
    title: String,
    query: String,
}

#[derive(Clone, PartialEq)]
struct SearchGroup {
    topic: ReferenceTopic,
    what: String,
    found: usize,
    entries: Vec<SearchEntry>,
}

fn search_groups(analysis: &PromptAnalysis, references: &[Reference]) -> Vec<SearchGroup> {
    let planned = analysis.needs.iter().map(|need| need.topic);
    let searched = ReferenceTopic::ALL
        .into_iter()
        .filter(|topic| references.iter().any(|reference| reference.topic == *topic));
    let mut topics: Vec<ReferenceTopic> = Vec::new();
    for topic in planned.chain(searched) {
        if !topics.contains(&topic) {
            topics.push(topic);
        }
    }
    topics
        .into_iter()
        .map(|topic| {
            let matching = || {
                references
                    .iter()
                    .enumerate()
                    .filter(move |(_, reference)| reference.topic == topic)
            };
            SearchGroup {
                topic,
                what: analysis
                    .need_for(topic)
                    .map(|need| need.what.clone())
                    .unwrap_or_default(),
                found: matching().map(|(_, reference)| reference.pins.len()).sum(),
                entries: matching()
                    .map(|(index, reference)| SearchEntry {
                        index,
                        title: reference.title.clone(),
                        query: reference.query.clone(),
                    })
                    .collect(),
            }
        })
        .collect()
}

fn board_pins(references: &[Reference]) -> Vec<BoardPin> {
    references
        .iter()
        .enumerate()
        .flat_map(|(index, reference)| {
            reference.pins.iter().map(move |pin| BoardPin {
                pin: pin.clone(),
                reference: reference.title.clone(),
                topic: reference.topic,
                index,
            })
        })
        .collect()
}

fn amount(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[component]
pub fn ReferencesPage() -> Element {
    let studio = use_studio();
    let tools = use_tools();
    let mut picking = use_signal(|| None::<i64>);
    let mut notice = use_signal(|| None::<String>);
    let mut pinterest = use_signal(PinterestStatus::default);

    use_resource(move || async move {
        let _ = (studio.revision)();
        if let Ok(status) = bridge::pinterest_status().await {
            pinterest.set(status);
        }
    });

    let project = studio.focused_project();
    let tool = tools.active(ToolSlot::References);
    let tool_name = tool
        .as_ref()
        .map_or_else(|| DEFAULT_TOOL_NAME.to_owned(), |tool| tool.name.clone());
    let tool_link = tool.filter(|tool| tool.url.is_some()).map(|tool| tool.id);
    let has_link = tool_link.is_some();

    let mut pick = move |project_id: i64| {
        if picking().is_some() {
            return;
        }
        picking.set(Some(project_id));
        notice.set(None);
        spawn(async move {
            match bridge::pick_references(project_id).await {
                Ok(_) => studio.changed(),
                Err(problem) => notice.set(Some(explain(&problem))),
            }
            picking.set(None);
        });
    };

    let mut load_pins = move |project_id: i64| {
        notice.set(None);
        spawn(async move {
            match bridge::load_reference_pins(project_id).await {
                Ok(()) => studio.changed(),
                Err(problem) => notice.set(Some(explain_pinterest(&problem))),
            }
        });
    };

    let sign_in = move |_| {
        notice.set(None);
        spawn(async move {
            if let Err(problem) = bridge::pinterest_login().await {
                notice.set(Some(explain_pinterest(&problem)));
            }
        });
    };

    let open_tool = move |_| {
        let Some(id) = tool_link.clone() else {
            return;
        };
        spawn(async move {
            if let Err(problem) = bridge::open_tool(&id).await {
                notice.set(Some(explain_tool(&problem)));
            }
        });
    };

    let lead = match &project {
        Some(project) => format!("Референси, які AI-ядро підібрало для «{}»", project.name),
        None => "Референси, які AI-ядро підбирає для кожної гри".to_owned(),
    };
    let ready = project
        .as_ref()
        .filter(|project| !project.status.is_planning() && !project.references.is_empty())
        .map(|project| project.id);
    let busy = picking().is_some();
    let status = pinterest();
    let pins_loading = ready.is_some_and(|project_id| status.is_loading(project_id));
    let signed_out = status.signed_in == Some(false);

    rsx! {
        section { class: "page",
            header { class: "page-head has-action",
                div { class: "page-head-text",
                    p { class: "mono-label", "Система · {ToolSlot::References.purpose()}" }
                    h1 { class: "page-title", "{tool_name}" }
                    p { class: "page-lead", "{lead}" }
                }
                div { class: "head-actions",
                    if let Some(project_id) = ready {
                        button {
                            class: "ghost-button",
                            r#type: "button",
                            disabled: busy,
                            onclick: move |_| pick(project_id),
                            if busy {
                                span { class: "spinner" }
                                "Підбираємо…"
                            } else {
                                "Підібрати заново"
                            }
                        }
                        button {
                            class: "ghost-button",
                            r#type: "button",
                            disabled: pins_loading,
                            onclick: move |_| load_pins(project_id),
                            if pins_loading {
                                span { class: "spinner" }
                                "Шукаємо піни…"
                            } else {
                                "Оновити піни"
                            }
                        }
                    }
                    if has_link {
                        button { class: "primary-button", r#type: "button", onclick: open_tool,
                            "Відкрити {tool_name}"
                            ExternalIcon {}
                        }
                    }
                }
            }
            if let Some(message) = notice() {
                div { class: "page-alert", role: "alert", span { "{message}" } }
            }
            if signed_out {
                div { class: "pinterest-link",
                    div { class: "pinterest-link-text",
                        p { class: "pinterest-link-title", "Підключи свій Pinterest" }
                        p { "Увійди один раз у вікні Pinterest — сесія збережеться в Noctics, і AI-ядро саме шукатиме й підтягуватиме піни для кожної гри. Пароль вводиш лише на сторінці Pinterest." }
                    }
                    button { class: "primary-button", r#type: "button", onclick: sign_in,
                        "Увійти в Pinterest"
                        ArrowRightIcon {}
                    }
                }
            }
            match project {
                None => rsx! {
                    div { class: "page-empty",
                        p { class: "mono-label", "Ще немає проєкту" }
                        p { "Запусти конвеєр гри — AI-ядро разом із планом підбере дошку референсів і підтягне піни з Pinterest." }
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
                        p { class: "mono-label", "Claude планує гру" }
                        p { "Референси й піни з’являться разом із планом «{project.name}»." }
                    }
                },
                Some(project) if project.references.is_empty() => {
                    let project_id = project.id;
                    rsx! {
                        div { class: "page-empty",
                            p { class: "mono-label", "Референсів ще немає" }
                            p { "Цей проєкт створено до появи дошки референсів. AI-ядро розбере промт і підбере їх зараз — VFX, панелі, кнопки, іконки, персонажів і світ гри." }
                            button {
                                class: "primary-button is-start",
                                r#type: "button",
                                disabled: busy,
                                onclick: move |_| pick(project_id),
                                if busy {
                                    span { class: "spinner" }
                                    "Підбираємо…"
                                } else {
                                    "Підібрати референси"
                                }
                            }
                        }
                    }
                }
                Some(project) => {
                    let came_back_empty = status.came_back_empty(project.id);
                    rsx! {
                        ReferenceBoard {
                            project,
                            loading: pins_loading,
                            came_back_empty,
                            connected: status.signed_in == Some(true),
                        }
                    }
                }
            }
            LocalPinterestApi {}
        }
    }
}

#[component]
fn LocalPinterestApi() -> Element {
    let address = format!("http://127.0.0.1:{PINTEREST_API_PORT}");
    rsx! {
        section { class: "pinterest-api", aria_label: "Локальний Pinterest API",
            p { class: "mono-label", "Безкоштовний Pinterest API · локально" }
            p { class: "pinterest-api-text",
                "Поки Noctics відкритий, будь-який твій проєкт чи скрипт може брати піни через нього — без RapidAPI, ключів і лімітів. Відповідь — JSON з id, image, title і link."
            }
            code { class: "pinterest-api-address", "{address}/pins?q=cyberpunk+ui&limit=30" }
            p { class: "pinterest-api-hint",
                "Довідка: {address}/ · стан: {address}/health · limit від 1 до 100. Приймає лише запити з цього комп’ютера."
            }
        }
    }
}

#[component]
fn ReferenceBoard(project: Project, loading: bool, came_back_empty: bool, connected: bool) -> Element {
    let notice = use_signal(|| None::<String>);
    let mut viewing = use_signal(|| None::<Viewed>);
    let filter = use_signal(|| None::<ReferenceTopic>);
    let project_id = project.id;
    let count = project.references.len();
    let filled = project
        .references
        .iter()
        .filter(|reference| !reference.pins.is_empty())
        .count();
    let groups = search_groups(&project.analysis, &project.references);
    let pins = board_pins(&project.references);
    let searches = plural(amount(count), SEARCH_WORDS);
    let pictures = plural(amount(pins.len()), PICTURE_WORDS);
    let has_pins = !pins.is_empty();

    rsx! {
        StylePanel { style: project.style.clone() }
        PromptAnalysisPanel {
            prompt: project.prompt.clone(),
            analysis: project.analysis.clone(),
            groups: groups.clone(),
            filter,
            on_search: move |index: usize| search_reference(project_id, index, notice),
        }
        div { class: "references-meta",
            span { class: "mono-label", "{searches} · {pictures}" }
            if connected {
                span { class: "references-connected", "Pinterest підключено" }
            }
            if loading {
                span { class: "references-progress",
                    span { class: "spinner" }
                    "Шукаємо в Pinterest · {filled}/{count}"
                }
            } else if came_back_empty {
                span { class: "references-warning",
                    "Pinterest нічого не віддав — увійди в акаунт і натисни «Оновити піни»."
                }
            } else if has_pins {
                span { class: "references-hint", "Натисни на картинку, щоб роздивитись її ближче" }
            }
        }
        if let Some(message) = notice() {
            div { class: "page-alert", role: "alert", span { "{message}" } }
        }
        StyleBoard {
            pins,
            groups,
            loading,
            filter,
            on_view: move |viewed: Viewed| viewing.set(Some(viewed)),
        }
        if let Some(viewed) = viewing() {
            PinViewer {
                viewed: viewed.clone(),
                on_search: move |_| search_reference(project_id, viewed.index, notice),
                on_close: move |_| viewing.set(None),
            }
        }
    }
}

fn search_reference(project_id: i64, index: usize, mut notice: Signal<Option<String>>) {
    spawn(async move {
        if let Err(problem) = bridge::open_reference(project_id, index).await {
            notice.set(Some(explain(&problem)));
        }
    });
}

#[component]
fn StylePanel(style: StyleGuide) -> Element {
    let mood = if style.mood.trim().is_empty() {
        "AI-ядро ще не описало настрій гри.".to_owned()
    } else {
        style.mood.clone()
    };

    rsx! {
        section { class: "style-panel",
            div { class: "style-panel-text",
                p { class: "mono-label", "Візуальний стиль · вибір AI-ядра" }
                p { class: "style-mood", "{mood}" }
                div { class: "style-facts",
                    if !style.fonts.trim().is_empty() {
                        span { class: "style-fact",
                            span { class: "mono-label", "Шрифти" }
                            span { "{style.fonts}" }
                        }
                    }
                    span { class: "style-fact",
                        span { class: "mono-label", "Кути" }
                        span { "{style.corner_radius} px" }
                    }
                }
            }
            if !style.palette.is_empty() {
                div { class: "style-palette", aria_label: "Палітра гри",
                    for color in style.palette.iter() {
                        div { key: "{color}", class: "style-swatch",
                            span { class: "style-swatch-fill", style: "background: {color}" }
                            code { "{color}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn PromptAnalysisPanel(
    prompt: String,
    analysis: PromptAnalysis,
    groups: Vec<SearchGroup>,
    filter: Signal<Option<ReferenceTopic>>,
    on_search: EventHandler<usize>,
) -> Element {
    let topics = plural(amount(groups.len()), TOPIC_WORDS);
    let prompt = prompt.trim().to_owned();

    rsx! {
        section { class: "analysis-panel", aria_label: "Аналіз промта від Claude",
            div { class: "analysis-read",
                p { class: "mono-label", "Аналіз промта · Claude" }
                if !prompt.is_empty() {
                    blockquote { class: "analysis-prompt", title: "{prompt}", "{prompt}" }
                }
                if analysis.summary.is_empty() {
                    p { class: "analysis-summary is-missing",
                        "Цю дошку зібрано до появи аналізу. Натисни «Підібрати заново» — Claude розбере промт і складе план пошуку: VFX, панелі, кнопки, іконки, персонажі."
                    }
                } else {
                    p { class: "analysis-summary", "{analysis.summary}" }
                }
                if !analysis.keywords.is_empty() {
                    div { class: "analysis-keywords", aria_label: "Ключові слова промта",
                        for keyword in analysis.keywords.iter() {
                            span { key: "{keyword}", class: "analysis-keyword", "{keyword}" }
                        }
                    }
                }
            }
            if !groups.is_empty() {
                p { class: "mono-label", "Що шукає Claude · {topics}" }
                div { class: "search-plan",
                    for group in groups {
                        {
                            let key = group.topic.code();
                            rsx! {
                                SearchNeedCard { key: "{key}", group, filter, on_search }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn SearchNeedCard(
    group: SearchGroup,
    filter: Signal<Option<ReferenceTopic>>,
    on_search: EventHandler<usize>,
) -> Element {
    let mut filter = filter;
    let topic = group.topic;
    let active = filter() == Some(topic);
    let found = plural(amount(group.found), PICTURE_WORDS);
    let pick_title = if active {
        "Показати всі картинки".to_owned()
    } else {
        format!("Показати лише «{}»", topic.label())
    };

    rsx! {
        article { class: if active { "search-need is-active" } else { "search-need" },
            button {
                class: "search-need-head",
                r#type: "button",
                aria_pressed: if active { "true" } else { "false" },
                title: "{pick_title}",
                onclick: move |_| filter.set(if active { None } else { Some(topic) }),
                span { class: "reference-topic", "{topic.label()}" }
                span { class: "search-need-count", "{found}" }
            }
            if !group.what.is_empty() {
                p { class: "search-need-what", "{group.what}" }
            }
            if group.entries.is_empty() {
                p { class: "search-need-empty", "Пошук для цієї теми ще не складено." }
            } else {
                ul { class: "search-need-queries",
                    for entry in group.entries {
                        {
                            let SearchEntry { index, title, query } = entry;
                            rsx! {
                                li { key: "{index}", class: "search-need-query",
                                    span { class: "search-need-title", "{title}" }
                                    button {
                                        class: "reference-query",
                                        r#type: "button",
                                        title: "Шукати ще в Pinterest: {query}",
                                        onclick: move |_| on_search.call(index),
                                        ExternalIcon {}
                                        span { "{query}" }
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
fn StyleBoard(
    pins: Vec<BoardPin>,
    groups: Vec<SearchGroup>,
    loading: bool,
    filter: Signal<Option<ReferenceTopic>>,
    on_view: EventHandler<Viewed>,
) -> Element {
    let mut filter = filter;
    let chips: Vec<(ReferenceTopic, usize)> = groups
        .iter()
        .filter(|group| group.found > 0)
        .map(|group| (group.topic, group.found))
        .collect();
    let active = filter().filter(|topic| groups.iter().any(|group| group.topic == *topic));
    let total = pins.len();
    let shown: Vec<BoardPin> = pins
        .into_iter()
        .filter(|item| active.is_none_or(|topic| item.topic == topic))
        .collect();
    let shown_label = plural(amount(shown.len()), PICTURE_WORDS);
    let empty_text = match active {
        Some(topic) => format!("Для теми «{}» Pinterest ще нічого не віддав.", topic.label()),
        None => "Картинок ще немає — увійди в Pinterest і натисни «Оновити піни».".to_owned(),
    };

    rsx! {
        section { class: "style-board", aria_label: "Усі картинки стилю гри",
            div { class: "style-board-head",
                p { class: "mono-label", "Дошка стилю · {shown_label}" }
                if !chips.is_empty() {
                    div { class: "style-board-filters", role: "group", aria_label: "Фільтр за темою",
                        button {
                            class: if active.is_none() { "board-chip is-active" } else { "board-chip" },
                            r#type: "button",
                            aria_pressed: if active.is_none() { "true" } else { "false" },
                            onclick: move |_| filter.set(None),
                            "Усі"
                            span { class: "board-chip-count", "{total}" }
                        }
                        for (topic, found) in chips {
                            {
                                let selected = active == Some(topic);
                                rsx! {
                                    button {
                                        key: "{topic.code()}",
                                        class: if selected { "board-chip is-active" } else { "board-chip" },
                                        r#type: "button",
                                        aria_pressed: if selected { "true" } else { "false" },
                                        onclick: move |_| filter.set(Some(topic)),
                                        "{topic.label()}"
                                        span { class: "board-chip-count", "{found}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if shown.is_empty() && !loading {
                p { class: "style-board-empty", "{empty_text}" }
            } else {
                div { class: "style-board-grid", aria_busy: if loading { "true" } else { "false" },
                    for item in shown {
                        {
                            let key = format!("{}-{}", item.index, item.pin.image);
                            rsx! {
                                PinTile { key: "{key}", item, on_view }
                            }
                        }
                    }
                    if loading {
                        for slot in 0..PINS_PER_REFERENCE {
                            span { key: "ghost-{slot}", class: "board-ghost" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn PinTile(item: BoardPin, on_view: EventHandler<Viewed>) -> Element {
    let label = if item.pin.title.is_empty() {
        format!("Пін для «{}»", item.reference)
    } else {
        item.pin.title.clone()
    };
    let image = item.pin.image.clone();
    let caption = format!("{} · {}", item.topic.label(), item.reference);

    rsx! {
        button {
            class: "board-pin",
            r#type: "button",
            title: "{label}",
            onclick: move |_| {
                on_view.call(Viewed {
                    pin: item.pin.clone(),
                    reference: item.reference.clone(),
                    index: item.index,
                })
            },
            img {
                src: "{image}",
                alt: "{label}",
                loading: "lazy",
                referrerpolicy: "no-referrer",
                draggable: "false",
            }
            span { class: "board-pin-caption", "{caption}" }
        }
    }
}

#[component]
fn PinViewer(viewed: Viewed, on_search: EventHandler<()>, on_close: EventHandler<()>) -> Element {
    let mut notice = use_signal(|| None::<String>);
    let mut backdrop_pressed = use_signal(|| false);
    let Viewed { pin, reference, .. } = viewed;
    let image = pin.sized(DETAIL_PIN_SIZE);
    let linked = pin.is_linked();
    let pin_id = pin.id.clone();

    let open_pin = move |_| {
        if !linked {
            on_search.call(());
            return;
        }
        let pin_id = pin_id.clone();
        spawn(async move {
            if let Err(problem) = bridge::open_pin(&pin_id).await {
                notice.set(Some(explain_pinterest(&problem)));
            }
        });
    };

    rsx! {
        div {
            class: "pin-viewer",
            tabindex: "-1",
            onmounted: move |event| async move {
                let _ = event.data().set_focus(true).await;
            },
            onmousedown: move |_| backdrop_pressed.set(true),
            onclick: move |_| {
                if backdrop_pressed() {
                    on_close.call(());
                }
            },
            onkeydown: move |event| {
                if event.key() == Key::Escape {
                    on_close.call(());
                }
            },
            figure {
                class: "pin-viewer-frame",
                role: "dialog",
                aria_modal: "true",
                aria_label: "{reference}",
                onmousedown: move |event| {
                    event.stop_propagation();
                    backdrop_pressed.set(false);
                },
                onclick: move |event| event.stop_propagation(),
                img {
                    src: "{image}",
                    alt: "{pin.title}",
                    referrerpolicy: "no-referrer",
                    draggable: "false",
                }
                figcaption { class: "pin-viewer-caption",
                    span { class: "reference-topic", "{reference}" }
                    if !pin.title.is_empty() {
                        span { class: "pin-viewer-title", "{pin.title}" }
                    }
                    if let Some(message) = notice() {
                        span { class: "references-warning", "{message}" }
                    }
                    div { class: "pin-viewer-actions",
                        button { class: "primary-button is-small", r#type: "button", onclick: open_pin,
                            if linked {
                                "Відкрити пін у Pinterest"
                            } else {
                                "Шукати схожі в Pinterest"
                            }
                            ExternalIcon {}
                        }
                        button {
                            class: "ghost-button is-small",
                            r#type: "button",
                            aria_label: "Закрити",
                            onclick: move |_| on_close.call(()),
                            CloseIcon {}
                        }
                    }
                }
            }
        }
    }
}

fn explain_pinterest(problem: &PinterestError) -> String {
    match problem {
        PinterestError::SignedOut => "Сесія завершилась — увійди знову.".to_owned(),
        PinterestError::NotFound => "Цього проєкту чи піна вже немає.".to_owned(),
        PinterestError::NoReferences => {
            "Спершу AI-ядро має підібрати дошку референсів.".to_owned()
        }
        PinterestError::Browser(detail) => {
            tracing::warn!("pinterest browser failed: {detail}");
            "Не вдалося відкрити Pinterest. Перевір інтернет і спробуй ще раз.".to_owned()
        }
        PinterestError::Storage(detail) => {
            tracing::error!("reference storage failed: {detail}");
            "Не вдалося зберегти піни в проєкт.".to_owned()
        }
    }
}
