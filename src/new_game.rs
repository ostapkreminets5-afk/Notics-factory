use dioxus::prelude::*;
use noctics_shared::{MAX_PROMPT_CHARS, StudioError, validate_game_prompt};

use crate::bridge;
use crate::icons::ArrowRightIcon;
use crate::studio::{Section, explain, use_studio};

const IDEA_LIMIT: usize = 3000;
const STYLE_NOTE_LIMIT: usize = 300;
const IDEA_EXCERPT_CHARS: usize = 240;
const PARAMETER_COUNT: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    Idea,
    Style,
    Parameters,
    Launch,
}

impl Step {
    const ALL: [Self; 4] = [Self::Idea, Self::Style, Self::Parameters, Self::Launch];

    fn position(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or_default()
    }

    fn code(self) -> String {
        format!("{:02}", self.position() + 1)
    }

    fn title(self) -> &'static str {
        match self {
            Self::Idea => "Ідея",
            Self::Style => "Стиль",
            Self::Parameters => "Параметри",
            Self::Launch => "Запуск",
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Self::Idea => "Ідея гри",
            Self::Style => "Візуальний стиль",
            Self::Parameters => "Параметри гри",
            Self::Launch => "Запуск ядра",
        }
    }

    fn next(self) -> Option<Self> {
        Self::ALL.get(self.position() + 1).copied()
    }

    fn previous(self) -> Option<Self> {
        self.position().checked_sub(1).map(|index| Self::ALL[index])
    }
}

struct StarterIdea {
    title: &'static str,
    prompt: &'static str,
}

static STARTER_IDEAS: [StarterIdea; 3] = [
    StarterIdea {
        title: "Затишний симулятор маяка",
        prompt: "Затишний симулятор доглядача маяка на скелястому острові: вночі тримаєш світло для кораблів у шторм, удень лагодиш механізми, ловиш рибу й читаєш листи від моряків, яких урятував.",
    },
    StarterIdea {
        title: "Рогалик у підземеллі",
        prompt: "Рогалик у підземеллі, що перебудовується після кожної смерті: ліхтарник спускається все глибше, збирає реліквії, торгується з тінями й вирішує, які спогади забрати з собою в наступний забіг.",
    },
    StarterIdea {
        title: "Головоломка зі світлом і тінню",
        prompt: "Головоломка, де гравець керує джерелом світла, а тіні від предметів стають мостами, сходами й платформами для маленького мандрівника, який шукає дорогу додому.",
    },
];

struct VisualStyle {
    title: &'static str,
    note: &'static str,
    brief: &'static str,
    palette: [&'static str; 5],
}

static STYLES: [VisualStyle; 6] = [
    VisualStyle {
        title: "Неонова ніч",
        note: "Темне місто, неон, контрастне світло",
        brief: "темна нічна атмосфера, неонові відблиски, глибокі сині тіні й контрастні акценти світла",
        palette: ["#070a1a", "#1d2a6b", "#3b7bff", "#b06cff", "#2dd4ee"],
    },
    VisualStyle {
        title: "Затишна акварель",
        note: "Теплі пастелі, м’яке світло",
        brief: "м’яка акварельна стилістика, теплі пастельні кольори, затишне розсіяне світло",
        palette: ["#f6efe1", "#e8c79a", "#9cc5a1", "#5b8f8a", "#2f3e46"],
    },
    VisualStyle {
        title: "Піксель-арт 16-біт",
        note: "Ретро-спрайти, обмежена палітра",
        brief: "піксель-арт у дусі 16-бітних консолей, чіткі спрайти, обмежена соковита палітра",
        palette: ["#1a1c2c", "#5d275d", "#b13e53", "#ef7d57", "#ffcd75"],
    },
    VisualStyle {
        title: "Low-poly",
        note: "Чисті грані, яскраві акценти",
        brief: "low-poly 3D з чистими гранями, плоским затіненням і яскравими кольоровими акцентами",
        palette: ["#0f172a", "#2563eb", "#38bdf8", "#f59e0b", "#f8fafc"],
    },
    VisualStyle {
        title: "Похмурий нуар",
        note: "Монохром, дощ, один акцент",
        brief: "похмурий нуар, майже монохромна гама, дощ і жорсткі тіні з одним червоним акцентом",
        palette: ["#0b0b0c", "#2b2b2e", "#6b6b70", "#c9c9cc", "#c8102e"],
    },
    VisualStyle {
        title: "Казкова мальовка",
        note: "Ручний живопис, м’які форми",
        brief: "мальована вручну казкова стилістика, м’які форми, текстурні мазки й теплі відтінки",
        palette: ["#24314f", "#4f6d7a", "#c0d6df", "#e8a87c", "#f4e1c1"],
    },
];

struct Parameter {
    title: &'static str,
    options: &'static [&'static str],
    multiple: bool,
    default: u32,
}

impl Parameter {
    fn choose(&self, mask: u32, option: usize) -> u32 {
        if !self.multiple {
            return bit(option);
        }
        match mask ^ bit(option) {
            0 => mask,
            toggled => toggled,
        }
    }

    fn labels(&self, mask: u32) -> Vec<&'static str> {
        self.options
            .iter()
            .enumerate()
            .filter(|(option, _)| is_picked(mask, *option))
            .map(|(_, label)| *label)
            .collect()
    }
}

static PARAMETERS: [Parameter; PARAMETER_COUNT] = [
    Parameter {
        title: "Камера",
        options: &["2D збоку", "Згори вниз", "Ізометрія", "3D від третьої особи", "Від першої особи"],
        multiple: false,
        default: 1 << 3,
    },
    Parameter {
        title: "Гравці",
        options: &["Соло", "Кооператив", "PvP"],
        multiple: true,
        default: 1 << 0,
    },
    Parameter {
        title: "Платформи",
        options: &["ПК", "Мобільні", "Консолі", "Браузер"],
        multiple: true,
        default: 1 << 0,
    },
    Parameter {
        title: "Тривалість сесії",
        options: &["5–10 хвилин", "20–40 хвилин", "Година й більше"],
        multiple: false,
        default: 1 << 1,
    },
    Parameter {
        title: "Обсяг інтерфейсу",
        options: &["Компактний · 3–4 екрани", "Середній · 5–6 екранів", "Розлогий · 7–8 екранів"],
        multiple: false,
        default: 1 << 1,
    },
];

struct CoreStage {
    code: &'static str,
    title: &'static str,
    note: &'static str,
    tone: &'static str,
}

static CORE_STAGES: [CoreStage; 6] = [
    CoreStage {
        code: "01",
        title: "Промпт",
        note: "Дизайн-документ: механіки, рівні, персонажі",
        tone: "is-sky",
    },
    CoreStage {
        code: "02",
        title: "Концепт",
        note: "Референси з Pinterest і концепт-арти в ComfyUI",
        tone: "is-violet",
    },
    CoreStage {
        code: "03",
        title: "Модель",
        note: "3D-моделі персонажів і пропсів",
        tone: "is-purple",
    },
    CoreStage {
        code: "04",
        title: "Матеріал",
        note: "PBR-текстури за палітрою гри",
        tone: "is-violet",
    },
    CoreStage {
        code: "05",
        title: "Анімація",
        note: "Риг і цикли рухів персонажів",
        tone: "is-cyan",
    },
    CoreStage {
        code: "06",
        title: "Збірка",
        note: "Рівні, інтерфейс і готовий білд",
        tone: "is-blue",
    },
];

fn bit(option: usize) -> u32 {
    1 << option
}

fn is_picked(mask: u32, option: usize) -> bool {
    mask & bit(option) != 0
}

fn default_picks() -> [u32; PARAMETER_COUNT] {
    std::array::from_fn(|index| PARAMETERS[index].default)
}

fn compose_brief(idea: &str, style: &VisualStyle, style_note: &str, picks: &[u32; PARAMETER_COUNT]) -> String {
    let mut lines = vec![
        idea.trim().to_owned(),
        String::new(),
        format!("Візуальний стиль: {} — {}.", style.title, style.brief),
        format!("Палітра: {}.", style.palette.join(", ")),
    ];
    let style_note = style_note.trim();
    if !style_note.is_empty() {
        lines.push(format!("Побажання до стилю: {style_note}"));
    }
    for (parameter, mask) in PARAMETERS.iter().zip(picks) {
        lines.push(format!("{}: {}.", parameter.title, parameter.labels(*mask).join(", ")));
    }
    lines.join("\n")
}

fn excerpt(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= IDEA_EXCERPT_CHARS {
        return text.to_owned();
    }
    let cut: String = text.chars().take(IDEA_EXCERPT_CHARS).collect();
    format!("{}…", cut.trim_end())
}

fn step_class(candidate: Step, current: Step) -> &'static str {
    match candidate.position().cmp(&current.position()) {
        std::cmp::Ordering::Equal => "wizard-step is-active",
        std::cmp::Ordering::Less => "wizard-step is-done",
        std::cmp::Ordering::Greater => "wizard-step",
    }
}

#[component]
pub fn NewGamePage() -> Element {
    let studio = use_studio();
    let mut step = use_signal(|| Step::Idea);
    let idea = use_signal(String::new);
    let style = use_signal(|| 0_usize);
    let style_note = use_signal(String::new);
    let picks = use_signal(default_picks);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<StudioError>);

    let mut go_to = move |target: Step| {
        if pending() {
            return;
        }
        if target != Step::Idea {
            let checked = validate_game_prompt(&idea.read());
            if let Err(problem) = checked {
                error.set(Some(problem));
                step.set(Step::Idea);
                return;
            }
        }
        error.set(None);
        step.set(target);
    };

    let mut launch = move || {
        if pending() {
            return;
        }
        let brief = compose_brief(&idea.read(), &STYLES[style()], &style_note.read(), &picks());
        let brief = match validate_game_prompt(&brief) {
            Ok(brief) => brief,
            Err(problem) => {
                error.set(Some(problem));
                return;
            }
        };
        pending.set(true);
        error.set(None);
        spawn(async move {
            match bridge::start_project(&brief).await {
                Ok(project) => {
                    studio.changed();
                    studio.open_project(project.id);
                }
                Err(problem) => {
                    error.set(Some(problem));
                    pending.set(false);
                }
            }
        });
    };

    let mut advance = move || match step().next() {
        Some(next) => go_to(next),
        None => launch(),
    };

    let current = step();

    rsx! {
        section { class: "page new-game",
            header { class: "page-head",
                nav { class: "crumbs mono-label", aria_label: "Шлях",
                    button {
                        class: "crumb-link",
                        r#type: "button",
                        onclick: move |_| studio.open(Section::Projects),
                        "Проєкти"
                    }
                    span { class: "crumb-divider", "/" }
                    span { aria_current: "page", "Нова гра" }
                }
                h1 { class: "page-title", "Нова гра з промпту" }
                p { class: "page-lead", "Опиши ідею — ядро проведе її через шість етапів конвеєра." }
            }
            ol { class: "wizard-steps", aria_label: "Кроки",
                for candidate in Step::ALL {
                    li { key: "{candidate:?}",
                        button {
                            class: step_class(candidate, current),
                            r#type: "button",
                            aria_current: if candidate == current { "step" } else { "false" },
                            onclick: move |_| go_to(candidate),
                            span { class: "wizard-step-code", "{candidate.code()}" }
                            span { class: "wizard-step-title", "{candidate.title()}" }
                        }
                    }
                }
            }
            div { class: "wizard-body",
                form {
                    class: "wizard-card",
                    novalidate: true,
                    onsubmit: move |event| {
                        event.prevent_default();
                        advance();
                    },
                    p { class: "mono-label wizard-card-label", "{current.code()} · {current.heading()}" }
                    div { class: "wizard-panel", key: "{current:?}",
                        {
                            match current {
                                Step::Idea => rsx! {
                                    IdeaStep { idea, on_submit: move |_| advance() }
                                },
                                Step::Style => rsx! {
                                    StyleStep { style, style_note }
                                },
                                Step::Parameters => rsx! {
                                    ParametersStep { picks }
                                },
                                Step::Launch => rsx! {
                                    LaunchStep { idea, style, style_note, picks }
                                },
                            }
                        }
                    }
                    if let Some(problem) = error() {
                        div { class: "composer-alert", role: "alert",
                            span { "{explain(&problem)}" }
                            if matches!(problem, StudioError::ClaudeCodeMissing | StudioError::ClaudeSignedOut) {
                                button {
                                    class: "ghost-button is-small",
                                    r#type: "button",
                                    onclick: move |_| studio.open(Section::Settings),
                                    "Налаштування"
                                }
                            }
                        }
                    }
                    div { class: "wizard-foot",
                        button {
                            class: "ghost-button",
                            r#type: "button",
                            disabled: current.previous().is_none() || pending(),
                            onclick: move |_| {
                                if let Some(previous) = step().previous() {
                                    error.set(None);
                                    step.set(previous);
                                }
                            },
                            "Назад"
                        }
                        button {
                            class: "primary-button",
                            r#type: "submit",
                            disabled: pending(),
                            if pending() {
                                span { class: "spinner" }
                                "Запускаємо…"
                            } else if current == Step::Launch {
                                "Запустити ядро"
                                ArrowRightIcon {}
                            } else {
                                "Далі"
                            }
                        }
                    }
                }
                CorePlan {}
            }
        }
    }
}

#[component]
fn IdeaStep(idea: Signal<String>, on_submit: EventHandler<()>) -> Element {
    let mut idea = idea;
    let length = idea.read().chars().count();

    rsx! {
        textarea {
            class: "input is-area wizard-idea",
            id: "new-game-idea",
            aria_label: "Ідея гри",
            rows: "5",
            maxlength: "{IDEA_LIMIT}",
            placeholder: "Наприклад: стелс-гра в нічному місті, де кіт-злодій збирає світлячків на дахах.",
            value: "{idea}",
            oninput: move |event| idea.set(event.value()),
            onkeydown: move |event| {
                if event.key() == Key::Enter && event.modifiers().ctrl() {
                    event.prevent_default();
                    on_submit.call(());
                }
            },
            onmounted: move |event| async move {
                let _ = event.data().set_focus(true).await;
            },
        }
        div { class: "wizard-row",
            div { class: "idea-starters",
                span { class: "idea-starters-label", "Ідеї для старту:" }
                for starter in STARTER_IDEAS.iter() {
                    button {
                        key: "{starter.title}",
                        class: "idea-chip",
                        r#type: "button",
                        onclick: move |_| idea.set(starter.prompt.to_owned()),
                        "{starter.title}"
                    }
                }
            }
            span { class: "mono-label wizard-count", "{length} / {IDEA_LIMIT}" }
        }
    }
}

#[component]
fn StyleStep(style: Signal<usize>, style_note: Signal<String>) -> Element {
    let mut style = style;
    let mut style_note = style_note;
    let chosen = style();

    rsx! {
        div { class: "style-options", role: "radiogroup", aria_label: "Візуальний стиль",
            for (index, option) in STYLES.iter().enumerate() {
                button {
                    key: "{option.title}",
                    class: if chosen == index { "style-option is-active" } else { "style-option" },
                    r#type: "button",
                    role: "radio",
                    aria_checked: if chosen == index { "true" } else { "false" },
                    onclick: move |_| style.set(index),
                    span { class: "style-option-palette", "aria-hidden": "true",
                        for color in option.palette.iter() {
                            span { key: "{color}", style: "background: {color}" }
                        }
                    }
                    span { class: "style-option-title", "{option.title}" }
                    span { class: "style-option-note", "{option.note}" }
                }
            }
        }
        label { class: "mono-label wizard-field-label", r#for: "new-game-style-note",
            "Свої побажання до стилю · необов’язково"
        }
        textarea {
            class: "input is-area",
            id: "new-game-style-note",
            rows: "2",
            maxlength: "{STYLE_NOTE_LIMIT}",
            placeholder: "Наприклад: більше туману, теплі ліхтарі, без кислотних кольорів.",
            value: "{style_note}",
            oninput: move |event| style_note.set(event.value()),
        }
    }
}

#[component]
fn ParametersStep(picks: Signal<[u32; PARAMETER_COUNT]>) -> Element {
    let mut picks = picks;
    let current = picks();

    rsx! {
        div { class: "parameter-list",
            for (index, parameter) in PARAMETERS.iter().enumerate() {
                div {
                    key: "{parameter.title}",
                    class: "parameter",
                    role: "group",
                    aria_label: "{parameter.title}",
                    p { class: "parameter-title",
                        "{parameter.title}"
                        if parameter.multiple {
                            span { class: "parameter-hint", "можна кілька" }
                        }
                    }
                    div { class: "parameter-options",
                        for (option, label) in parameter.options.iter().enumerate() {
                            button {
                                key: "{label}",
                                class: if is_picked(current[index], option) { "option-pill is-active" } else { "option-pill" },
                                r#type: "button",
                                aria_pressed: if is_picked(current[index], option) { "true" } else { "false" },
                                onclick: move |_| {
                                    let updated = parameter.choose(picks()[index], option);
                                    picks.write()[index] = updated;
                                },
                                "{label}"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn LaunchStep(
    idea: Signal<String>,
    style: Signal<usize>,
    style_note: Signal<String>,
    picks: Signal<[u32; PARAMETER_COUNT]>,
) -> Element {
    let studio = use_studio();
    let chosen = &STYLES[style()];
    let note = style_note.read().trim().to_owned();
    let current = picks();
    let brief = compose_brief(&idea.read(), chosen, &note, &current);
    let length = brief.chars().count();
    let too_long = length > MAX_PROMPT_CHARS;
    let key_missing = studio.settings.read().is_some() && !studio.connected();

    rsx! {
        dl { class: "launch-summary",
            div { class: "launch-row",
                dt { "Ідея" }
                dd { "{excerpt(&idea.read())}" }
            }
            div { class: "launch-row",
                dt { "Стиль" }
                dd { class: "launch-style",
                    span { class: "style-option-palette is-inline", "aria-hidden": "true",
                        for color in chosen.palette.iter() {
                            span { key: "{color}", style: "background: {color}" }
                        }
                    }
                    span { "{chosen.title}" }
                    if !note.is_empty() {
                        span { class: "launch-muted", "· {note}" }
                    }
                }
            }
            for (parameter, mask) in PARAMETERS.iter().zip(current) {
                div { key: "{parameter.title}", class: "launch-row",
                    dt { "{parameter.title}" }
                    dd { "{parameter.labels(mask).join(\", \")}" }
                }
            }
        }
        if key_missing {
            div { class: "page-alert is-info",
                span { "Підписку Claude ще не підключено — без неї ядро не стартує." }
                button {
                    class: "ghost-button is-small",
                    r#type: "button",
                    onclick: move |_| studio.open(Section::Settings),
                    "Налаштування"
                }
            }
        }
        div { class: "launch-brief-head",
            span { class: "mono-label", "Бриф для ядра" }
            span { class: if too_long { "mono-label is-over" } else { "mono-label" }, "{length} / {MAX_PROMPT_CHARS}" }
        }
        pre { class: "launch-brief", "{brief}" }
    }
}

#[component]
fn CorePlan() -> Element {
    rsx! {
        aside { class: "core-plan", aria_label: "Що зробить ядро",
            p { class: "mono-label core-plan-label", "Що зробить ядро" }
            ol { class: "core-plan-list",
                for stage in CORE_STAGES.iter() {
                    li { key: "{stage.code}", class: "core-stage {stage.tone}",
                        span { class: "core-stage-code", "{stage.code}" }
                        span { class: "core-stage-text",
                            span { class: "core-stage-title", "{stage.title}" }
                            span { class: "core-stage-note", "{stage.note}" }
                        }
                    }
                }
            }
        }
    }
}
