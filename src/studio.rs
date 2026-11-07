use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_logger::tracing;
use noctics_shared::{
    BuildTarget, CLAUDE_TIMEOUT, GameEngine, MAX_FEEDBACK_CHARS, MAX_PROJECT_NAME_CHARS, MAX_PROMPT_CHARS,
    MIN_PROMPT_CHARS, OutputStatus, Project, ProjectStatus, ScreenStatus, StudioError, StudioSettings,
};

use crate::bridge;

const CHANGED_EVENT: &str = "studio-changed";
const ENGINE_KEY: &str = "noctics.engine";
const BUILD_PLAN_KEY: &str = "noctics.build-plan";
const BUILD_PLAN_SEPARATOR: char = ',';

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Pipeline,
    Core,
    Skills,
    ComfyUi,
    ClaudeDesign,
    Projects,
    Pinterest,
    AssetHunt,
    Mixamo,
    Models,
    Build,
    Journal,
    Settings,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Studio {
    pub section: Signal<Section>,
    pub projects: Signal<Vec<Project>>,
    pub selected: Signal<Option<i64>>,
    pub settings: Signal<Option<StudioSettings>>,
    pub revision: Signal<u64>,
    pub draft_engine: Signal<GameEngine>,
    pub build_plan: Signal<Vec<BuildTarget>>,
}

impl Studio {
    pub fn open(mut self, section: Section) {
        self.section.set(section);
    }

    pub fn open_project(mut self, project_id: i64) {
        self.selected.set(Some(project_id));
        self.section.set(Section::ClaudeDesign);
    }

    pub fn connected(self) -> bool {
        self.settings
            .read()
            .as_ref()
            .is_some_and(|settings| settings.account.connected())
    }

    pub fn changed(mut self) {
        if let Ok(mut revision) = self.revision.try_write() {
            *revision += 1;
        }
    }

    pub fn choose_draft_engine(mut self, engine: GameEngine) {
        self.draft_engine.set(engine);
        if let Some(storage) = local_storage() {
            let _ = storage.set_item(ENGINE_KEY, engine.code());
        }
    }

    pub fn plans_build(self, target: BuildTarget) -> bool {
        self.build_plan.read().contains(&target)
    }

    pub fn toggle_build_target(self, target: BuildTarget) {
        let mut plan = self.build_plan.read().clone();
        match plan.iter().position(|item| *item == target) {
            Some(at) => {
                plan.remove(at);
            }
            None => plan.push(target),
        }
        self.set_build_plan(&plan);
    }

    pub fn add_build_targets(self, targets: &[BuildTarget]) {
        let mut plan = self.build_plan.read().clone();
        plan.extend_from_slice(targets);
        self.set_build_plan(&plan);
    }

    pub fn set_build_plan(mut self, targets: &[BuildTarget]) {
        let ordered: Vec<BuildTarget> = BuildTarget::ALL
            .into_iter()
            .filter(|target| targets.contains(target))
            .collect();
        if let Some(storage) = local_storage() {
            let _ = storage.set_item(BUILD_PLAN_KEY, &encode_build_plan(&ordered));
        }
        self.build_plan.set(ordered);
    }

    pub fn focused_project(self) -> Option<Project> {
        let projects = self.projects.read();
        let chosen = (self.selected)();
        chosen
            .and_then(|id| projects.iter().find(|project| project.id == id))
            .or_else(|| projects.first())
            .cloned()
    }
}

pub fn use_studio() -> Studio {
    use_context::<Studio>()
}

pub fn provide_studio() -> Studio {
    let studio = use_context_provider(|| Studio {
        section: Signal::new(Section::Skills),
        projects: Signal::new(Vec::new()),
        selected: Signal::new(None),
        settings: Signal::new(None),
        revision: Signal::new(0),
        draft_engine: Signal::new(load_draft_engine()),
        build_plan: Signal::new(load_build_plan()),
    });
    let subscription = use_hook(|| Rc::new(RefCell::new(None::<bridge::Subscription>)));

    use_future(move || {
        let slot = Rc::clone(&subscription);
        async move {
            let listening = bridge::listen(CHANGED_EVENT, move || studio.changed()).await;
            *slot.borrow_mut() = listening;
        }
    });

    let mut projects = studio.projects;
    use_resource(move || async move {
        let _ = (studio.revision)();
        if let Ok(list) = bridge::list_projects().await {
            projects.set(list);
        }
    });

    let mut settings = studio.settings;
    use_future(move || async move {
        if let Ok(loaded) = bridge::studio_settings().await {
            settings.set(Some(loaded));
        }
    });

    studio
}

pub fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn load_draft_engine() -> GameEngine {
    local_storage()
        .and_then(|storage| storage.get_item(ENGINE_KEY).ok().flatten())
        .and_then(|code| GameEngine::from_code(&code))
        .unwrap_or_default()
}

fn load_build_plan() -> Vec<BuildTarget> {
    let stored = local_storage()
        .and_then(|storage| storage.get_item(BUILD_PLAN_KEY).ok().flatten())
        .unwrap_or_default();
    BuildTarget::ALL
        .into_iter()
        .filter(|target| stored.split(BUILD_PLAN_SEPARATOR).any(|code| code == target.code()))
        .collect()
}

fn encode_build_plan(targets: &[BuildTarget]) -> String {
    targets
        .iter()
        .map(|target| target.code())
        .collect::<Vec<_>>()
        .join(&BUILD_PLAN_SEPARATOR.to_string())
}

pub fn project_status_label(status: ProjectStatus) -> &'static str {
    match status {
        ProjectStatus::Queued => "У черзі",
        ProjectStatus::Planning => "Claude планує",
        ProjectStatus::Designing => "У Claude Design",
        ProjectStatus::Ready => "Сформовано",
        ProjectStatus::Failed => "Помилка",
    }
}

pub fn screen_status_label(status: ScreenStatus) -> &'static str {
    match status {
        ScreenStatus::Queued => "У черзі",
        ScreenStatus::Generating => "Генерується",
        ScreenStatus::Draft => "Чернетка",
        ScreenStatus::Approved => "Синхронізовано",
        ScreenStatus::Failed => "Помилка",
    }
}

pub fn screen_status_tone(status: ScreenStatus) -> &'static str {
    match status {
        ScreenStatus::Queued => "is-queued",
        ScreenStatus::Generating => "is-generating",
        ScreenStatus::Draft => "is-draft",
        ScreenStatus::Approved => "is-approved",
        ScreenStatus::Failed => "is-failed",
    }
}

pub fn output_status_label(status: OutputStatus) -> &'static str {
    match status {
        OutputStatus::Queued => "У черзі",
        OutputStatus::Generating => "Працює",
        OutputStatus::Done => "Готово",
        OutputStatus::Failed => "Помилка",
    }
}

pub fn output_status_tone(status: OutputStatus) -> &'static str {
    match status {
        OutputStatus::Queued => "is-queued",
        OutputStatus::Generating => "is-generating",
        OutputStatus::Done => "is-approved",
        OutputStatus::Failed => "is-failed",
    }
}

pub fn explain(problem: &StudioError) -> String {
    match problem {
        StudioError::SignedOut => "Сесія завершилась — увійди знову.".to_owned(),
        StudioError::ClaudeCodeMissing => {
            "Не знайдено Claude Code. Встанови його й увійди в підписку Claude — деталі в Налаштуваннях.".to_owned()
        }
        StudioError::ClaudeSignedOut => {
            "Claude Code не підключено до підписки. Увійди в Налаштуваннях.".to_owned()
        }
        StudioError::UsageLimit => {
            "Ліміт підписки Claude на зараз вичерпано. Спробуй, коли він оновиться.".to_owned()
        }
        StudioError::NotFound => "Цього проєкту чи екрана вже немає.".to_owned(),
        StudioError::PromptTooShort => {
            format!("Опиши гру детальніше — хоча б {MIN_PROMPT_CHARS} символів.")
        }
        StudioError::PromptTooLong => format!("Промпт задовгий — максимум {MAX_PROMPT_CHARS} символів."),
        StudioError::NameMissing => "Назва не може бути порожньою.".to_owned(),
        StudioError::NameTooLong => format!("Назва задовга — максимум {MAX_PROJECT_NAME_CHARS} символів."),
        StudioError::SummaryTooLong => "Опис екрана задовгий.".to_owned(),
        StudioError::FeedbackMissing => "Напиши, що змінити на екрані.".to_owned(),
        StudioError::FeedbackTooLong => format!("Правки задовгі — максимум {MAX_FEEDBACK_CHARS} символів."),
        StudioError::NotApproved => "Обкладинкою може бути лише апрувнутий екран.".to_owned(),
        StudioError::StillPlanning => "Claude ще планує гру — зачекай кілька секунд.".to_owned(),
        StudioError::ScreenBusy => "Екран ще генерується.".to_owned(),
        StudioError::SkillUnavailable => "Цю навичку вимкнули або видалили — увімкни її в SKILLS і натисни «Синхронізувати».".to_owned(),
        StudioError::Refused => "Claude відмовився виконати цей запит. Переформулюй промпт.".to_owned(),
        StudioError::Truncated => "Відповідь Claude обірвалась. Спробуй перегенерувати.".to_owned(),
        StudioError::RateLimited => "Забагато запитів до Claude. Спробуй трохи згодом.".to_owned(),
        StudioError::Overloaded => "Claude зараз перевантажений. Спробуй трохи згодом.".to_owned(),
        StudioError::Process(detail) if detail == CLAUDE_TIMEOUT => {
            "Claude не встиг відповісти за 20 хвилин — завдання завелике для однієї відповіді.".to_owned()
        }
        StudioError::Process(detail) => {
            tracing::warn!("claude code process failed: {detail}");
            "Claude Code не відповів. Перевір інтернет і спробуй ще раз.".to_owned()
        }
        StudioError::Claude(detail) => format!("Claude: {detail}"),
        StudioError::Storage(detail) => {
            tracing::error!("studio storage failed: {detail}");
            "Не вдалося звернутися до сховища студії.".to_owned()
        }
    }
}
