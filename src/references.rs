use dioxus::prelude::*;
use dioxus_logger::tracing;
use noctics_shared::{
    ARTSTATION_API_PORT, ARTWORKS_PER_REFERENCE, ArtstationError, ArtstationStatus, Artwork,
    DETAIL_ARTWORK_SIZE, DETAIL_PIN_SIZE, PINTEREST_API_PORT, Pin, PinterestError, PinterestStatus,
    Project, PromptAnalysis, Reference, ReferenceTopic, StyleGuide, ToolSlot,
};

use crate::artstation_bridge;
use crate::bridge;
use crate::icons::{ArrowRightIcon, CloseIcon, ExternalIcon};
use crate::journal::plural;
use crate::studio::{Section, explain, use_studio};
use crate::tools::{explain_tool, use_tools};

const DEFAULT_TOOL_NAME: &str = "Pinterest";
const PICTURE_WORDS: [&str; 3] = ["картинка", "картинки", "картинок"];
const SEARCH_WORDS: [&str; 3] = ["пошук", "пошуки", "пошуків"];
const TOPIC_WORDS: [&str; 3] = ["тема", "теми", "тем"];
const SOUND_SOURCE: &str = "звучання для ComfyUI";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Pinterest,
    ArtStation,
}

impl Source {
    const ALL: [Self; 2] = [Self::Pinterest, Self::ArtStation];

    fn label(self) -> &'static str {
        match self {
            Self::Pinterest => "Pinterest",
            Self::ArtStation => "ArtStation",
        }
    }

    fn item_name(self) -> &'static str {
        match self {
            Self::Pinterest => "Пін",
            Self::ArtStation => "Робота",
        }
    }
}

#[derive(Clone, PartialEq)]
enum Picture {
    Pin(Pin),
    Artwork(Artwork),
}

impl Picture {
    fn source(&self) -> Source {
        match self {
            Self::Pin(_) => Source::Pinterest,
            Self::Artwork(_) => Source::ArtStation,
        }
    }

    fn image(&self) -> &str {
        match self {
            Self::Pin(pin) => &pin.image,
            Self::Artwork(artwork) => &artwork.image,
        }
    }

    fn detail_image(&self) -> String {
        match self {
            Self::Pin(pin) => pin.sized(DETAIL_PIN_SIZE),
            Self::Artwork(artwork) => artwork.sized(DETAIL_ARTWORK_SIZE),
        }
    }

    fn title(&self) -> &str {
        match self {
            Self::Pin(pin) => &pin.title,
            Self::Artwork(artwork) => &artwork.title,
        }
    }
}

#[derive(Clone, PartialEq)]
struct Viewed {
    picture: Picture,
    reference: String,
    index: usize,
}

#[derive(Clone, PartialEq)]
struct BoardPin {
    picture: Picture,
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

#[derive(Clone, Copy, PartialEq)]
struct SourceState {
    loading: bool,
    came_back_empty: bool,
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
                found: matching()
                    .map(|(_, reference)| reference.pins.len() + reference.artworks.len())
                    .sum(),
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
            let pins = reference.pins.iter().cloned().map(Picture::Pin);
            let artworks = reference.artworks.iter().cloned().map(Picture::Artwork);
            pins.chain(artworks).map(move |picture| BoardPin {
                picture,
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
    let mut artstation = use_signal(ArtstationStatus::default);

    use_resource(move || async move {
        let _ = (studio.revision)();
        if let Ok(status) = bridge::pinterest_status().await {
            pinterest.set(status);
        }
        if let Ok(status) = artstation_bridge::status().await {
            artstation.set(status);
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

    let mut load_artworks = move |project_id: i64| {
        notice.set(None);
        spawn(async move {
            match artstation_bridge::load_reference_artworks(project_id).await {
                Ok(()) => studio.changed(),
                Err(problem) => notice.set(Some(explain_artstation(&problem))),
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
        Some(project) => format!("Референси з Pinterest і ArtStation, які AI-ядро підібрало для «{}»", project.name),
        None => "Референси з Pinterest і ArtStation, які AI-ядро підбирає для кожної гри".to_owned(),
    };
    let ready = project
        .as_ref()
        .filter(|project| !project.status.is_planning() && !project.references.is_empty())
        .map(|project| project.id);
    let busy = picking().is_some();
    let status = pinterest();
    let art_status = artstation();
    let pins_loading = ready.is_some_and(|project_id| status.is_loading(project_id));
    let artworks_loading = ready.is_some_and(|project_id| art_status.is_loading(project_id));
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
                        button {
                            class: "ghost-button",
                            r#type: "button",
                            disabled: artworks_loading,
                            onclick: move |_| load_artworks(project_id),
                            if artworks_loading {
                                span { class: "spinner" }
                                "Шукаємо на ArtStation…"
                            } else {
                                "Оновити ArtStation"
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
                        p { "Увійди один раз у вікні Pinterest — сесія збережеться в Noctics, і AI-ядро саме шукатиме й підтягуватиме піни для кожної гри. Пароль вводиш лише на сторінці Pinterest. ArtStation працює й без входу." }
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
                        p { "Запусти конвеєр гри — AI-ядро разом із планом підбере дошку референсів і підтягне картинки з Pinterest і ArtStation." }
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
                        p { "Референси, піни й роботи з ArtStation з’являться разом із планом «{project.name}»." }
                    }
                },
                Some(project) if project.references.is_empty() => {
                    let project_id = project.id;
                    rsx! {
                        div { class: "page-empty",
                            p { class: "mono-label", "Референсів ще немає" }
                            p { "Цей проєкт створено до появи дошки референсів. AI-ядро розбере промт і підбере їх зараз — персонажів і стиль 3D-персонажів, інтерфейс, головні меню, панелі, кнопки, VFX і VFX-спрайти, а також світ гри." }
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
                    let pinterest_state = SourceState {
                        loading: pins_loading,
                        came_back_empty: status.came_back_empty(project.id),
                    };
                    let artstation_state = SourceState {
                        loading: artworks_loading,
                        came_back_empty: art_status.came_back_empty(project.id),
                    };
                    rsx! {
                        ReferenceBoard {
                            project,
                            pinterest: pinterest_state,
                            artstation: artstation_state,
                            connected: status.signed_in == Some(true),
                        }
                    }
                }
            }
            div { class: "local-apis",
                LocalPinterestApi {}
                LocalArtstationApi {}
            }
        }
    }
}

#[component]
fn LocalPinterestApi() -> Element {
    let address = format!("http://127.0.0.1:{PINTEREST_API_PORT}");
    rsx! {
        section { class: "local-api", aria_label: "Локальний Pinterest API",
            p { class: "mono-label", "Безкоштовний Pinterest API · локально" }
            p { class: "local-api-text",
                "Поки Noctics відкритий, будь-який твій проєкт чи скрипт може брати піни через нього — без RapidAPI, ключів і лімітів. Відповідь — JSON з id, image, title і link."
            }
            code { class: "local-api-address", "{address}/pins?q=cyberpunk+ui&limit=30" }
            p { class: "local-api-hint",
                "Довідка: {address}/ · стан: {address}/health · limit від 1 до 100. Приймає лише запити з цього комп’ютера."
            }
        }
    }
}

#[component]
fn LocalArtstationApi() -> Element {
    let address = format!("http://127.0.0.1:{ARTSTATION_API_PORT}");
    rsx! {
        section { class: "local-api", aria_label: "Локальний ArtStation API",
            p { class: "mono-label", "Безкоштовний ArtStation API · локально" }
            p { class: "local-api-text",
                "Шукає роботи на ArtStation для будь-якого твого проєкту чи скрипта — без ключів і входу. Ті самі запити, які Claude складає з промпту гри, ядро вже підтягує на дошку вище. Відповідь — JSON з id, title, author, image, image_large і link."
            }
            code { class: "local-api-address", "{address}/artworks?q=dark+fantasy+castle&limit=30" }
            p { class: "local-api-hint",
                "Довідка: {address}/ · стан: {address}/health · limit від 1 до 100 · роботи 18+ відсіяно. Приймає лише запити з цього комп’ютера."
            }
        }
    }
}

#[component]
fn ReferenceBoard(project: Project, pinterest: SourceState, artstation: SourceState, connected: bool) -> Element {
    let notice = use_signal(|| None::<String>);
    let mut viewing = use_signal(|| None::<Viewed>);
    let filter = use_signal(|| None::<ReferenceTopic>);
    let project_id = project.id;
    let count = project.references.len();
    let with_pins = project
        .references
        .iter()
        .filter(|reference| !reference.pins.is_empty())
        .count();
    let with_artworks = project
        .references
        .iter()
        .filter(|reference| !reference.artworks.is_empty())
        .count();
    let groups = search_groups(&project.analysis, &project.references);
    let pins = board_pins(&project.references);
    let searches = plural(amount(count), SEARCH_WORDS);
    let pictures = plural(amount(pins.len()), PICTURE_WORDS);
    let loading = pinterest.loading || artstation.loading;
    let has_pins = !pins.is_empty();

    rsx! {
        StylePanel { style: project.style.clone(), project_id }
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
            if pinterest.loading {
                span { class: "references-progress",
                    span { class: "spinner" }
                    "Шукаємо в Pinterest · {with_pins}/{count}"
                }
            } else if pinterest.came_back_empty {
                span { class: "references-warning",
                    "Pinterest нічого не віддав — увійди в акаунт і натисни «Оновити піни»."
                }
            }
            if artstation.loading {
                span { class: "references-progress",
                    span { class: "spinner" }
                    "Шукаємо на ArtStation · {with_artworks}/{count}"
                }
            } else if artstation.came_back_empty {
                span { class: "references-warning",
                    "ArtStation нічого не віддав — перевір інтернет і натисни «Оновити ArtStation»."
                }
            }
            if has_pins && !loading {
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
fn StylePanel(style: StyleGuide, project_id: i64) -> Element {
    let studio = use_studio();
    let mut restyling = use_signal(|| false);
    let mut notice = use_signal(|| None::<String>);
    let mood = if style.mood.trim().is_empty() {
        "AI-ядро ще не описало настрій гри.".to_owned()
    } else {
        style.mood.clone()
    };
    let explained: Vec<(String, String)> = style
        .palette
        .iter()
        .enumerate()
        .filter_map(|(index, color)| style.palette_note(index).map(|note| (color.clone(), note.to_owned())))
        .collect();
    let restyle = move |_| {
        if restyling() {
            return;
        }
        restyling.set(true);
        notice.set(None);
        spawn(async move {
            match bridge::restyle_project(project_id).await {
                Ok(_) => studio.changed(),
                Err(problem) => notice.set(Some(explain(&problem))),
            }
            restyling.set(false);
        });
    };

    rsx! {
        section { class: "style-panel",
            div { class: "style-panel-text",
                p { class: "mono-label", "Візуальний стиль · виведений зі світу гри" }
                p { class: "style-mood", "{mood}" }
                if !style.look.trim().is_empty() {
                    p { class: "style-look", "{style.look}" }
                }
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
                button {
                    class: "ghost-button is-small style-restyle",
                    r#type: "button",
                    title: "Claude заново виведе палітру, шрифти й вигляд зі світу гри, а Claude Design перемалює всі екрани",
                    disabled: restyling(),
                    onclick: restyle,
                    if restyling() {
                        span { class: "spinner" }
                        "Виводжу стиль зі світу…"
                    } else {
                        "Новий стиль і екрани"
                    }
                }
                if let Some(message) = notice() {
                    span { class: "references-warning", "{message}" }
                }
            }
            if !style.palette.is_empty() {
                div { class: "style-palette-side",
                    div { class: "style-palette", aria_label: "Палітра гри",
                        for (index, color) in style.palette.iter().enumerate() {
                            div {
                                key: "{index}",
                                class: "style-swatch",
                                title: style.palette_note(index).unwrap_or(color.as_str()),
                                span { class: "style-swatch-fill", style: "background: {color}" }
                                code { "{color}" }
                            }
                        }
                    }
                    if !explained.is_empty() {
                        ul { class: "style-palette-legend", aria_label: "Звідки кожен колір",
                            for (index, (color, note)) in explained.into_iter().enumerate() {
                                li { key: "{index}",
                                    span { class: "style-palette-dot", style: "background: {color}" }
                                    span { "{note}" }
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
                        "Цю дошку зібрано до появи аналізу. Натисни «Підібрати заново» — Claude розбере промт і складе план пошуку: персонажі й стиль 3D-персонажів, інтерфейс, головні меню, панелі, кнопки, VFX і VFX-спрайти, іконки, анімацію, звуки й музику."
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
    let audio = topic.is_audio();
    let found = if audio {
        SOUND_SOURCE.to_owned()
    } else {
        plural(amount(group.found), PICTURE_WORDS)
    };
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
                                    if audio {
                                        span {
                                            class: "reference-sound",
                                            title: "Теги звучання для генерації в ComfyUI",
                                            "{query}"
                                        }
                                    } else {
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
    let mut source = use_signal(|| None::<Source>);
    let chips: Vec<(ReferenceTopic, usize)> = groups
        .iter()
        .filter(|group| group.found > 0)
        .map(|group| (group.topic, group.found))
        .collect();
    let sources: Vec<(Source, usize)> = Source::ALL
        .into_iter()
        .map(|candidate| {
            let found = pins
                .iter()
                .filter(|item| item.picture.source() == candidate)
                .count();
            (candidate, found)
        })
        .filter(|(_, found)| *found > 0)
        .collect();
    let active = filter().filter(|topic| groups.iter().any(|group| group.topic == *topic));
    let active_source = source().filter(|picked| sources.iter().any(|(candidate, _)| candidate == picked));
    let total = pins.len();
    let shown: Vec<BoardPin> = pins
        .into_iter()
        .filter(|item| active.is_none_or(|topic| item.topic == topic))
        .filter(|item| active_source.is_none_or(|picked| item.picture.source() == picked))
        .collect();
    let shown_label = plural(amount(shown.len()), PICTURE_WORDS);
    let empty_text = match (active, active_source) {
        (Some(topic), Some(picked)) => format!("Для теми «{}» {} ще нічого не віддав.", topic.label(), picked.label()),
        (Some(topic), None) => format!("Для теми «{}» Pinterest і ArtStation ще нічого не віддали.", topic.label()),
        (None, _) => "Картинок ще немає — натисни «Оновити ArtStation» або увійди в Pinterest і натисни «Оновити піни».".to_owned(),
    };
    let ghosts = if loading { ARTWORKS_PER_REFERENCE } else { 0 };

    rsx! {
        section { class: "style-board", aria_label: "Усі картинки стилю гри",
            div { class: "style-board-head",
                p { class: "mono-label", "Дошка стилю · {shown_label}" }
                div { class: "style-board-tools",
                    if sources.len() > 1 {
                        div { class: "style-board-filters is-sources", role: "group", aria_label: "Фільтр за джерелом",
                            for (candidate, found) in sources {
                                {
                                    let selected = active_source == Some(candidate);
                                    rsx! {
                                        button {
                                            key: "{candidate.label()}",
                                            class: if selected { "board-chip is-active" } else { "board-chip" },
                                            r#type: "button",
                                            aria_pressed: if selected { "true" } else { "false" },
                                            onclick: move |_| source.set(if selected { None } else { Some(candidate) }),
                                            "{candidate.label()}"
                                            span { class: "board-chip-count", "{found}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
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
            }
            if shown.is_empty() && !loading {
                p { class: "style-board-empty", "{empty_text}" }
            } else {
                div { class: "style-board-grid", aria_busy: if loading { "true" } else { "false" },
                    for item in shown {
                        {
                            let key = format!("{}-{}", item.index, item.picture.image());
                            rsx! {
                                PinTile { key: "{key}", item, on_view }
                            }
                        }
                    }
                    for slot in 0..ghosts {
                        span { key: "ghost-{slot}", class: "board-ghost" }
                    }
                }
            }
        }
    }
}

#[component]
fn PinTile(item: BoardPin, on_view: EventHandler<Viewed>) -> Element {
    let source = item.picture.source();
    let label = if item.picture.title().is_empty() {
        format!("{} для «{}»", source.item_name(), item.reference)
    } else {
        item.picture.title().to_owned()
    };
    let image = item.picture.image().to_owned();
    let caption = format!("{} · {} · {}", source.label(), item.topic.label(), item.reference);

    rsx! {
        button {
            class: "board-pin",
            r#type: "button",
            title: "{label}",
            onclick: move |_| {
                on_view.call(Viewed {
                    picture: item.picture.clone(),
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
    let Viewed { picture, reference, .. } = viewed;
    let image = picture.detail_image();
    let title = picture.title().to_owned();
    let author = match &picture {
        Picture::Artwork(artwork) => artwork.author.clone(),
        Picture::Pin(_) => String::new(),
    };
    let action = match &picture {
        Picture::Pin(pin) if pin.is_linked() => "Відкрити пін у Pinterest",
        Picture::Pin(_) => "Шукати схожі в Pinterest",
        Picture::Artwork(_) => "Відкрити на ArtStation",
    };

    let open = move |_| match picture.clone() {
        Picture::Pin(pin) if !pin.is_linked() => on_search.call(()),
        Picture::Pin(pin) => {
            spawn(async move {
                if let Err(problem) = bridge::open_pin(&pin.id).await {
                    notice.set(Some(explain_pinterest(&problem)));
                }
            });
        }
        Picture::Artwork(artwork) => {
            spawn(async move {
                if let Err(problem) = artstation_bridge::open_artwork(&artwork.id).await {
                    notice.set(Some(explain_artstation(&problem)));
                }
            });
        }
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
                    alt: "{title}",
                    referrerpolicy: "no-referrer",
                    draggable: "false",
                }
                figcaption { class: "pin-viewer-caption",
                    span { class: "reference-topic", "{reference}" }
                    if !title.is_empty() {
                        span { class: "pin-viewer-title", "{title}" }
                    }
                    if !author.is_empty() {
                        span { class: "pin-viewer-author", "Автор: {author}" }
                    }
                    if let Some(message) = notice() {
                        span { class: "references-warning", "{message}" }
                    }
                    div { class: "pin-viewer-actions",
                        button { class: "primary-button is-small", r#type: "button", onclick: open,
                            "{action}"
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

pub(crate) fn explain_pinterest(problem: &PinterestError) -> String {
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

pub(crate) fn explain_artstation(problem: &ArtstationError) -> String {
    match problem {
        ArtstationError::SignedOut => "Сесія завершилась — увійди знову.".to_owned(),
        ArtstationError::NotFound => "Цього проєкту чи роботи вже немає.".to_owned(),
        ArtstationError::NoReferences => {
            "Спершу AI-ядро має підібрати дошку референсів.".to_owned()
        }
        ArtstationError::Network(detail) => {
            tracing::warn!("artstation request failed: {detail}");
            "Не вдалося дістатися до ArtStation. Перевір інтернет і спробуй ще раз.".to_owned()
        }
        ArtstationError::Storage(detail) => {
            tracing::error!("reference storage failed: {detail}");
            "Не вдалося зберегти роботи з ArtStation у проєкт.".to_owned()
        }
    }
}
