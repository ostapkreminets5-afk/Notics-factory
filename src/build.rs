use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_logger::tracing;
use noctics_shared::{
    Build, BuildError, BuildFamily, BuildOverview, BuildStatus, BuildTarget, GameEngine, GameSource,
    MAX_BUILD_REQUEST_CHARS, MAX_SOURCE_PATH_CHARS, Project, Readiness, SourceOrigin, SourceStatus,
    format_duration, format_size, project_marker,
};
use wasm_bindgen::JsValue;

use crate::bridge;
use crate::build_bridge::{self, BUILDS_FOLDER, SOURCE_FOLDER};
use crate::icons::{ArrowRightIcon, ExternalIcon};
use crate::studio::{Section, explain, use_studio};

const CHANGED_EVENT: &str = "builds-changed";
const VISIBLE_ACTIVITY: usize = 12;
const VISIBLE_LOG_LINES: usize = 14;
const MILLIS_PER_SECOND: f64 = 1000.0;
const PAGE_LABEL: &str = "Система · Білди гри";
const PAGE_TITLE: &str = "Збірка";

#[derive(Clone, Copy, PartialEq)]
struct Board {
    project_id: i64,
    overview: Signal<Option<BuildOverview>>,
    notice: Signal<Option<String>>,
}

impl Board {
    fn settle(self, result: Result<BuildOverview, BuildError>) {
        let mut overview = self.overview;
        let mut notice = self.notice;
        match result {
            Ok(fresh) => {
                overview.set(Some(fresh));
                notice.set(None);
            }
            Err(problem) => notice.set(Some(explain_build(&problem))),
        }
    }

    fn report(self, result: Result<(), BuildError>) {
        let mut notice = self.notice;
        notice.set(result.err().map(|problem| explain_build(&problem)));
    }

    fn engine(self) -> Option<GameEngine> {
        self.overview.read().as_ref().map(|overview| overview.engine)
    }
}

enum SourceView {
    Empty,
    Generating(GameSource),
    Failed(GameSource),
    Broken(GameSource),
    Ready(GameSource),
}

impl SourceView {
    fn of(source: Option<GameSource>) -> Self {
        match source {
            None => Self::Empty,
            Some(source) if source.status == SourceStatus::Generating => Self::Generating(source),
            Some(source) if source.status == SourceStatus::Failed => Self::Failed(source),
            Some(source) if !source.valid => Self::Broken(source),
            Some(source) => Self::Ready(source),
        }
    }
}

#[component]
pub fn BuildPage() -> Element {
    let studio = use_studio();
    match studio.focused_project() {
        Some(project) => rsx! {
            BuildBoard { key: "{project.id}", project }
        },
        None => rsx! {
            section { class: "page build-page",
                header { class: "page-head",
                    p { class: "mono-label", "{PAGE_LABEL}" }
                    h1 { class: "page-title", "{PAGE_TITLE}" }
                    p { class: "page-lead", "Фінальні білди гри для ПК, вебу й телефонів." }
                }
                div { class: "build-stack",
                    PlanCard {}
                    div { class: "page-empty",
                        p { class: "mono-label", "Ще немає ігор" }
                        p {
                            "Запусти конвеєр гри — і тут з’являться її код та білди під вибрані платформи."
                        }
                        button {
                            class: "primary-button is-start",
                            r#type: "button",
                            onclick: move |_| studio.open(Section::Pipeline),
                            "Конвеєр гри"
                            ArrowRightIcon {}
                        }
                    }
                }
            }
        },
    }
}

#[component]
fn BuildBoard(project: Project) -> Element {
    let studio = use_studio();
    let project_id = project.id;
    let mut revision = use_signal(|| 0_u64);
    let mut refreshing = use_signal(|| false);
    let board = use_context_provider(|| Board {
        project_id,
        overview: Signal::new(None),
        notice: Signal::new(None),
    });
    let subscription = use_hook(|| Rc::new(RefCell::new(None::<bridge::Subscription>)));

    use_future(move || {
        let slot = Rc::clone(&subscription);
        async move {
            let listening = bridge::listen(CHANGED_EVENT, move || {
                if let Ok(mut value) = revision.try_write() {
                    *value += 1;
                }
            })
            .await;
            *slot.borrow_mut() = listening;
        }
    });

    use_resource(move || async move {
        let _ = revision();
        let _ = (studio.revision)();
        match build_bridge::overview(project_id, false).await {
            Ok(fresh) => {
                let mut overview = board.overview;
                overview.set(Some(fresh));
            }
            Err(problem) => board.report(Err(problem)),
        }
    });

    let refresh = move |_| {
        refreshing.set(true);
        spawn(async move {
            board.settle(build_bridge::overview(project_id, true).await);
            refreshing.set(false);
        });
    };

    let open_builds = move |_| {
        spawn(async move {
            board.report(build_bridge::open_folder(project_id, BUILDS_FOLDER).await);
        });
    };

    let engine = board.engine().unwrap_or(project.engine);
    let loaded = board.overview.read().is_some();

    rsx! {
        section { class: "page build-page",
            header { class: "page-head has-action",
                div { class: "page-head-text",
                    p { class: "mono-label", "{PAGE_LABEL}" }
                    h1 { class: "page-title", "{PAGE_TITLE}" }
                    p { class: "page-lead",
                        "{project.name} · {engine.label()} — фінальні білди для ПК, вебу й телефонів."
                    }
                }
                div { class: "head-actions",
                    button {
                        class: "ghost-button",
                        r#type: "button",
                        disabled: refreshing(),
                        title: "Ще раз знайти рушії, SDK і JDK на цьому комп’ютері",
                        onclick: refresh,
                        if refreshing() {
                            span { class: "spinner" }
                        }
                        "Перевірити інструменти"
                    }
                    button {
                        class: "ghost-button",
                        r#type: "button",
                        onclick: open_builds,
                        "Папка збірок"
                        ExternalIcon {}
                    }
                }
            }
            if let Some(message) = (board.notice)() {
                div { class: "page-alert", role: "alert", span { "{message}" } }
            }
            if loaded {
                div { class: "build-stack",
                    SourceCard { project: project.clone() }
                    TargetsCard {}
                    HistoryCard {}
                }
            } else {
                div { class: "build-loading",
                    span { class: "spinner" }
                    "Шукаємо рушії та інструменти збірки на цьому комп’ютері…"
                }
            }
        }
    }
}

#[component]
fn CardHead(step: &'static str, title: &'static str, note: String) -> Element {
    rsx! {
        div { class: "build-card-head",
            span { class: "build-step", "{step}" }
            div { class: "build-card-text",
                h2 { class: "build-card-title", "{title}" }
                p { class: "build-card-note", "{note}" }
            }
        }
    }
}

#[component]
fn SourceCard(project: Project) -> Element {
    let board = use_context::<Board>();
    let studio = use_studio();
    let mut request = use_signal(String::new);
    let mut sending = use_signal(|| false);
    let Some(overview) = (board.overview)() else {
        return rsx! {};
    };
    let engine = overview.engine;
    let connected = studio.connected();
    let planning = project.status.is_planning();
    let can_ask = connected && !planning && !sending();

    let mut generate = move |text: Option<String>| {
        sending.set(true);
        spawn(async move {
            let result = build_bridge::generate(board.project_id, text.as_deref()).await;
            if result.is_ok() {
                request.set(String::new());
            }
            board.settle(result);
            sending.set(false);
        });
    };

    let stop = move |_| {
        spawn(async move {
            board.report(build_bridge::stop_generation(board.project_id).await);
        });
    };

    let open_source = move |_| {
        spawn(async move {
            board.report(build_bridge::open_folder(board.project_id, SOURCE_FOLDER).await);
        });
    };

    let blocker = if !connected {
        Some("Спершу підключи Claude у Налаштуваннях.")
    } else if planning {
        Some("Claude ще планує гру — зачекай на дизайн-документ.")
    } else {
        None
    };

    let view = SourceView::of(overview.source.clone());
    let note = match &view {
        SourceView::Empty => format!(
            "Claude напише повний проєкт {} за дизайн-документом, навичками й затвердженими екранами — або підключи папку з готовим проєктом.",
            engine.label()
        ),
        SourceView::Generating(_) => "Claude пише гру — це триває 10–40 хвилин. Можна працювати далі, прогрес збережеться тут.".to_owned(),
        SourceView::Failed(_) => "Остання генерація не вдалася.".to_owned(),
        SourceView::Broken(_) => format!("Папка не схожа на проєкт {}.", engine.label()),
        SourceView::Ready(source) => match source.origin {
            SourceOrigin::Generated => format!("Проєкт {} написав Claude. Його можна доопрацювати й зібрати.", engine.label()),
            SourceOrigin::Linked => format!("Підключено твій проєкт {}.", engine.label()),
        },
    };

    rsx! {
        section { class: "build-card",
            CardHead { step: "01", title: "Код гри", note }
            match view {
                SourceView::Empty => rsx! {
                    div { class: "build-actions",
                        button {
                            class: "primary-button",
                            r#type: "button",
                            disabled: !can_ask,
                            onclick: move |_| generate(None),
                            if sending() {
                                span { class: "spinner" }
                                "Запускаємо…"
                            } else {
                                "Згенерувати код гри"
                                ArrowRightIcon {}
                            }
                        }
                        if let Some(reason) = blocker {
                            BlockerHint { reason }
                        }
                    }
                    LinkForm { engine }
                },
                SourceView::Generating(source) => rsx! {
                    div { class: "build-progress",
                        span { class: "spinner" }
                        p { class: "build-progress-title", "Claude пише гру на {engine.label()}…" }
                        button {
                            class: "ghost-button is-small",
                            r#type: "button",
                            onclick: stop,
                            "Зупинити"
                        }
                    }
                    ActivityFeed { lines: source.activity.clone() }
                },
                SourceView::Failed(source) => rsx! {
                    div { class: "build-alert", role: "alert",
                        "{source.error.clone().unwrap_or_default()}"
                    }
                    ActivityFeed { lines: source.activity.clone() }
                    div { class: "build-actions",
                        button {
                            class: "primary-button",
                            r#type: "button",
                            disabled: !can_ask,
                            onclick: move |_| generate(None),
                            "Спробувати ще раз"
                        }
                        button {
                            class: "ghost-button",
                            r#type: "button",
                            onclick: open_source,
                            "Відкрити папку"
                            ExternalIcon {}
                        }
                        if let Some(reason) = blocker {
                            BlockerHint { reason }
                        }
                    }
                    if source.valid {
                        RefineForm { request, can_ask, sending: sending(), on_send: move |text| generate(Some(text)) }
                    }
                },
                SourceView::Broken(source) => rsx! {
                    div { class: "build-alert", role: "alert",
                        "У папці немає {project_marker(engine)} — це не проєкт {engine.label()}."
                        if source.engine != engine {
                            " Код писали під {source.engine.label()}, а рушій проєкту зараз — {engine.label()}."
                        }
                    }
                    SourcePath { source: source.clone(), on_open: open_source }
                    div { class: "build-actions",
                        button {
                            class: "primary-button",
                            r#type: "button",
                            disabled: !can_ask,
                            onclick: move |_| generate(None),
                            "Згенерувати код гри"
                            ArrowRightIcon {}
                        }
                        if let Some(reason) = blocker {
                            BlockerHint { reason }
                        }
                    }
                    LinkForm { engine }
                },
                SourceView::Ready(source) => rsx! {
                    SourcePath { source: source.clone(), on_open: open_source }
                    RefineForm { request, can_ask, sending: sending(), on_send: move |text| generate(Some(text)) }
                    if let Some(reason) = blocker {
                        BlockerHint { reason }
                    }
                    details { class: "build-more",
                        summary { "Підключити іншу папку" }
                        LinkForm { engine }
                    }
                },
            }
        }
    }
}

#[component]
fn BlockerHint(reason: &'static str) -> Element {
    let studio = use_studio();
    rsx! {
        span { class: "build-hint",
            "{reason}"
            if !studio.connected() {
                button {
                    class: "build-hint-link",
                    r#type: "button",
                    onclick: move |_| studio.open(Section::Settings),
                    "Налаштування"
                }
            }
        }
    }
}

#[component]
fn SourcePath(source: GameSource, on_open: EventHandler<MouseEvent>) -> Element {
    let origin = match source.origin {
        SourceOrigin::Generated => "Згенеровано Claude",
        SourceOrigin::Linked => "Твоя папка",
    };
    rsx! {
        div { class: "build-source",
            div { class: "build-source-text",
                span { class: "mono-label", "{origin}" }
                code { class: "build-path", "{source.path}" }
            }
            button {
                class: "ghost-button is-small",
                r#type: "button",
                onclick: move |event| on_open.call(event),
                "Відкрити папку"
                ExternalIcon {}
            }
        }
    }
}

#[component]
fn ActivityFeed(lines: Vec<String>) -> Element {
    let start = lines.len().saturating_sub(VISIBLE_ACTIVITY);
    let recent: Vec<(usize, String)> = lines.into_iter().enumerate().skip(start).collect();
    if recent.is_empty() {
        return rsx! {};
    }
    rsx! {
        ol { class: "build-activity", aria_live: "polite",
            for (index, line) in recent {
                li { key: "{index}", "{line}" }
            }
        }
    }
}

#[component]
fn RefineForm(
    request: Signal<String>,
    can_ask: bool,
    sending: bool,
    on_send: EventHandler<String>,
) -> Element {
    let mut request = request;
    let length = request.read().chars().count();
    let empty = request.read().trim().is_empty();
    let send = move || {
        let text = request.read().trim().to_owned();
        if !text.is_empty() && can_ask {
            on_send.call(text);
        }
    };
    rsx! {
        form {
            class: "build-refine",
            novalidate: true,
            onsubmit: move |event| {
                event.prevent_default();
                send();
            },
            div { class: "field-head",
                label { class: "mono-label", r#for: "build-request", "Доопрацювати з Claude" }
                span { class: "mono-label", "{length} / {MAX_BUILD_REQUEST_CHARS}" }
            }
            textarea {
                class: "input is-area",
                id: "build-request",
                rows: "2",
                maxlength: "{MAX_BUILD_REQUEST_CHARS}",
                placeholder: "Що змінити: напр. додай другий рівень або зроби сенсорне керування зручнішим",
                value: "{request}",
                oninput: move |event| request.set(event.value()),
                onkeydown: move |event| {
                    if event.key() == Key::Enter && event.modifiers().ctrl() {
                        event.prevent_default();
                        send();
                    }
                },
            }
            div { class: "build-refine-foot",
                button {
                    class: "primary-button is-small",
                    r#type: "submit",
                    disabled: empty || !can_ask,
                    if sending {
                        span { class: "spinner" }
                    }
                    "Доопрацювати"
                }
            }
        }
    }
}

#[component]
fn LinkForm(engine: GameEngine) -> Element {
    let board = use_context::<Board>();
    let mut folder = use_signal(String::new);
    let mut linking = use_signal(|| false);
    let submit = move |event: FormEvent| {
        event.prevent_default();
        if linking() || folder.read().trim().is_empty() {
            return;
        }
        linking.set(true);
        spawn(async move {
            let result = build_bridge::link_source(board.project_id, &folder()).await;
            if result.is_ok() {
                folder.set(String::new());
            }
            board.settle(result);
            linking.set(false);
        });
    };
    rsx! {
        form { class: "build-link", novalidate: true, onsubmit: submit,
            input {
                class: "input is-mono",
                r#type: "text",
                aria_label: "Папка проєкту {engine.label()}",
                placeholder: "Папка з {project_marker(engine)}, напр. C:\\Games\\MyGame",
                maxlength: "{MAX_SOURCE_PATH_CHARS}",
                spellcheck: "false",
                value: "{folder}",
                oninput: move |event| folder.set(event.value()),
            }
            button {
                class: "ghost-button is-small",
                r#type: "submit",
                disabled: linking() || folder.read().trim().is_empty(),
                "Підключити папку"
            }
        }
    }
}

#[derive(Clone, PartialEq)]
struct TargetChoice {
    target: BuildTarget,
    picked: bool,
    pickable: bool,
    tone: &'static str,
    state: String,
}

#[component]
fn TargetGrid(choices: Vec<TargetChoice>) -> Element {
    let studio = use_studio();
    rsx! {
        div { class: "build-families", role: "group", aria_label: "Варіанти білду",
            for family in BuildFamily::ALL {
                div { key: "{family.label()}", class: "build-family",
                    p { class: "mono-label", "{family.label()}" }
                    div { class: "build-targets",
                        for choice in choices.iter().filter(|choice| choice.target.family() == family).cloned() {
                            {
                                let target = choice.target;
                                rsx! {
                                    button {
                                        key: "{target.code()}",
                                        class: target_class(choice.picked, choice.pickable),
                                        r#type: "button",
                                        disabled: !choice.pickable,
                                        aria_pressed: if choice.picked { "true" } else { "false" },
                                        title: "{choice.state}",
                                        onclick: move |_| studio.toggle_build_target(target),
                                        span { class: "build-target-head",
                                            span { class: "build-target-check" }
                                            span { class: "build-target-name", "{target.label()}" }
                                            span { class: "build-target-package", "{target.package()}" }
                                        }
                                        span { class: "build-target-state {choice.tone}", "{choice.state}" }
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
fn PlanShortcuts(supported: Vec<BuildTarget>, picked: usize) -> Element {
    let studio = use_studio();
    let everything_picked = picked == supported.len();
    rsx! {
        div { class: "build-actions-group",
            button {
                class: "ghost-button is-small",
                r#type: "button",
                disabled: everything_picked,
                onclick: move |_| studio.add_build_targets(&supported),
                "Усі платформи"
            }
            button {
                class: "ghost-button is-small",
                r#type: "button",
                disabled: picked == 0,
                onclick: move |_| studio.set_build_plan(&[]),
                "Очистити"
            }
        }
    }
}

#[component]
pub fn PlanCard() -> Element {
    let studio = use_studio();
    let choices: Vec<TargetChoice> = BuildTarget::ALL
        .into_iter()
        .map(|target| TargetChoice {
            target,
            picked: studio.plans_build(target),
            pickable: true,
            tone: "is-off",
            state: target_hint(target).to_owned(),
        })
        .collect();
    let picked = choices.iter().filter(|choice| choice.picked).count();
    rsx! {
        section { class: "build-card",
            CardHead {
                step: "01",
                title: "Варіанти білду",
                note: "Обери, під які платформи потрібна гра. Вибір збережеться — коли конвеєр напише код, зберемо саме ці білди.".to_owned(),
            }
            TargetGrid { choices }
            div { class: "build-actions is-spread",
                PlanShortcuts { supported: BuildTarget::ALL.to_vec(), picked }
                span { class: "mono-label", "{plan_summary(picked, None)}" }
            }
        }
    }
}

#[component]
fn TargetsCard() -> Element {
    let board = use_context::<Board>();
    let studio = use_studio();
    let mut starting = use_signal(|| false);
    let Some(overview) = (board.overview)() else {
        return rsx! {};
    };
    let engine = overview.engine;
    let can_build = overview.can_build();
    let choices: Vec<TargetChoice> = BuildTarget::ALL
        .into_iter()
        .map(|target| {
            let readiness = overview.readiness(target);
            let pickable = !matches!(readiness, Readiness::Unsupported);
            let (tone, state) = readiness_view(&readiness, engine);
            TargetChoice {
                target,
                picked: pickable && studio.plans_build(target),
                pickable,
                tone,
                state,
            }
        })
        .collect();
    let supported: Vec<BuildTarget> = choices
        .iter()
        .filter(|choice| choice.pickable)
        .map(|choice| choice.target)
        .collect();
    let picked = choices.iter().filter(|choice| choice.picked).count();
    let buildable: Vec<BuildTarget> = choices
        .iter()
        .filter(|choice| choice.picked && can_build && overview.readiness(choice.target).is_ready())
        .map(|choice| choice.target)
        .collect();
    let count = buildable.len();
    let ready_count = overview.targets.iter().filter(|report| report.readiness.is_ready()).count();
    let note = if can_build {
        format!(
            "Обери потрібні варіанти білду — вибір збережеться. Збірка йде на цьому комп’ютері: зараз доступно {ready_count} з {}.",
            BuildTarget::ALL.len()
        )
    } else {
        "Обери потрібні варіанти білду вже зараз — зберемо їх, щойно буде код гри (крок 01).".to_owned()
    };

    let start = move |_| {
        let targets = buildable.clone();
        if targets.is_empty() || starting() {
            return;
        }
        starting.set(true);
        spawn(async move {
            board.settle(build_bridge::start(board.project_id, &targets).await);
            starting.set(false);
        });
    };

    rsx! {
        section { class: "build-card",
            CardHead { step: "02", title: "Варіанти білду", note }
            TargetGrid { choices }
            div { class: "build-actions is-spread",
                PlanShortcuts { supported, picked }
                div { class: "build-actions-group",
                    span { class: "mono-label", "{plan_summary(picked, Some(count))}" }
                    button {
                        class: "primary-button",
                        r#type: "button",
                        disabled: count == 0 || starting(),
                        onclick: start,
                        if starting() {
                            span { class: "spinner" }
                            "Ставимо в чергу…"
                        } else {
                            "Зібрати"
                            ArrowRightIcon {}
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn HistoryCard() -> Element {
    let board = use_context::<Board>();
    let Some(overview) = (board.overview)() else {
        return rsx! {};
    };
    let generating = overview
        .source
        .as_ref()
        .is_some_and(|source| source.status == SourceStatus::Generating);
    let note = format!("Готові файли лежать у {}", overview.output_dir);
    rsx! {
        section { class: "build-card",
            CardHead { step: "03", title: "Збірки", note }
            if overview.builds.is_empty() {
                p { class: "build-empty", "Ще нічого не збирали — обери платформи вище й натисни «Зібрати»." }
            } else {
                ul { class: "build-list",
                    for entry in overview.builds.clone() {
                        BuildRow { key: "{entry.id}", entry, generating }
                    }
                }
            }
        }
    }
}

#[component]
fn BuildRow(entry: Build, generating: bool) -> Element {
    let board = use_context::<Board>();
    let studio = use_studio();
    let id = entry.id;
    let status = entry.status;

    let reveal = move |_| {
        spawn(async move { board.report(build_bridge::reveal(id).await) });
    };
    let cancel = move |_| {
        spawn(async move { board.report(build_bridge::cancel(id).await) });
    };
    let fix = move |_| {
        spawn(async move { board.report(build_bridge::fix(id).await) });
    };
    let open_log = move |_| {
        spawn(async move { board.report(build_bridge::open_log(id).await) });
    };

    let tail = recent_lines(&entry.log_tail, VISIBLE_LOG_LINES);

    rsx! {
        li { class: "build-row {status_tone(status)}",
            div { class: "build-row-main",
                div { class: "build-row-text",
                    span { class: "build-row-target",
                        "{entry.target.label()}"
                        span { class: "build-row-package", "{entry.target.package()}" }
                    }
                    span { class: "build-row-meta", "{build_meta(&entry)}" }
                }
                span { class: "design-status {status_tone(status)}",
                    if status == BuildStatus::Running {
                        span { class: "spinner is-tiny" }
                    }
                    "{status_label(status)}"
                }
                div { class: "build-row-actions",
                    match status {
                        BuildStatus::Done => rsx! {
                            button { class: "primary-button is-small", r#type: "button", onclick: reveal,
                                "Показати файл"
                                ExternalIcon {}
                            }
                        },
                        BuildStatus::Queued | BuildStatus::Running => rsx! {
                            button { class: "ghost-button is-small", r#type: "button", onclick: cancel, "Скасувати" }
                        },
                        BuildStatus::Failed => rsx! {
                            button {
                                class: "ghost-button is-small",
                                r#type: "button",
                                disabled: generating || !studio.connected(),
                                title: "Claude прочитає лог, виправить код гри, і можна буде зібрати ще раз",
                                onclick: fix,
                                "Виправити з Claude"
                            }
                        },
                        BuildStatus::Cancelled => rsx! {},
                    }
                    if status != BuildStatus::Queued {
                        button { class: "ghost-button is-small", r#type: "button", onclick: open_log, "Лог" }
                    }
                }
            }
            if let Some(path) = entry.artifact.clone().filter(|_| status == BuildStatus::Done) {
                code { class: "build-path", "{path}" }
            }
            if let Some(error) = entry.error.clone() {
                p { class: "build-error", "{error}" }
            }
            if status == BuildStatus::Running && !tail.is_empty() {
                pre { class: "build-log", "{tail}" }
            }
            if status == BuildStatus::Failed && !tail.is_empty() {
                details { class: "build-more",
                    summary { "Останні рядки логу" }
                    pre { class: "build-log", "{tail}" }
                }
            }
        }
    }
}

fn target_class(picked: bool, pickable: bool) -> &'static str {
    match (picked, pickable) {
        (true, _) => "build-target is-picked",
        (false, true) => "build-target",
        (false, false) => "build-target is-blocked",
    }
}

fn target_hint(target: BuildTarget) -> &'static str {
    match target {
        BuildTarget::Windows => "ПК з Windows",
        BuildTarget::Macos => "Комп’ютери Mac",
        BuildTarget::Linux => "ПК з Linux і Steam Deck",
        BuildTarget::Web => "Грається просто в браузері",
        BuildTarget::Android => "Телефони й планшети Android",
        BuildTarget::Ios => "iPhone та iPad",
    }
}

fn plan_summary(picked: usize, buildable: Option<usize>) -> String {
    match (picked, buildable) {
        (0, _) => "Нічого не вибрано".to_owned(),
        (picked, Some(ready)) if ready < picked => format!("Вибрано: {picked} · зібрати зараз: {ready}"),
        (picked, _) => format!("Вибрано: {picked}"),
    }
}

fn readiness_view(readiness: &Readiness, engine: GameEngine) -> (&'static str, String) {
    match readiness {
        Readiness::Ready(toolchain) => ("is-ready", toolchain.clone()),
        Readiness::Missing(what) => ("is-missing", format!("Потрібно: {what}")),
        Readiness::NeedsHost(host) => ("is-host", format!("Збирається лише на {host}")),
        Readiness::Unsupported => ("is-off", format!("{} не збирає під цю платформу", engine.label())),
    }
}

fn status_label(status: BuildStatus) -> &'static str {
    match status {
        BuildStatus::Queued => "У черзі",
        BuildStatus::Running => "Збирається",
        BuildStatus::Done => "Готово",
        BuildStatus::Failed => "Помилка",
        BuildStatus::Cancelled => "Скасовано",
    }
}

fn status_tone(status: BuildStatus) -> &'static str {
    match status {
        BuildStatus::Queued => "is-queued",
        BuildStatus::Running => "is-generating",
        BuildStatus::Done => "is-approved",
        BuildStatus::Failed => "is-failed",
        BuildStatus::Cancelled => "is-draft",
    }
}

fn build_meta(build: &Build) -> String {
    let mut parts = vec![clock(build.created_at)];
    if let Some(started) = build.started_at {
        let finished = build.finished_at.unwrap_or_else(now_seconds);
        parts.push(format_duration(finished - started));
    }
    if let Some(size) = build.size_bytes {
        parts.push(format_size(size));
    }
    parts.join(" · ")
}

fn recent_lines(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(count)..].join("\n")
}

fn clock(seconds: i64) -> String {
    let date = js_sys::Date::new(&JsValue::from_f64(seconds as f64 * MILLIS_PER_SECOND));
    format!(
        "{:02}.{:02} {:02}:{:02}",
        date.get_date(),
        date.get_month() + 1,
        date.get_hours(),
        date.get_minutes()
    )
}

fn now_seconds() -> i64 {
    (js_sys::Date::now() / MILLIS_PER_SECOND) as i64
}

pub fn explain_build(problem: &BuildError) -> String {
    match problem {
        BuildError::SignedOut => "Сесія завершилась — увійди знову.".to_owned(),
        BuildError::NotFound => "Цього проєкту чи збірки вже немає.".to_owned(),
        BuildError::NoSource => "Спершу потрібен код гри: згенеруй його або підключи папку.".to_owned(),
        BuildError::SourceBusy => "Claude ще пише код гри — зачекай або зупини генерацію.".to_owned(),
        BuildError::SourceInvalid => {
            "У цій папці немає головного файлу проєкту обраного рушія — перевір шлях або рушій.".to_owned()
        }
        BuildError::PathInvalid => "Такої папки немає. Встав повний шлях, напр. C:\\Games\\MyGame.".to_owned(),
        BuildError::RequestTooLong => {
            format!("Завдання задовге — максимум {MAX_BUILD_REQUEST_CHARS} символів.")
        }
        BuildError::NothingSelected => "Обери хоча б одну платформу.".to_owned(),
        BuildError::TargetUnavailable(target) => format!(
            "{} зараз не зібрати на цьому комп’ютері — на картці платформи написано, чого бракує.",
            target.label()
        ),
        BuildError::Claude(problem) => explain(problem),
        BuildError::Process(detail) => format!("Не вдалося: {detail}"),
        BuildError::Storage(detail) => {
            tracing::error!("build storage failed: {detail}");
            "Не вдалося звернутися до сховища збірок.".to_owned()
        }
    }
}
