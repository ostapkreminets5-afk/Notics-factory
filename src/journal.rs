use dioxus::prelude::*;
use js_sys::Date;
use noctics_shared::{
    JournalEntry, JournalEvent, JournalSource, JournalTone, LAST_STAGE, PlaytestVerdict, format_size,
};
use wasm_bindgen::JsValue;

use crate::bridge;
use crate::icons::{ChevronRightIcon, SearchIcon};
use crate::stage_track::{Recent, StageTrack};
use crate::studio::{Section, Studio, explain, use_studio};

const MILLIS_PER_SECOND: f64 = 1000.0;
const MILLIS_PER_DAY: f64 = 86_400_000.0;
const MONTHS: [&str; 12] = [
    "січня",
    "лютого",
    "березня",
    "квітня",
    "травня",
    "червня",
    "липня",
    "серпня",
    "вересня",
    "жовтня",
    "листопада",
    "грудня",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Only(JournalSource),
}

impl Filter {
    fn admits(self, source: JournalSource) -> bool {
        match self {
            Self::All => true,
            Self::Only(only) => only == source,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Day {
    year: u32,
    month: u32,
    date: u32,
}

impl Day {
    fn of(moment: &Date) -> Self {
        Self {
            year: moment.get_full_year(),
            month: moment.get_month(),
            date: moment.get_date(),
        }
    }

    fn key(self) -> String {
        format!("{}-{}-{}", self.year, self.month, self.date)
    }
}

struct Calendar {
    today: Day,
    yesterday: Day,
}

impl Calendar {
    fn now() -> Self {
        Self {
            today: Day::of(&Date::new_0()),
            yesterday: Day::of(&Date::new(&JsValue::from_f64(Date::now() - MILLIS_PER_DAY))),
        }
    }

    fn label(&self, day: Day) -> String {
        let month = MONTHS.get(day.month as usize).copied().unwrap_or_default();
        let date = if day.year == self.today.year {
            format!("{} {month}", day.date)
        } else {
            format!("{} {month} {}", day.date, day.year)
        };
        if day == self.today {
            format!("Сьогодні · {date}")
        } else if day == self.yesterday {
            format!("Учора · {date}")
        } else {
            date
        }
    }
}

struct Line {
    id: i64,
    project_id: i64,
    source: JournalSource,
    tone: JournalTone,
    stage: Option<u8>,
    finds: bool,
    text: String,
    detail: Option<String>,
    day: Day,
    clock: String,
    stamp: String,
    haystack: String,
}

impl Line {
    fn of(entry: &JournalEntry) -> Self {
        let moment = Date::new(&JsValue::from_f64(entry.created_at as f64 * MILLIS_PER_SECOND));
        let source = entry.event.source();
        let text = describe(&entry.event);
        let detail = entry.event.error().map(explain);
        let haystack = [Some(text.as_str()), detail.as_deref(), Some(source.label())]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        Self {
            id: entry.id,
            project_id: entry.project_id,
            source,
            tone: entry.event.tone(),
            stage: entry.event.stage(),
            finds: matches!(
                entry.event,
                JournalEvent::AssetsFound { .. } | JournalEvent::AssetHuntFailed { .. }
            ),
            text,
            detail,
            day: Day::of(&moment),
            clock: format!("{:02}:{:02}", moment.get_hours(), moment.get_minutes()),
            stamp: format!(
                "{:02}.{:02}.{} {:02}:{:02}:{:02}",
                moment.get_date(),
                moment.get_month() + 1,
                moment.get_full_year(),
                moment.get_hours(),
                moment.get_minutes(),
                moment.get_seconds()
            ),
            haystack,
        }
    }

    fn matches(&self, needle: &str) -> bool {
        needle.is_empty() || self.haystack.contains(needle)
    }
}

#[component]
pub fn JournalPage() -> Element {
    let studio = use_studio();
    let mut filter = use_signal(|| Filter::All);
    let mut query = use_signal(String::new);

    let journal = use_resource(move || async move {
        let _ = (studio.revision)();
        let project_id = studio.focused_project()?.id;
        Some(bridge::project_journal(project_id).await)
    });

    let focused = studio.focused_project();
    let focused_id = focused.as_ref().map(|project| project.id);
    let loaded = journal.read().is_some();
    let (entries, problem) = match journal.read().clone().flatten() {
        Some(Ok(entries)) => (entries, None),
        Some(Err(problem)) => (Vec::new(), Some(explain(&problem))),
        None => (Vec::new(), None),
    };
    let lines: Vec<Line> = entries
        .iter()
        .filter(|entry| Some(entry.project_id) == focused_id)
        .map(Line::of)
        .collect();
    let starts: Vec<(u8, i64)> = entries
        .iter()
        .filter(|entry| Some(entry.project_id) == focused_id)
        .filter(|entry| entry.event.tone() == JournalTone::Working)
        .filter_map(|entry| entry.event.stage().map(|stage| (stage, entry.created_at)))
        .collect();
    let recent = lines.first().map(|line| Recent {
        clock: line.clock.clone(),
        source: line.source.label().to_owned(),
        text: line.text.clone(),
    });

    let sources: Vec<JournalSource> = JournalSource::ALL
        .into_iter()
        .filter(|source| lines.iter().any(|line| line.source == *source))
        .collect();
    let active = match filter() {
        Filter::Only(source) if !sources.contains(&source) => Filter::All,
        chosen => chosen,
    };
    let needle = query.read().trim().to_lowercase();
    let found: Vec<&Line> = lines.iter().filter(|line| line.matches(&needle)).collect();
    let tally = |source: JournalSource| found.iter().filter(|line| line.source == source).count();
    let total = found.len();

    let calendar = Calendar::now();
    let mut days: Vec<(Day, Vec<&Line>)> = Vec::new();
    for line in found.iter().copied().filter(|line| active.admits(line.source)) {
        match days.last_mut() {
            Some((day, rows)) if *day == line.day => rows.push(line),
            _ => days.push((line.day, vec![line])),
        }
    }

    let lead = if focused.is_some() {
        "Що ядро робить зараз, на якому етапі проєкт і все, що з ним сталося."
    } else {
        "Події та рішення AI-ядра з’являться тут, щойно запустиш конвеєр гри."
    };

    rsx! {
        section { class: "page journal-page",
            header { class: "page-head",
                div { class: "page-head-text",
                    p { class: "mono-label", "AI-ядро / Журнал" }
                    h1 { class: "page-title", "Журнал ядра" }
                    p { class: "page-lead", "{lead}" }
                }
            }
            if let Some(project_id) = focused_id {
                StageTrack {
                    key: "{project_id}",
                    project_id,
                    starts,
                    recent,
                }
            }
            if !lines.is_empty() {
                div { class: "journal-toolbar",
                    div { class: "chip-row journal-chips", role: "tablist", aria_label: "Системи ядра",
                        button {
                            class: if active == Filter::All { "journal-chip is-all is-active" } else { "journal-chip is-all" },
                            r#type: "button",
                            role: "tab",
                            aria_selected: if active == Filter::All { "true" } else { "false" },
                            onclick: move |_| filter.set(Filter::All),
                            span { class: "journal-chip-dot" }
                            span { "Усі" }
                            span { class: "journal-chip-count", "{total}" }
                        }
                        for source in sources.iter().copied() {
                            button {
                                key: "{source.code()}",
                                class: if active == Filter::Only(source) { "journal-chip is-{source.code()} is-active" } else { "journal-chip is-{source.code()}" },
                                r#type: "button",
                                role: "tab",
                                aria_selected: if active == Filter::Only(source) { "true" } else { "false" },
                                onclick: move |_| filter.set(Filter::Only(source)),
                                span { class: "journal-chip-dot" }
                                span { "{source.label()}" }
                                span { class: "journal-chip-count", "{tally(source)}" }
                            }
                        }
                    }
                    label { class: "journal-search",
                        SearchIcon {}
                        input {
                            class: "journal-search-input",
                            r#type: "search",
                            placeholder: "Пошук у журналі",
                            aria_label: "Пошук у журналі",
                            value: "{query}",
                            oninput: move |event| query.set(event.value()),
                            onkeydown: move |event| {
                                if event.key() == Key::Escape {
                                    query.set(String::new());
                                }
                            },
                        }
                    }
                }
            }
            if let Some(message) = problem {
                div { class: "page-alert", role: "alert", span { "{message}" } }
            }
            if focused.is_none() {
                div { class: "page-empty",
                    p { class: "mono-label", "Проєктів ще немає" }
                    p { "Опиши гру в конвеєрі — ядро почне записувати сюди кожен свій крок." }
                    button {
                        class: "primary-button is-start",
                        r#type: "button",
                        onclick: move |_| studio.open(Section::Pipeline),
                        "Конвеєр гри"
                    }
                }
            } else if loaded && lines.is_empty() {
                div { class: "page-empty",
                    p { class: "mono-label", "Журнал порожній" }
                    p {
                        "Ядро записуватиме сюди план, роботу навичок, екрани Claude Design, референси з Pinterest, код гри й білди — щойно конвеєр візьметься до цього проєкту."
                    }
                }
            } else if !lines.is_empty() && days.is_empty() {
                div { class: "page-empty",
                    p { class: "mono-label", "Нічого не знайдено" }
                    p { "Спробуй інший запит або обери «Усі»." }
                }
            } else if !days.is_empty() {
                div { class: "journal",
                    for (day, rows) in days {
                        section { key: "{day.key()}", class: "journal-day",
                            p { class: "journal-day-label", "{calendar.label(day)}" }
                            ol { class: "journal-list",
                                for line in rows {
                                    JournalRow {
                                        key: "{line.id}",
                                        source: line.source,
                                        tone: line.tone,
                                        stage: line.stage,
                                        finds: line.finds,
                                        text: line.text.clone(),
                                        detail: line.detail.clone(),
                                        clock: line.clock.clone(),
                                        stamp: line.stamp.clone(),
                                        project_id: line.project_id,
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
fn JournalRow(
    source: JournalSource,
    tone: JournalTone,
    stage: Option<u8>,
    finds: bool,
    text: String,
    detail: Option<String>,
    clock: String,
    stamp: String,
    project_id: i64,
) -> Element {
    let studio = use_studio();
    let builds = source == JournalSource::Core && stage == Some(LAST_STAGE);

    rsx! {
        li {
            button {
                class: "journal-row is-{tone.code()}",
                r#type: "button",
                title: "{destination(source, builds, finds)}",
                onclick: move |_| open_source(studio, source, builds, finds, project_id),
                time { class: "journal-time", title: "{stamp}", "{clock}" }
                span { class: "journal-source is-{source.code()}",
                    span { class: "journal-source-dot" }
                    span { class: "journal-source-name", "{source.label()}" }
                }
                span { class: "journal-text",
                    span { class: "journal-message", "{text}" }
                    if let Some(reason) = detail {
                        span { class: "journal-detail", "{reason}" }
                    }
                }
                span { class: "journal-go", ChevronRightIcon {} }
            }
        }
    }
}

fn open_source(studio: Studio, source: JournalSource, builds: bool, finds: bool, project_id: i64) {
    match source {
        JournalSource::Core if builds => studio.open(Section::Build),
        JournalSource::Core => studio.open(Section::Core),
        JournalSource::References if finds => studio.open(Section::AssetHunt),
        JournalSource::References => studio.open(Section::Pinterest),
        JournalSource::Skills | JournalSource::Interface => studio.open_project(project_id),
    }
}

fn destination(source: JournalSource, builds: bool, finds: bool) -> &'static str {
    match source {
        JournalSource::Core if builds => "Відкрити збірку",
        JournalSource::Core => "Відкрити AI-ядро",
        JournalSource::References if finds => "Відкрити «Пошук ассетів»",
        JournalSource::References => "Відкрити референси Pinterest",
        JournalSource::Skills | JournalSource::Interface => "Відкрити проєкт у Claude Design",
    }
}

fn describe(event: &JournalEvent) -> String {
    match event {
        JournalEvent::ProjectStarted { engine } => {
            format!("Промпт прийнято, конвеєр запущено · {}", engine.label())
        }
        JournalEvent::PlanReady {
            name,
            screens,
            skills,
        } => {
            let screens = plural(*screens, ["екран", "екрани", "екранів"]);
            if *skills == 0 {
                format!("План «{name}» готовий: {screens}")
            } else {
                let skills = plural(*skills, ["навичка", "навички", "навичок"]);
                format!("План «{name}» готовий: {screens}, {skills} у черзі")
            }
        }
        JournalEvent::PlanFailed { .. } => "Планування гри не вдалося".to_owned(),
        JournalEvent::SkillsSynced { count } => format!(
            "У конвеєр додано {}",
            plural(*count, ["нову навичку", "нові навички", "нових навичок"])
        ),
        JournalEvent::SkillStarted { title, .. } => format!("Навичка «{title}» почала роботу"),
        JournalEvent::SkillDone { title, .. } => format!("«{title}» написала документ"),
        JournalEvent::SkillFailed { title, .. } => format!("Навичка «{title}» не відпрацювала"),
        JournalEvent::SkillRequeued { title, .. } => format!("Навичку «{title}» перезапущено"),
        JournalEvent::ScreenAdded { title } => format!("Додано екран «{title}»"),
        JournalEvent::ScreenStarted { title, revision } => {
            if *revision {
                format!("Почато правки екрана «{title}»")
            } else {
                format!("Почато екран «{title}»")
            }
        }
        JournalEvent::ScreenDone { title, version } => {
            if *version > 1 {
                format!("«{title}» перероблено за коментарем, ревізія {version}")
            } else {
                format!("Екран «{title}» готовий")
            }
        }
        JournalEvent::ScreenFailed { title, .. } => format!("Екран «{title}» не згенеровано"),
        JournalEvent::ScreenRefined { title } => {
            format!("Правки до екрана «{title}» поставлено в чергу")
        }
        JournalEvent::ScreenRequeued { title } => {
            format!("Екран «{title}» поставлено на перегенерацію")
        }
        JournalEvent::ScreenApproved { title } => format!("Екран «{title}» апрувнуто"),
        JournalEvent::BoardsPicked { count } => format!(
            "Ядро підібрало {} референсів",
            plural(*count, ["дошку", "дошки", "дошок"])
        ),
        JournalEvent::PinsImported { board, count } => format!(
            "{} для референсу «{board}»",
            plural(*count, ["пін", "піни", "пінів"])
        ),
        JournalEvent::PinsMissing => {
            "Pinterest не віддав жодного піна — перевір вхід у Pinterest".to_owned()
        }
        JournalEvent::ArtworksImported { board, count } => format!(
            "{} з ArtStation для референсу «{board}»",
            plural(*count, ["робота", "роботи", "робіт"])
        ),
        JournalEvent::ArtworksMissing => {
            "ArtStation не віддав жодної роботи — перевір інтернет".to_owned()
        }
        JournalEvent::AssetsFound { count } => format!(
            "Claude знайшов в інтернеті {} для гри",
            plural(*count, ["готовий ассет", "готові ассети", "готових ассетів"])
        ),
        JournalEvent::AssetHuntFailed { .. } => "Пошук готових ассетів не вдався".to_owned(),
        JournalEvent::ProjectFormed { name } => format!("Проєкт «{name}» сформовано"),
        JournalEvent::ProjectRenamed { name } => format!("Проєкт перейменовано на «{name}»"),
        JournalEvent::EngineChanged { engine } => format!("Рушій змінено на {}", engine.label()),
        JournalEvent::RetryQueued { count } => format!(
            "Повтор: {} знову в черзі",
            plural(*count, ["завдання", "завдання", "завдань"])
        ),
        JournalEvent::CodeStarted { fresh } => {
            if *fresh {
                "Claude почав писати код гри з нуля".to_owned()
            } else {
                "Claude почав доопрацьовувати код гри".to_owned()
            }
        }
        JournalEvent::CodeReady { files } => format!(
            "Код гри згенеровано: {}",
            plural(*files, ["файл", "файли", "файлів"])
        ),
        JournalEvent::CodeFailed { reason } => format!("Код гри не згенеровано: {reason}"),
        JournalEvent::BuildStarted { target, toolchain } => {
            format!("Білд {} запущено: {toolchain}", target.label())
        }
        JournalEvent::BuildDone { target, size_bytes } => {
            format!("Білд {} готовий · {}", target.label(), format_size(*size_bytes))
        }
        JournalEvent::BuildFailed { target, reason } => {
            format!("Білд {} упав: {reason}", target.label())
        }
        JournalEvent::BuildCancelled { target } => format!("Білд {} скасовано", target.label()),
        JournalEvent::BuildSkipped { target, reason } => {
            format!("Білд {} пропущено: {reason}", target.label())
        }
        JournalEvent::PlaytestStarted => "Плейтест: бот грає в гру й знімає екрани".to_owned(),
        JournalEvent::PlaytestDone { verdict, fixed, open } => playtest_note(*verdict, *fixed, *open),
        JournalEvent::PlaytestFailed { reason } => format!("Плейтест не вдався: {reason}"),
        JournalEvent::PlaytestSkipped { reason } => format!("Плейтест пропущено: {reason}"),
    }
}

fn playtest_note(verdict: PlaytestVerdict, fixed: u32, open: u32) -> String {
    let repaired = plural(fixed, ["проблему", "проблеми", "проблем"]);
    let left = plural(open, ["проблема", "проблеми", "проблем"]);
    match verdict {
        PlaytestVerdict::Passed => "Плейтест пройдено: гра запускається й грається".to_owned(),
        PlaytestVerdict::Fixed if open == 0 => format!("Плейтест: виправлено {repaired}"),
        PlaytestVerdict::Fixed => format!("Плейтест: виправлено {repaired}, лишилось {left}"),
        PlaytestVerdict::NeedsWork if open == 0 => "Плейтест: гра ще потребує доробки — глянь звіт".to_owned(),
        PlaytestVerdict::NeedsWork => format!("Плейтест: лишилось {left} — глянь звіт"),
    }
}

pub(crate) fn plural(count: u32, [one, few, many]: [&str; 3]) -> String {
    let word = match (count % 10, count % 100) {
        (1, rest) if rest != 11 => one,
        (2..=4, rest) if !(12..=14).contains(&rest) => few,
        _ => many,
    };
    format!("{count} {word}")
}
