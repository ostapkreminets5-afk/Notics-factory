use dioxus::prelude::*;
use dioxus_logger::tracing;
use noctics_shared::{
    FIRST_STAGE, LAST_STAGE, MAX_SKILL_INSTRUCTIONS_CHARS, MAX_SKILL_NAME_CHARS,
    MAX_SKILL_SUMMARY_CHARS, MAX_SKILL_TITLE_CHARS, Skill, SkillCategory, SkillDraft, SkillError,
    suggest_skill_name,
};

use crate::bridge;
use crate::icons::{CloseIcon, TrashIcon};

const STAGE_NAMES: [&str; 6] = ["Промпт", "Концепт", "Модель", "Матеріал", "Анімація", "Збірка"];
const FILTERS: [Filter; 4] = [
    Filter::All,
    Filter::Only(SkillCategory::GameDesign),
    Filter::Only(SkillCategory::Art),
    Filter::Only(SkillCategory::Code),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Only(SkillCategory),
}

impl Filter {
    fn admits(self, skill: &Skill) -> bool {
        match self {
            Self::All => true,
            Self::Only(category) => skill.category == category,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "Усі",
            Self::Only(category) => category_label(category),
        }
    }

    fn preset(self) -> SkillCategory {
        match self {
            Self::All => SkillCategory::GameDesign,
            Self::Only(category) => category,
        }
    }
}

#[derive(Clone, PartialEq)]
enum Editor {
    Closed,
    Creating(SkillCategory),
    Editing(Skill),
}

#[component]
pub fn SkillsPage() -> Element {
    let mut skills = use_signal(Vec::<Skill>::new);
    let mut loading = use_signal(|| true);
    let mut notice = use_signal(|| None::<String>);
    let mut filter = use_signal(|| Filter::All);
    let mut editor = use_signal(|| Editor::Closed);
    let mut export_dir = use_signal(|| None::<String>);

    use_future(move || async move {
        match bridge::list_skills().await {
            Ok(list) => skills.set(list),
            Err(problem) => notice.set(Some(explain(&problem))),
        }
        loading.set(false);
        if let Ok(folder) = bridge::skills_folder().await {
            export_dir.set(folder);
        }
    });

    let mut toggle = move |id: i64, enabled: bool| {
        set_enabled_locally(&mut skills.write(), id, enabled);
        spawn(async move {
            match bridge::set_skill_enabled(id, enabled).await {
                Ok(saved) => upsert(&mut skills.write(), saved),
                Err(problem) => {
                    set_enabled_locally(&mut skills.write(), id, !enabled);
                    notice.set(Some(explain(&problem)));
                }
            }
        });
    };

    let active_filter = filter();
    let visible: Vec<Skill> = skills
        .read()
        .iter()
        .filter(|skill| active_filter.admits(skill))
        .cloned()
        .collect();
    let tallies: Vec<(Filter, usize)> = FILTERS
        .into_iter()
        .map(|option| {
            let count = skills.read().iter().filter(|skill| option.admits(skill)).count();
            (option, count)
        })
        .collect();
    let active_count = skills.read().iter().filter(|skill| skill.enabled).count();

    rsx! {
        section { class: "page",
            header { class: "page-head has-action",
                div { class: "page-head-text",
                    p { class: "mono-label", "Система · Skills" }
                    h1 { class: "page-title", "Навички AI-ядра" }
                    p { class: "page-lead", "Інструкції, якими ядро керується на кожному етапі конвеєра." }
                }
                button {
                    class: "primary-button",
                    r#type: "button",
                    onclick: move |_| editor.set(Editor::Creating(filter().preset())),
                    "+ Нова навичка"
                }
            }
            div { class: "filter-tabs", role: "tablist", aria_label: "Напрям навичок",
                for (option, count) in tallies {
                    button {
                        key: "{option.label()}",
                        class: if option == active_filter { "filter-tab is-active" } else { "filter-tab" },
                        r#type: "button",
                        role: "tab",
                        aria_selected: if option == active_filter { "true" } else { "false" },
                        onclick: move |_| filter.set(option),
                        span { "{option.label()}" }
                        if !loading() {
                            span { class: "filter-tab-count", "{count}" }
                        }
                    }
                }
            }
            if let Some(message) = notice() {
                div { class: "page-alert", role: "alert",
                    span { "{message}" }
                    button {
                        class: "icon-button",
                        r#type: "button",
                        aria_label: "Сховати повідомлення",
                        onclick: move |_| notice.set(None),
                        CloseIcon {}
                    }
                }
            }
            if !loading() && visible.is_empty() {
                div { class: "page-empty",
                    p { class: "mono-label", "Порожньо" }
                    p { "У цьому напрямі ще немає навичок. Створи першу — ядро підхопить її на своєму етапі." }
                }
            }
            div { class: "skill-grid",
                for skill in visible {
                    SkillCard {
                        key: "{skill.id}",
                        on_open: move |picked| editor.set(Editor::Editing(picked)),
                        on_toggle: {
                            let id = skill.id;
                            move |enabled| toggle(id, enabled)
                        },
                        skill,
                    }
                }
            }
            if !loading() {
                p { class: "mono-label skill-export",
                    "Активних: {active_count} · SKILL.md для Claude"
                    if let Some(folder) = export_dir() {
                        span { class: "skill-export-path", " · {folder}" }
                    }
                }
            }
        }
        {
            let close = move |_| editor.set(Editor::Closed);
            let saved = move |skill: Skill| {
                upsert(&mut skills.write(), skill);
                editor.set(Editor::Closed);
            };
            let deleted = move |id: i64| {
                skills.write().retain(|skill| skill.id != id);
                editor.set(Editor::Closed);
            };
            match editor() {
                Editor::Closed => rsx! {},
                Editor::Creating(category) => rsx! {
                    SkillEditor {
                        existing: None,
                        preset_category: category,
                        on_close: close,
                        on_saved: saved,
                        on_deleted: deleted,
                    }
                },
                Editor::Editing(skill) => rsx! {
                    SkillEditor {
                        key: "{skill.id}",
                        preset_category: skill.category,
                        existing: Some(skill),
                        on_close: close,
                        on_saved: saved,
                        on_deleted: deleted,
                    }
                },
            }
        }
    }
}

#[component]
fn SkillCard(skill: Skill, on_open: EventHandler<Skill>, on_toggle: EventHandler<bool>) -> Element {
    let clicked = skill.clone();
    let pressed = skill.clone();
    let status = if skill.enabled { "Активна" } else { "Вимкнено" };

    rsx! {
        article {
            class: if skill.enabled { "skill-card" } else { "skill-card is-off" },
            tabindex: "0",
            aria_label: "{skill.title} — відкрити навичку",
            onclick: move |_| on_open.call(clicked.clone()),
            onkeydown: move |event| {
                if event.key() == Key::Enter {
                    event.prevent_default();
                    on_open.call(pressed.clone());
                }
            },
            div { class: "skill-card-head",
                span { class: "skill-stage", "Етап {skill.stage_code()}" }
                Switch {
                    on: skill.enabled,
                    label: format!("Навичка «{}» активна", skill.title),
                    on_change: move |next| on_toggle.call(next),
                }
            }
            h2 { class: "skill-title", "{skill.title}" }
            p { class: "skill-summary", "{skill.summary}" }
            footer { class: "skill-foot",
                span { "{category_label(skill.category)}" }
                span { "{status}" }
            }
        }
    }
}

#[component]
fn Switch(on: bool, label: String, on_change: EventHandler<bool>) -> Element {
    rsx! {
        button {
            class: "switch",
            r#type: "button",
            role: "switch",
            aria_checked: if on { "true" } else { "false" },
            aria_label: "{label}",
            onclick: move |event| {
                event.stop_propagation();
                on_change.call(!on);
            },
            onkeydown: move |event| event.stop_propagation(),
            span { class: "switch-knob" }
        }
    }
}

#[component]
fn SkillEditor(
    existing: Option<Skill>,
    preset_category: SkillCategory,
    on_close: EventHandler<()>,
    on_saved: EventHandler<Skill>,
    on_deleted: EventHandler<i64>,
) -> Element {
    let editing_id = existing.as_ref().map(|skill| skill.id);
    let initial = existing.as_ref().map_or_else(
        || SkillDraft {
            name: String::new(),
            stage: FIRST_STAGE,
            title: String::new(),
            summary: String::new(),
            category: preset_category,
            instructions: String::new(),
            enabled: true,
        },
        Skill::draft,
    );

    let mut title = use_signal(|| initial.title.clone());
    let mut name = use_signal(|| initial.name.clone());
    let mut name_touched = use_signal(|| editing_id.is_some());
    let mut stage = use_signal(|| initial.stage);
    let mut category = use_signal(|| initial.category);
    let mut summary = use_signal(|| initial.summary.clone());
    let mut instructions = use_signal(|| initial.instructions.clone());
    let mut enabled = use_signal(|| initial.enabled);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut confirming_delete = use_signal(|| false);
    let mut backdrop_pressed = use_signal(|| false);

    let submit = move |event: FormEvent| {
        event.prevent_default();
        if pending() {
            return;
        }
        let draft = SkillDraft {
            name: name(),
            stage: stage(),
            title: title(),
            summary: summary(),
            category: category(),
            instructions: instructions(),
            enabled: enabled(),
        };
        if let Err(problem) = draft.validate() {
            error.set(Some(explain(&problem)));
            return;
        }
        error.set(None);
        pending.set(true);
        spawn(async move {
            let saved = match editing_id {
                Some(id) => bridge::update_skill(id, &draft).await,
                None => bridge::create_skill(&draft).await,
            };
            match saved {
                Ok(skill) => on_saved.call(skill),
                Err(problem) => {
                    error.set(Some(explain(&problem)));
                    pending.set(false);
                }
            }
        });
    };

    let remove = move |_| {
        let Some(id) = editing_id else {
            return;
        };
        if pending() {
            return;
        }
        if !confirming_delete() {
            confirming_delete.set(true);
            return;
        }
        pending.set(true);
        spawn(async move {
            match bridge::delete_skill(id).await {
                Ok(()) => on_deleted.call(id),
                Err(problem) => {
                    error.set(Some(explain(&problem)));
                    confirming_delete.set(false);
                    pending.set(false);
                }
            }
        });
    };

    let heading = if editing_id.is_some() {
        "Редагування навички"
    } else {
        "Нова навичка"
    };
    let action = if editing_id.is_some() {
        "Зберегти"
    } else {
        "Створити навичку"
    };
    let summary_count = summary.read().chars().count();
    let instructions_count = instructions.read().chars().count();

    rsx! {
        div {
            class: "editor-backdrop",
            onmousedown: move |_| backdrop_pressed.set(true),
            onclick: move |_| {
                if backdrop_pressed() && !pending() {
                    on_close.call(());
                }
            },
            onkeydown: move |event| {
                if event.key() == Key::Escape && !pending() {
                    on_close.call(());
                }
            },
            form {
                class: "editor",
                role: "dialog",
                aria_modal: "true",
                aria_label: "{heading}",
                novalidate: true,
                onsubmit: submit,
                onmousedown: move |event| {
                    event.stop_propagation();
                    backdrop_pressed.set(false);
                },
                onclick: move |event| event.stop_propagation(),
                header { class: "editor-head",
                    div {
                        p { class: "mono-label", "Skills · Етап {stage():02}" }
                        h2 { class: "editor-title", "{heading}" }
                    }
                    div { class: "editor-head-side",
                        span { class: "mono-label", if enabled() { "Активна" } else { "Вимкнено" } }
                        Switch {
                            on: enabled(),
                            label: "Навичка активна".to_owned(),
                            on_change: move |next| enabled.set(next),
                        }
                        button {
                            class: "icon-button",
                            r#type: "button",
                            aria_label: "Закрити",
                            onclick: move |_| on_close.call(()),
                            CloseIcon {}
                        }
                    }
                }
                div { class: "editor-body",
                    div { class: "editor-row",
                        div { class: "field",
                            label { class: "mono-label", r#for: "skill-title", "Назва" }
                            input {
                                class: "input",
                                id: "skill-title",
                                r#type: "text",
                                placeholder: "Напр. Ритм анімацій",
                                maxlength: "{MAX_SKILL_TITLE_CHARS}",
                                value: "{title}",
                                oninput: move |event| {
                                    let next = event.value();
                                    if !name_touched() {
                                        name.set(suggest_skill_name(&next));
                                    }
                                    title.set(next);
                                },
                                onmounted: move |event| async move {
                                    let _ = event.data().set_focus(true).await;
                                },
                            }
                        }
                        div { class: "field",
                            div { class: "field-head",
                                label { class: "mono-label", r#for: "skill-name", "Ім’я для Claude" }
                                span { class: "mono-label is-raw", "a-z · 0-9 · -" }
                            }
                            input {
                                class: "input is-mono",
                                id: "skill-name",
                                r#type: "text",
                                placeholder: "animation-rhythm",
                                spellcheck: "false",
                                autocomplete: "off",
                                maxlength: "{MAX_SKILL_NAME_CHARS}",
                                value: "{name}",
                                oninput: move |event| {
                                    name_touched.set(true);
                                    name.set(event.value().to_lowercase());
                                },
                            }
                        }
                    }
                    div { class: "field",
                        span { class: "mono-label", "Етап конвеєра" }
                        div { class: "segmented is-stages", role: "radiogroup", aria_label: "Етап конвеєра",
                            for (offset, stage_name) in STAGE_NAMES.iter().enumerate() {
                                {
                                    let value = FIRST_STAGE + offset as u8;
                                    rsx! {
                                        button {
                                            key: "{value}",
                                            class: if stage() == value { "segment is-active" } else { "segment" },
                                            r#type: "button",
                                            role: "radio",
                                            aria_checked: if stage() == value { "true" } else { "false" },
                                            onclick: move |_| stage.set(value),
                                            span { class: "segment-code", "{value:02}" }
                                            span { class: "segment-name", "{stage_name}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "field",
                        span { class: "mono-label", "Напрям" }
                        div { class: "segmented", role: "radiogroup", aria_label: "Напрям",
                            for option in SkillCategory::ALL {
                                button {
                                    key: "{option.code()}",
                                    class: if category() == option { "segment is-active" } else { "segment" },
                                    r#type: "button",
                                    role: "radio",
                                    aria_checked: if category() == option { "true" } else { "false" },
                                    onclick: move |_| category.set(option),
                                    span { class: "segment-name", "{category_label(option)}" }
                                }
                            }
                        }
                    }
                    div { class: "field",
                        div { class: "field-head",
                            label { class: "mono-label", r#for: "skill-summary", "Опис · коли застосовувати" }
                            span { class: "mono-label", "{summary_count} / {MAX_SKILL_SUMMARY_CHARS}" }
                        }
                        textarea {
                            class: "input is-area",
                            id: "skill-summary",
                            rows: "2",
                            placeholder: "Що робить навичка. Claude читає це, щоб вирішити, коли її застосувати.",
                            maxlength: "{MAX_SKILL_SUMMARY_CHARS}",
                            value: "{summary}",
                            oninput: move |event| summary.set(event.value()),
                        }
                    }
                    div { class: "field",
                        div { class: "field-head",
                            label { class: "mono-label", r#for: "skill-instructions", "Інструкції · Markdown" }
                            span { class: "mono-label", "{instructions_count} / {MAX_SKILL_INSTRUCTIONS_CHARS}" }
                        }
                        textarea {
                            class: "input is-area is-mono",
                            id: "skill-instructions",
                            rows: "10",
                            spellcheck: "false",
                            placeholder: "## Що видати\n- …\n\n## Правила\n- …",
                            maxlength: "{MAX_SKILL_INSTRUCTIONS_CHARS}",
                            value: "{instructions}",
                            oninput: move |event| instructions.set(event.value()),
                        }
                    }
                    if let Some(message) = error() {
                        p { class: "alert", role: "alert", "{message}" }
                    }
                }
                footer { class: "editor-foot",
                    if editing_id.is_some() {
                        button {
                            class: if confirming_delete() { "danger-button is-armed" } else { "danger-button" },
                            r#type: "button",
                            disabled: pending(),
                            onclick: remove,
                            onblur: move |_| confirming_delete.set(false),
                            TrashIcon {}
                            if confirming_delete() {
                                "Точно видалити?"
                            } else {
                                "Видалити"
                            }
                        }
                    }
                    div { class: "editor-foot-actions",
                        button {
                            class: "ghost-button",
                            r#type: "button",
                            disabled: pending(),
                            onclick: move |_| on_close.call(()),
                            "Скасувати"
                        }
                        button { class: "primary-button", r#type: "submit", disabled: pending(),
                            if pending() {
                                span { class: "spinner" }
                            }
                            "{action}"
                        }
                    }
                }
            }
        }
    }
}

fn category_label(category: SkillCategory) -> &'static str {
    match category {
        SkillCategory::GameDesign => "Дизайн гри",
        SkillCategory::Art => "Арт",
        SkillCategory::Code => "Код",
    }
}

fn upsert(skills: &mut Vec<Skill>, skill: Skill) {
    match skills.iter_mut().find(|existing| existing.id == skill.id) {
        Some(slot) => *slot = skill,
        None => skills.push(skill),
    }
    skills.sort_by_key(|skill| (skill.stage, skill.id));
}

fn set_enabled_locally(skills: &mut [Skill], id: i64, enabled: bool) {
    if let Some(skill) = skills.iter_mut().find(|skill| skill.id == id) {
        skill.enabled = enabled;
    }
}

fn explain(problem: &SkillError) -> String {
    match problem {
        SkillError::SignedOut => "Сесія завершилась — увійди знову.".to_owned(),
        SkillError::NotFound => "Цієї навички вже немає. Онови сторінку.".to_owned(),
        SkillError::NameInvalid => {
            "Ім’я для Claude — латинські малі літери, цифри й одинарні дефіси, без «claude» та «anthropic».".to_owned()
        }
        SkillError::NameTaken => "Навичка з таким ім’ям для Claude вже є.".to_owned(),
        SkillError::StageOutOfRange => {
            format!("Обери етап від {FIRST_STAGE:02} до {LAST_STAGE:02}.")
        }
        SkillError::TitleMissing => "Дай навичці назву.".to_owned(),
        SkillError::TitleTooLong => {
            format!("Назва задовга — максимум {MAX_SKILL_TITLE_CHARS} символів.")
        }
        SkillError::SummaryMissing => {
            "Опиши, що робить навичка — Claude читає опис, щоб вирішити, коли її застосувати.".to_owned()
        }
        SkillError::SummaryTooLong => {
            format!("Опис задовгий — максимум {MAX_SKILL_SUMMARY_CHARS} символів.")
        }
        SkillError::InstructionsTooLong => {
            format!("Інструкції задовгі — максимум {MAX_SKILL_INSTRUCTIONS_CHARS} символів.")
        }
        SkillError::Storage(detail) => {
            tracing::error!("skill storage failed: {detail}");
            "Не вдалося звернутися до сховища навичок. Спробуй ще раз.".to_owned()
        }
    }
}
