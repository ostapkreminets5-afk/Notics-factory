use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use dioxus::prelude::*;
use noctics_shared::{
    Build, BuildOverview, BuildStatus, BuildTarget, DESIGN_STAGE, GameSource, INTERFACE_STAGE,
    LAST_STAGE, OutputStatus, Project, ProjectDetail, ProjectStatus, Screen, ScreenStatus,
    SkillOutput, SourceStatus, format_size,
};

use crate::journal::plural;
use crate::studio::{Section, Studio, explain, use_studio};
use crate::{bridge, build_bridge};

const BUILDS_CHANGED: &str = "builds-changed";
const TICK_MILLIS: i32 = 1000;
const MILLIS_PER_SECOND: f64 = 1000.0;
const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_HOUR: i64 = 3600;
const FULL_PERCENT: usize = 100;
const IDLE_CLOCK: &str = "--:--";

struct StageInfo {
    number: u8,
    title: &'static str,
    note: &'static str,
    empty: &'static str,
}

const STAGES: [StageInfo; LAST_STAGE as usize] = [
    StageInfo {
        number: 1,
        title: "Промпт",
        note: "План, дизайн-документ, лор",
        empty: "Чекає на промпт гри",
    },
    StageInfo {
        number: 2,
        title: "Концепт",
        note: "Референси Pinterest, екрани Claude Design",
        empty: "Немає навички — додай свою",
    },
    StageInfo {
        number: 3,
        title: "Модель",
        note: "Специфікації 3D-моделей",
        empty: "Немає навички — додай свою",
    },
    StageInfo {
        number: 4,
        title: "Матеріал",
        note: "Стиль матеріалів і текстур",
        empty: "Немає навички — додай свою",
    },
    StageInfo {
        number: 5,
        title: "Анімація",
        note: "Анімація об’єктів",
        empty: "Немає навички — додай свою",
    },
    StageInfo {
        number: LAST_STAGE,
        title: "Збірка",
        note: "Код, ассети ComfyUI і білди",
        empty: "Код і ассети стартують самі, коли екрани й навички готові",
    },
];

#[derive(Clone, PartialEq)]
pub struct Recent {
    pub clock: String,
    pub source: String,
    pub text: String,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Phase {
    Idle,
    Done,
    Review,
    Queued,
    Failed,
    Working,
}

impl Phase {
    fn code(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Done => "done",
            Self::Review => "review",
            Self::Queued => "queued",
            Self::Failed => "failed",
            Self::Working => "working",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Idle => "Очікує",
            Self::Done => "Готово",
            Self::Review => "Чекає на тебе",
            Self::Queued => "У черзі",
            Self::Failed => "Помилка",
            Self::Working => "Працює",
        }
    }

    fn is_active(self) -> bool {
        self >= Self::Review
    }
}

#[derive(Clone, PartialEq)]
struct Task {
    phase: Phase,
    text: String,
    detail: Option<String>,
    since: Option<i64>,
    ordinal: Option<(usize, usize)>,
    call: Option<String>,
    action: Option<String>,
}

impl Task {
    fn new(phase: Phase, text: String) -> Self {
        Self {
            phase,
            text,
            detail: None,
            since: None,
            ordinal: None,
            call: None,
            action: None,
        }
    }

    fn with_detail(mut self, detail: Option<String>) -> Self {
        self.detail = detail.filter(|text| !text.trim().is_empty());
        self
    }

    fn since(mut self, moment: Option<i64>) -> Self {
        self.since = moment;
        self
    }

    fn ordinal(mut self, index: usize, total: usize) -> Self {
        self.ordinal = Some((index + 1, total));
        self
    }

    fn agent(mut self, call: Option<String>, action: &str) -> Self {
        self.call = call.filter(|text| !text.trim().is_empty());
        self.action = Some(action.to_owned());
        self
    }

    fn headline(&self) -> String {
        match self.ordinal {
            Some((index, total)) if total > 1 => format!("{} — {index} з {total}", self.text),
            _ => self.text.clone(),
        }
    }
}

#[derive(Clone, PartialEq)]
struct Stage {
    number: u8,
    title: &'static str,
    note: &'static str,
    empty: &'static str,
    tasks: Vec<Task>,
}

impl Stage {
    fn phase(&self) -> Phase {
        self.tasks
            .iter()
            .map(|task| task.phase)
            .max()
            .unwrap_or(Phase::Idle)
    }

    fn lead(&self) -> Option<&Task> {
        let phase = self.phase();
        self.tasks.iter().find(|task| task.phase == phase)
    }

    fn count(&self, phase: Phase) -> usize {
        self.tasks.iter().filter(|task| task.phase == phase).count()
    }

    fn summary(&self) -> String {
        let phase = self.phase();
        let lead = self.lead().map(Task::headline).unwrap_or_default();
        let count = self.count(phase);
        match phase {
            Phase::Idle => self.empty.to_owned(),
            Phase::Done => "Усе готово".to_owned(),
            Phase::Working | Phase::Failed => lead,
            Phase::Queued => format!(
                "{} у черзі",
                plural(count as u32, ["завдання", "завдання", "завдань"])
            ),
            Phase::Review if count > 1 => format!(
                "Апруву чекають {}",
                plural(count as u32, ["екран", "екрани", "екранів"])
            ),
            Phase::Review => lead,
        }
    }

    fn caption(&self) -> &'static str {
        if self.tasks.is_empty() { self.empty } else { self.note }
    }

    fn progress(&self) -> (usize, usize) {
        (self.count(Phase::Done), self.tasks.len())
    }

    fn roster(&self) -> String {
        if self.tasks.is_empty() {
            return self.empty.to_owned();
        }
        self.tasks
            .iter()
            .map(|task| format!("{} — {}", task.phase.label(), task.text))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[component]
pub fn StageTrack(project_id: i64, starts: Vec<(u8, i64)>, recent: Option<Recent>) -> Element {
    let studio = use_studio();
    let mut builds_revision = use_signal(|| 0_u64);
    let mut detail = use_signal(|| None::<ProjectDetail>);
    let mut overview = use_signal(|| None::<BuildOverview>);
    let subscription = use_hook(|| Rc::new(RefCell::new(None::<bridge::Subscription>)));

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
        if let Ok(fresh) = bridge::project_detail(project_id).await {
            detail.set(Some(fresh));
        }
    });

    use_resource(move || async move {
        let _ = builds_revision();
        let _ = (studio.revision)();
        if let Ok(fresh) = build_bridge::overview(project_id, false).await {
            overview.set(Some(fresh));
        }
    });

    let Some(loaded) = detail.read().clone() else {
        return rsx! {};
    };
    let stages = gather(&loaded, overview.read().as_ref(), &starts);
    let current = stages
        .iter()
        .filter(|stage| stage.phase().is_active())
        .max_by(|a, b| a.phase().cmp(&b.phase()).then(b.number.cmp(&a.number)))
        .map(|stage| stage.number);

    rsx! {
        section { class: "stage-board", aria_label: "Етапи проєкту",
            NowPanel { stages: stages.clone(), current, recent }
            ol { class: "stage-track",
                for stage in stages {
                    StageCard {
                        key: "{stage.number}",
                        stage: stage.clone(),
                        current: current == Some(stage.number),
                        project_id,
                    }
                }
            }
        }
    }
}

#[component]
fn NowPanel(stages: Vec<Stage>, current: Option<u8>, recent: Option<Recent>) -> Element {
    let active = current.and_then(|number| stages.iter().find(|stage| stage.number == number));
    let lead = active.and_then(Stage::lead).cloned();
    let phase = active.map_or(Phase::Idle, Stage::phase);

    let (heading, summary) = match active {
        Some(stage) => (
            format!("{:02} {}", stage.number, stage.title),
            stage.summary(),
        ),
        None => {
            let finished = stages
                .iter()
                .filter(|stage| stage.phase() == Phase::Done)
                .map(|stage| stage.number)
                .max();
            match finished {
                Some(LAST_STAGE) => (
                    "Конвеєр пройдено".to_owned(),
                    "Код гри й білди готові — можна збирати ще платформи".to_owned(),
                ),
                Some(number) => (
                    "Пауза".to_owned(),
                    format!("Останній завершений етап — {number:02} {}", title_of(number)),
                ),
                None => (
                    "Пауза".to_owned(),
                    "Ядро чекає, доки конвеєр візьметься до проєкту".to_owned(),
                ),
            }
        }
    };

    let working = phase == Phase::Working;
    let (call, action) = match lead.as_ref().filter(|_| working) {
        Some(task) => (
            task.call.clone().unwrap_or_else(|| task.text.clone()),
            task.action.clone().unwrap_or_default(),
        ),
        None => match &recent {
            Some(recent) => (format!("{} · {}", recent.clock, recent.source), recent.text.clone()),
            None => ("—".to_owned(), "Агент ще нічого не робив у цьому проєкті".to_owned()),
        },
    };
    let since = lead.as_ref().and_then(|task| task.since).filter(|_| working);
    let failure = lead
        .as_ref()
        .filter(|_| phase == Phase::Failed)
        .and_then(|task| task.detail.clone());

    rsx! {
        div { class: "stage-now is-{phase.code()}", role: "status", aria_live: "polite",
            div { class: "stage-now-cell is-current",
                p { class: "stage-now-label",
                    span { "Зараз" }
                    if active.is_some() {
                        span { class: "stage-pill is-{phase.code()}",
                            span { class: "stage-pill-dot" }
                            if working { "Наживо" } else { "{phase.label()}" }
                        }
                    }
                }
                p { class: "stage-now-title", "{heading}" }
                p { class: "stage-now-summary", "{summary}" }
                if let Some(reason) = failure {
                    p { class: "stage-now-failure", "{reason}" }
                }
            }
            div { class: "stage-now-cell is-agent",
                p { class: "stage-now-label",
                    span { if working { "Остання дія агента" } else { "Остання подія" } }
                }
                p { class: "stage-now-call", title: "{call}", "{call}" }
                p { class: "stage-now-action", "{action}" }
            }
            div { class: "stage-now-cell is-clock",
                if let Some(since) = since {
                    Elapsed { since }
                } else {
                    p { class: "stage-clock is-idle", "{IDLE_CLOCK}" }
                    p { class: "stage-clock-note", "немає активної задачі" }
                }
            }
        }
    }
}

#[component]
fn Elapsed(since: i64) -> Element {
    let mut now = use_signal(now_seconds);

    use_future(move || async move {
        loop {
            pause(TICK_MILLIS).await;
            now.set(now_seconds());
        }
    });

    rsx! {
        p { class: "stage-clock", "{stopwatch(now() - since)}" }
        p { class: "stage-clock-note", "від старту задачі" }
    }
}

#[component]
fn StageCard(stage: Stage, current: bool, project_id: i64) -> Element {
    let studio = use_studio();
    let phase = stage.phase();
    let (done, total) = stage.progress();
    let percent = if total == 0 { 0 } else { done * FULL_PERCENT / total };
    let roster = stage.roster();
    let number = stage.number;
    let class = if current {
        format!("stage-card is-{} is-current", phase.code())
    } else {
        format!("stage-card is-{}", phase.code())
    };

    rsx! {
        li {
            button {
                class: "{class}",
                r#type: "button",
                title: "{roster}",
                aria_current: if current { "step" } else { "false" },
                onclick: move |_| open_stage(studio, number, project_id),
                span { class: "stage-card-head",
                    span { class: "stage-number", "{stage.number:02}" }
                    span { class: "stage-status",
                        span { class: "stage-status-dot" }
                        "{phase.label()}"
                    }
                }
                span { class: "stage-title", "{stage.title}" }
                span { class: "stage-note", "{stage.caption()}" }
                span { class: "stage-progress",
                    span { class: "stage-progress-bar",
                        span { class: "stage-progress-fill", style: "width: {percent}%" }
                    }
                    span { class: "stage-count", "{done}/{total}" }
                }
            }
        }
    }
}

fn open_stage(studio: Studio, number: u8, project_id: i64) {
    if number == LAST_STAGE {
        studio.open(Section::Build);
    } else {
        studio.open_project(project_id);
    }
}

fn gather(detail: &ProjectDetail, overview: Option<&BuildOverview>, starts: &[(u8, i64)]) -> Vec<Stage> {
    let started = |number: u8| {
        starts
            .iter()
            .filter(|(stage, _)| *stage == number)
            .map(|(_, moment)| *moment)
            .max()
    };
    STAGES
        .iter()
        .map(|info| {
            let since = started(info.number);
            let mut tasks = Vec::new();
            if info.number == 1 {
                tasks.push(plan_task(&detail.project));
            }
            let outputs: Vec<&SkillOutput> = detail
                .outputs
                .iter()
                .filter(|output| output.stage == info.number)
                .collect();
            tasks.extend(
                outputs
                    .iter()
                    .enumerate()
                    .map(|(index, output)| output_task(output, since).ordinal(index, outputs.len())),
            );
            if info.number == INTERFACE_STAGE {
                tasks.extend(
                    detail
                        .screens
                        .iter()
                        .enumerate()
                        .map(|(index, screen)| screen_task(screen, since).ordinal(index, detail.screens.len())),
                );
            }
            if info.number == LAST_STAGE
                && let Some(overview) = overview
            {
                tasks.extend(overview.source.as_ref().map(|source| source_task(source, since)));
                let builds = latest_builds(&overview.builds);
                let total = builds.len();
                tasks.extend(
                    builds
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, build)| build_task(build).map(|task| task.ordinal(index, total))),
                );
            }
            Stage {
                number: info.number,
                title: info.title,
                note: info.note,
                empty: info.empty,
                tasks,
            }
        })
        .collect()
}

fn plan_task(project: &Project) -> Task {
    match project.status {
        ProjectStatus::Queued => Task::new(Phase::Queued, "План гри в черзі".to_owned()),
        ProjectStatus::Planning => Task::new(Phase::Working, "Claude планує гру".to_owned())
            .since(Some(project.created_at))
            .agent(
                Some(claude_call("high", "json-schema plan")),
                "складає назву, стиль, дизайн-документ і список екранів",
            ),
        ProjectStatus::Failed => Task::new(Phase::Failed, "Планування гри не вдалося".to_owned())
            .with_detail(project.error.as_ref().map(explain)),
        ProjectStatus::Designing | ProjectStatus::Ready => {
            Task::new(Phase::Done, format!("План «{}» готовий", project.name))
        }
    }
}

fn output_task(output: &SkillOutput, since: Option<i64>) -> Task {
    let title = &output.title;
    match output.status {
        OutputStatus::Queued => Task::new(Phase::Queued, format!("«{title}» у черзі")),
        OutputStatus::Generating => {
            let effort = if output.stage <= DESIGN_STAGE { "high" } else { "medium" };
            Task::new(Phase::Working, format!("Навичка «{title}» пише документ"))
                .since(since)
                .agent(
                    Some(claude_call(effort, &format!("skill {}", output.skill_name))),
                    "пише документ навички за брифом проєкту",
                )
        }
        OutputStatus::Done => Task::new(Phase::Done, format!("«{title}» готово")),
        OutputStatus::Failed => Task::new(Phase::Failed, format!("Навичка «{title}» не відпрацювала"))
            .with_detail(output.error.as_ref().map(explain)),
    }
}

fn screen_task(screen: &Screen, since: Option<i64>) -> Task {
    let title = &screen.title;
    match screen.status {
        ScreenStatus::Queued => Task::new(Phase::Queued, format!("Екран «{title}» у черзі")),
        ScreenStatus::Generating if screen.revision > 0 => {
            Task::new(Phase::Working, format!("Переробляється екран «{title}»"))
                .since(since)
                .agent(
                    Some(claude_call("medium", "screen")),
                    "переробляє екран за твоїми правками",
                )
        }
        ScreenStatus::Generating => Task::new(Phase::Working, format!("Генерується екран «{title}»"))
            .since(since)
            .agent(
                Some(claude_call("high", "screen")),
                "малює екран 1280×720 у палітрі й шрифтах проєкту",
            ),
        ScreenStatus::Draft => Task::new(Phase::Review, format!("Екран «{title}» чекає на апрув")),
        ScreenStatus::Approved => Task::new(Phase::Done, format!("Екран «{title}» апрувнуто")),
        ScreenStatus::Failed => Task::new(Phase::Failed, format!("Екран «{title}» не згенеровано"))
            .with_detail(screen.error.as_ref().map(explain)),
    }
}

fn source_task(source: &GameSource, since: Option<i64>) -> Task {
    match source.status {
        SourceStatus::Generating => Task::new(Phase::Working, "Claude пише код гри".to_owned())
            .since(since)
            .agent(source.activity.last().cloned(), "Claude-агент пише й перевіряє код гри"),
        SourceStatus::Ready if source.valid => Task::new(Phase::Done, "Код гри готовий".to_owned()),
        SourceStatus::Ready => Task::new(Phase::Failed, "Папка коду гри не схожа на проєкт рушія".to_owned())
            .with_detail(Some(source.path.clone())),
        SourceStatus::Failed => Task::new(Phase::Failed, "Генерація коду гри не вдалася".to_owned())
            .with_detail(source.error.clone()),
    }
}

fn latest_builds(builds: &[Build]) -> Vec<&Build> {
    let mut latest: BTreeMap<usize, &Build> = BTreeMap::new();
    for build in builds.iter().filter(|build| build.status != BuildStatus::Cancelled) {
        let slot = target_order(build.target);
        if latest.get(&slot).is_none_or(|kept| kept.id < build.id) {
            latest.insert(slot, build);
        }
    }
    latest.into_values().collect()
}

fn target_order(target: BuildTarget) -> usize {
    BuildTarget::ALL
        .iter()
        .position(|candidate| *candidate == target)
        .unwrap_or(BuildTarget::ALL.len())
}

fn build_task(build: &Build) -> Option<Task> {
    let target = build.target.label();
    let task = match build.status {
        BuildStatus::Queued => Task::new(Phase::Queued, format!("Білд {target} у черзі")),
        BuildStatus::Running => Task::new(Phase::Working, format!("Збирається {target}"))
            .since(build.started_at)
            .agent(last_line(&build.log_tail), "збирає білд і пише лог збірки"),
        BuildStatus::Done => Task::new(Phase::Done, format!("Білд {target} готовий"))
            .with_detail(build.size_bytes.map(format_size)),
        BuildStatus::Failed => {
            Task::new(Phase::Failed, format!("Білд {target} упав")).with_detail(build.error.clone())
        }
        BuildStatus::Cancelled => return None,
    };
    Some(task)
}

fn claude_call(effort: &str, what: &str) -> String {
    format!("claude -p · effort {effort} · {what}")
}

fn last_line(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

fn title_of(number: u8) -> &'static str {
    STAGES
        .iter()
        .find(|stage| stage.number == number)
        .map_or("", |stage| stage.title)
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
