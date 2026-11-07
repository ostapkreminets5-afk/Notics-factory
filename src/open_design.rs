use dioxus::prelude::*;
use dioxus_logger::tracing;
use noctics_shared::{
    DesignRunStatus, DesignSource, DesignVariant, DesignVariation, OpenDesignBoard, OpenDesignError, Screen,
};

use crate::icons::{CloseIcon, ExpandIcon, ExternalIcon};
use crate::journal::plural;
use crate::modal::Modal;
use crate::open_design_bridge;
use crate::preview::ScreenPreview;
use crate::screen_gallery::Slide;
use crate::studio::use_studio;

pub const VARIANT_WORDS: [&str; 3] = ["варіація", "варіації", "варіацій"];
const OTHER_GROUP: &str = "Інші дизайни";
const ALL_SOURCES_LABEL: &str = "Усі";
const WAITING_NOTE: &str = "Open Design чекає, поки Pinterest і ArtStation доберуть референси, — тоді зробить варіації кожного екрана.";
const SOURCES_NOTE: &str = "Open Design робить 4 варіації кожного екрана: за промптом, за референсами Pinterest, за ArtStation і за обома дошками разом.";

pub fn use_open_design(focus: Memo<Option<i64>>) -> Signal<Option<OpenDesignBoard>> {
    let studio = use_studio();
    let mut board = use_signal(|| None::<OpenDesignBoard>);
    use_resource(move || async move {
        let _ = (studio.revision)();
        let Some(project_id) = focus() else {
            board.set(None);
            return;
        };
        if let Ok(fresh) = open_design_bridge::board(project_id).await {
            board.set(Some(fresh));
        }
    });
    board
}

pub fn status_label(status: DesignRunStatus) -> &'static str {
    match status {
        DesignRunStatus::Idle => "Ще не запускали",
        DesignRunStatus::Starting => "Запускається",
        DesignRunStatus::WaitingReferences => "Чекає референси",
        DesignRunStatus::Running => "Генерує",
        DesignRunStatus::Done => "Готово",
        DesignRunStatus::Failed => "Помилка",
    }
}

fn status_tone(status: DesignRunStatus) -> &'static str {
    match status {
        DesignRunStatus::Idle => "is-draft",
        DesignRunStatus::Starting | DesignRunStatus::WaitingReferences => "is-queued",
        DesignRunStatus::Running => "is-generating",
        DesignRunStatus::Done => "is-approved",
        DesignRunStatus::Failed => "is-failed",
    }
}

pub fn skipped_note(board: &OpenDesignBoard) -> Option<String> {
    if board.skipped.is_empty() || board.rounds == 0 {
        return None;
    }
    let names = board
        .skipped
        .iter()
        .map(|source| format!("«{}»", source.label()))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "Останній раунд пройшов без варіацій {names} — на дошці референсів не було картинок звідти. Підтягни піни Pinterest і роботи ArtStation, тоді натисни «Ще варіації»."
    ))
}

fn source_counts(board: &OpenDesignBoard) -> Vec<(DesignSource, usize)> {
    DesignSource::ALL
        .into_iter()
        .map(|source| (source, board.count_from(source)))
        .filter(|(_, count)| *count > 0)
        .collect()
}

fn passes(filter: Option<DesignSource>, variant: &DesignVariant) -> bool {
    filter.is_none_or(|source| variant.source == source)
}

#[component]
pub fn SourceBadge(source: DesignSource) -> Element {
    rsx! {
        span { class: "od-source is-source-{source.code()}",
            span { class: "od-source-dot" }
            "{source.short_label()}"
        }
    }
}

#[component]
fn SourceFilters(
    board: OpenDesignBoard,
    active: Option<DesignSource>,
    nodes: bool,
    on_pick: EventHandler<Option<DesignSource>>,
) -> Element {
    let counts = source_counts(&board);
    if counts.len() < 2 {
        return rsx! {};
    }
    let (group, chip, count_class) = if nodes {
        ("nf-od-filters", "nf-od-filter", "nf-od-filter-count")
    } else {
        ("od-filters", "od-filter", "od-filter-count")
    };
    let all_state = if active.is_none() { "is-active" } else { "" };
    rsx! {
        div { class: "{group}", role: "group", aria_label: "Джерело варіацій",
            button {
                class: "{chip} is-source-all {all_state}",
                r#type: "button",
                aria_pressed: if active.is_none() { "true" } else { "false" },
                onclick: move |_| on_pick.call(None),
                "{ALL_SOURCES_LABEL}"
                span { class: "{count_class}", "{board.variants.len()}" }
            }
            for (source, count) in counts {
                button {
                    key: "{source.code()}",
                    class: if active == Some(source) { "{chip} is-source-{source.code()} is-active" } else { "{chip} is-source-{source.code()}" },
                    r#type: "button",
                    aria_pressed: if active == Some(source) { "true" } else { "false" },
                    onclick: move |_| on_pick.call(Some(source)),
                    span { class: "od-source-dot" }
                    "{source.label()}"
                    span { class: "{count_class}", "{count}" }
                }
            }
        }
    }
}

pub fn start_label(board: &OpenDesignBoard) -> &'static str {
    if board.rounds == 0 {
        "Згенерувати в Open Design"
    } else {
        "Ще варіації"
    }
}

pub fn variants_summary(board: &OpenDesignBoard) -> String {
    let count = plural(amount(board.variants.len()), VARIANT_WORDS);
    match &board.design_system {
        Some(system) => format!("{count} · дизайн-система {system}"),
        None => count,
    }
}

fn amount(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn grouped(board: &OpenDesignBoard, screens: &[Screen], filter: Option<DesignSource>) -> Vec<(String, Vec<DesignVariant>)> {
    let mut groups: Vec<(String, Vec<DesignVariant>)> = screens
        .iter()
        .filter_map(|screen| {
            let variants: Vec<DesignVariant> = board
                .for_screen(screen.position)
                .into_iter()
                .filter(|variant| passes(filter, variant))
                .cloned()
                .collect();
            (!variants.is_empty()).then(|| (format!("{:02} · {}", screen.position, screen.title), variants))
        })
        .collect();
    let others: Vec<DesignVariant> = board
        .variants
        .iter()
        .filter(|variant| {
            passes(filter, variant)
                && variant
                    .screen
                    .is_none_or(|position| !screens.iter().any(|screen| screen.position == position))
        })
        .cloned()
        .collect();
    if !others.is_empty() {
        groups.push((OTHER_GROUP.to_owned(), others));
    }
    groups
}

fn working_note(board: &OpenDesignBoard) -> String {
    if board.status == DesignRunStatus::WaitingReferences {
        return WAITING_NOTE.to_owned();
    }
    format!(
        "Claude Code в Open Design робить варіації кожного екрана за промптом, за Pinterest, за ArtStation і за обома дошками разом{}. Готові з’являються тут одразу.",
        board
            .design_system
            .as_ref()
            .map(|system| format!(" у дизайн-системі {system}"))
            .unwrap_or_default()
    )
}

fn progress_note(status: DesignRunStatus) -> &'static str {
    if status == DesignRunStatus::WaitingReferences {
        "Чекає на референси Pinterest і ArtStation — до 6 хвилин."
    } else {
        "Open Design працює — 4 варіації на кожен екран займають до години, можна працювати далі."
    }
}

#[component]
pub fn OpenDesignSection(project_id: i64, screens: Vec<Screen>) -> Element {
    let studio = use_studio();
    let mut board = use_signal(|| None::<OpenDesignBoard>);
    let mut notice = use_signal(|| None::<String>);
    let mut starting = use_signal(|| false);
    let mut viewing = use_signal(|| None::<DesignVariant>);
    let mut filter = use_signal(|| None::<DesignSource>);

    use_resource(use_reactive!(|(project_id,)| async move {
        let _ = (studio.revision)();
        if let Ok(fresh) = open_design_bridge::board(project_id).await {
            board.set(Some(fresh));
        }
    }));

    let Some(current) = board().filter(|loaded| loaded.project_id == project_id) else {
        return rsx! {};
    };

    let start = move |_| {
        if starting() {
            return;
        }
        starting.set(true);
        notice.set(None);
        spawn(async move {
            match open_design_bridge::start(project_id).await {
                Ok(fresh) => board.set(Some(fresh)),
                Err(problem) => notice.set(Some(explain_open_design(&problem))),
            }
            starting.set(false);
        });
    };

    let open_folder = move |_| {
        spawn(async move {
            if let Err(problem) = open_design_bridge::open_folder(project_id).await {
                notice.set(Some(explain_open_design(&problem)));
            }
        });
    };

    let busy = current.status.is_busy();
    let active = filter().filter(|source| current.count_from(*source) > 0);
    let groups = grouped(&current, &screens, active);
    let has_variants = !current.variants.is_empty();
    let skipped = skipped_note(&current).filter(|_| !busy);
    let note = if !current.installed {
        "Open Design не встановлено на цьому комп’ютері — встанови його з open-design.ai, і тут з’являться інші дизайни кожного екрана.".to_owned()
    } else if busy {
        working_note(&current)
    } else if has_variants {
        format!("{}. Натисни на варіацію, щоб роздивитись її ближче.", variants_summary(&current))
    } else {
        SOURCES_NOTE.to_owned()
    };

    rsx! {
        section { class: "od-section", aria_label: "Open Design — варіації екранів",
            div { class: "od-head",
                div { class: "od-head-text",
                    p { class: "mono-label", "Open Design · паралельно з Claude Design" }
                    h2 { class: "od-title",
                        "Інші дизайни й варіації"
                        span { class: "design-status {status_tone(current.status)}", "{status_label(current.status)}" }
                    }
                    p { class: "od-note", "{note}" }
                }
                div { class: "od-actions",
                    if has_variants {
                        button { class: "ghost-button is-small", r#type: "button", onclick: open_folder,
                            "Папка"
                            ExternalIcon {}
                        }
                    }
                    button {
                        class: "primary-button is-small",
                        r#type: "button",
                        disabled: !current.installed || busy || starting(),
                        onclick: start,
                        if starting() {
                            span { class: "spinner" }
                        }
                        "{start_label(&current)}"
                    }
                }
            }
            if busy {
                div { class: "od-progress", role: "status",
                    span { class: "spinner" }
                    span { "{progress_note(current.status)}" }
                }
            }
            if let Some(error) = current.error.clone().filter(|_| current.status == DesignRunStatus::Failed) {
                p { class: "alert", role: "alert", "{error}" }
            }
            if let Some(message) = notice() {
                p { class: "alert", role: "alert", "{message}" }
            }
            if let Some(text) = skipped {
                p { class: "od-skipped", "{text}" }
            }
            if has_variants {
                SourceFilters {
                    board: current.clone(),
                    active,
                    nodes: false,
                    on_pick: move |picked| filter.set(picked),
                }
            }
            if groups.is_empty() {
                if current.installed && !busy {
                    p { class: "od-empty", "Варіацій ще немає — натисни «{start_label(&current)}»." }
                }
            } else {
                div { class: "od-groups",
                    for (title, variants) in groups {
                        div { key: "{title}", class: "od-group",
                            p { class: "mono-label", "{title} · {plural(amount(variants.len()), VARIANT_WORDS)}" }
                            div { class: "design-grid",
                                for variant in variants {
                                    VariantCard {
                                        key: "{variant.file}",
                                        variant,
                                        on_open: move |picked| viewing.set(Some(picked)),
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(variant) = viewing() {
            VariantViewer {
                project_id,
                variant,
                on_close: move |_| viewing.set(None),
            }
        }
    }
}

#[component]
pub fn VariantCard(variant: DesignVariant, on_open: EventHandler<DesignVariant>) -> Element {
    let opened = variant.clone();
    let entered = variant.clone();
    rsx! {
        article {
            class: "design-card",
            tabindex: "0",
            aria_label: "{variant.title} — відкрити варіацію",
            onclick: move |_| on_open.call(opened.clone()),
            onkeydown: move |event| {
                if event.key() == Key::Enter {
                    on_open.call(entered.clone());
                }
            },
            ScreenPreview {
                html: None,
                src: Some(variant.preview_url.clone()),
                label: variant.title.clone(),
            }
            div { class: "design-card-body",
                div { class: "design-card-head",
                    h3 { class: "design-card-title", "{variant.title}" }
                    SourceBadge { source: variant.source }
                }
                p { class: "design-card-summary", "{variant.file}" }
            }
        }
    }
}

#[component]
pub fn VariantViewer(project_id: i64, variant: DesignVariant, on_close: EventHandler<()>) -> Element {
    let mut notice = use_signal(|| None::<String>);
    let file = variant.file.clone();

    let open_in_browser = move |_| {
        let file = file.clone();
        spawn(async move {
            if let Err(problem) = open_design_bridge::open_variant(project_id, &file).await {
                notice.set(Some(explain_open_design(&problem)));
            }
        });
    };
    let open_folder = move |_| {
        spawn(async move {
            if let Err(problem) = open_design_bridge::open_folder(project_id).await {
                notice.set(Some(explain_open_design(&problem)));
            }
        });
    };

    rsx! {
        Modal { label: variant.title.clone(), wide: true, on_close,
            header { class: "editor-head",
                div {
                    p { class: "mono-label", "Open Design · {variant.source.label()} · {variant.file}" }
                    h2 { class: "editor-title", "{variant.title}" }
                }
                div { class: "editor-head-side",
                    button {
                        class: "icon-button",
                        r#type: "button",
                        aria_label: "Закрити",
                        onclick: move |_| on_close.call(()),
                        CloseIcon {}
                    }
                }
            }
            div { class: "editor-body viewer-body",
                ScreenPreview {
                    html: None,
                    src: Some(variant.preview_url.clone()),
                    label: variant.title.clone(),
                    live: true,
                }
                if let Some(message) = notice() {
                    p { class: "alert", role: "alert", "{message}" }
                }
            }
            footer { class: "editor-foot",
                button { class: "ghost-button", r#type: "button", onclick: open_folder,
                    "Папка проєкту"
                }
                div { class: "editor-foot-actions",
                    button { class: "primary-button", r#type: "button", onclick: open_in_browser,
                        "Відкрити в браузері"
                        ExternalIcon {}
                    }
                }
            }
        }
    }
}

#[component]
pub fn OpenDesignPanel(project_id: i64, board: Option<OpenDesignBoard>, opened: Signal<Option<usize>>) -> Element {
    let studio = use_studio();
    let mut starting = use_signal(|| false);
    let mut notice = use_signal(|| None::<String>);

    let start = move |_| {
        if starting() {
            return;
        }
        starting.set(true);
        notice.set(None);
        spawn(async move {
            match open_design_bridge::start(project_id).await {
                Ok(_) => studio.changed(),
                Err(problem) => notice.set(Some(explain_open_design(&problem))),
            }
            starting.set(false);
        });
    };
    let open_folder = move |_| {
        spawn(async move {
            if let Err(problem) = open_design_bridge::open_folder(project_id).await {
                notice.set(Some(explain_open_design(&problem)));
            }
        });
    };

    let Some(board) = board else {
        return rsx! {
            p { class: "nf-inspector-lead", "Перевіряємо Open Design…" }
        };
    };
    let busy = board.status.is_busy();
    let skipped = skipped_note(&board).filter(|_| !busy);
    let variations = board.variations();
    let lead = if !board.installed {
        "Open Design не встановлено — встанови його з open-design.ai.".to_owned()
    } else if busy {
        working_note(&board)
    } else if board.variants.is_empty() {
        SOURCES_NOTE.to_owned()
    } else {
        variants_summary(&board)
    };

    rsx! {
        if let Some(text) = notice() {
            p { class: "nf-notice", role: "alert", "{text}" }
        }
        if let Some(error) = board.error.clone().filter(|_| board.status == DesignRunStatus::Failed) {
            p { class: "nf-notice", role: "alert", "{error}" }
        }
        if let Some(text) = skipped {
            p { class: "nf-notice is-warning", "{text}" }
        }
        p { class: "nf-inspector-lead", "{lead}" }
        div { class: "nf-actions",
            button {
                class: "nf-button is-primary",
                r#type: "button",
                disabled: !board.installed || busy || starting(),
                onclick: start,
                if busy || starting() {
                    span { class: "nf-spinner" }
                    "Генерує"
                } else {
                    "{start_label(&board)}"
                }
            }
            if !board.variants.is_empty() {
                button { class: "nf-button is-quiet", r#type: "button", onclick: open_folder,
                    ExternalIcon {}
                    "Папка"
                }
            }
        }
        if !variations.is_empty() {
            div { class: "nf-variations", role: "group", aria_label: "Варіації дизайну",
                for variation in variations {
                    button {
                        key: "{variation.number}",
                        class: "nf-variation is-source-{variation.source.code()}",
                        r#type: "button",
                        title: "{variation_title(&variation)} — відкрити всі екрани",
                        aria_label: "{variation_title(&variation)} — відкрити всі екрани",
                        onclick: {
                            let number = variation.number;
                            let mut opened = opened;
                            move |_| opened.set(Some(number))
                        },
                        span { class: "nf-variation-name", "{variation.label()}" }
                        span { class: "nf-variation-source",
                            span { class: "od-source-dot" }
                            "{variation.source.short_label()}"
                        }
                        span { class: "nf-variation-count", "{variation.variants.len()}" }
                    }
                }
            }
        }
    }
}

pub fn variation_title(variation: &DesignVariation) -> String {
    let round = match variation.round.as_str() {
        "" => String::new(),
        tag if tag.chars().all(|letter| letter.is_ascii_digit()) => format!(" · раунд {tag}"),
        tag => format!(" · {tag}"),
    };
    format!("{} · {}{round}", variation.label(), variation.source.label())
}

fn tile_title(variant: &DesignVariant, screens: &[Screen]) -> String {
    variant
        .screen
        .and_then(|position| screens.iter().find(|screen| screen.position == position))
        .map_or_else(|| variant.title.clone(), |screen| screen.title.clone())
}

pub fn variation_slides(variation: &DesignVariation, screens: &[Screen]) -> Vec<Slide> {
    variation
        .variants
        .iter()
        .enumerate()
        .map(|(index, variant)| Slide {
            id: i64::try_from(index).unwrap_or(i64::MAX),
            title: tile_title(variant, screens),
            summary: variant.title.clone(),
            phase: "done",
            status: variation.source.label(),
            html: None,
            src: Some(variant.preview_url.clone()),
        })
        .collect()
}

#[component]
pub fn VariationBoard(
    project_id: i64,
    variation: DesignVariation,
    screens: Vec<Screen>,
    on_view: EventHandler<i64>,
    on_close: EventHandler<()>,
) -> Element {
    let mut notice = use_signal(|| None::<String>);
    let heading = variation_title(&variation);
    let total = plural(amount(variation.variants.len()), ["екран", "екрани", "екранів"]);
    let tiles: Vec<(i64, String, String, DesignVariant)> = variation
        .variants
        .iter()
        .enumerate()
        .map(|(index, variant)| {
            let number = variant
                .screen
                .map_or_else(|| format!("{:02}", index + 1), |position| format!("{position:02}"));
            (
                i64::try_from(index).unwrap_or(i64::MAX),
                number,
                tile_title(variant, &screens),
                variant.clone(),
            )
        })
        .collect();
    let open_variant = move |file: String| {
        spawn(async move {
            if let Err(problem) = open_design_bridge::open_variant(project_id, &file).await {
                notice.set(Some(explain_open_design(&problem)));
            }
        });
    };

    rsx! {
        div {
            class: "nf-board",
            role: "dialog",
            aria_modal: "true",
            aria_label: "{heading}",
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
                h2 { class: "nf-board-title", title: "{heading}", "{heading}" }
                span { class: "nf-board-sub", "Open Design" }
                span { class: "nf-board-count", "{total}" }
                button {
                    class: "nf-inspector-close",
                    r#type: "button",
                    title: "Закрити (Esc)",
                    aria_label: "Закрити варіацію",
                    onclick: move |_| on_close.call(()),
                    CloseIcon {}
                }
            }
            div { class: "nf-board-grid",
                if let Some(text) = notice() {
                    p { class: "nf-notice", role: "alert", "{text}" }
                }
                for (slide_id, number, title, variant) in tiles {
                    article { key: "{variant.file}", class: "nf-board-tile",
                        button {
                            class: "nf-preview",
                            r#type: "button",
                            title: "Відкрити «{title}» на весь екран",
                            aria_label: "Відкрити «{title}» на весь екран",
                            onclick: move |_| on_view.call(slide_id),
                            iframe {
                                title: "{title}",
                                tabindex: "-1",
                                "sandbox": "",
                                "scrolling": "no",
                                "loading": "lazy",
                                src: "{variant.preview_url}",
                            }
                            span { class: "nf-board-number", "{number}" }
                            span { class: "nf-preview-hint",
                                ExpandIcon {}
                                "На весь екран"
                            }
                        }
                        div { class: "nf-item-head",
                            span { class: "nf-led is-done" }
                            span { class: "nf-item-title", title: "{title}", "{title}" }
                            SourceBadge { source: variant.source }
                        }
                        if variant.title != title {
                            p { class: "nf-board-note", title: "{variant.title}", "{variant.title}" }
                        }
                        div { class: "nf-actions",
                            button {
                                class: "nf-button is-quiet",
                                r#type: "button",
                                onclick: {
                                    let file = variant.file.clone();
                                    move |_| open_variant(file.clone())
                                },
                                ExternalIcon {}
                                "Відкрити в браузері"
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn explain_open_design(problem: &OpenDesignError) -> String {
    match problem {
        OpenDesignError::SignedOut => "Сесія завершилась — увійди знову.".to_owned(),
        OpenDesignError::NotInstalled => "Open Design не встановлено — завантаж його з open-design.ai.".to_owned(),
        OpenDesignError::NotFound => "Цього проєкту чи варіації вже немає.".to_owned(),
        OpenDesignError::NotPlanned => "Спершу Claude має спланувати екрани гри.".to_owned(),
        OpenDesignError::Daemon(detail) => {
            tracing::warn!("open design daemon failed: {detail}");
            format!("Open Design не відповідає: {detail}")
        }
        OpenDesignError::Storage(detail) => {
            tracing::error!("open design storage failed: {detail}");
            "Не вдалося зберегти стан Open Design.".to_owned()
        }
    }
}
