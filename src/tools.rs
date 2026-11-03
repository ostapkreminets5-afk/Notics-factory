use dioxus::prelude::*;
use dioxus_logger::tracing;
use noctics_shared::{
    MAX_TOOL_NAME_CHARS, MAX_TOOL_NOTE_CHARS, MAX_TOOL_URL_CHARS, Tool, ToolDraft, ToolError,
    ToolSlot, Toolbox,
};

use wasm_bindgen::JsCast;

use crate::bridge;
use crate::build::PlanCard;
use crate::comfy::{ModelRoster, use_comfy};
use crate::icons::{ArrowRightIcon, CheckIcon, ExternalIcon, TrashIcon};
use crate::studio::{Section, use_studio};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PickerTab {
    Models,
    Tools,
}

const PICKER_MARGIN: f64 = 8.0;
const DEFAULT_VIEWPORT_HEIGHT: f64 = 800.0;
const CLAUDE_TOOL: &str = "claude";
const COMFY_TOOL: &str = "comfyui";
const CLAUDE_DESIGN_TOOL: &str = "claude-design";
const BUILD_TOOL: &str = "build";

#[derive(Clone, Copy, PartialEq)]
pub struct Tools {
    pub toolbox: Signal<Toolbox>,
}

impl Tools {
    pub fn active(self, slot: ToolSlot) -> Option<Tool> {
        self.toolbox.read().active_tool(slot).cloned()
    }

    pub fn runs_builtin(self, slot: ToolSlot) -> bool {
        let builtin = match slot {
            ToolSlot::Core => CLAUDE_TOOL,
            ToolSlot::Interface => CLAUDE_DESIGN_TOOL,
            ToolSlot::Build => BUILD_TOOL,
            _ => return false,
        };
        self.active(slot).is_none_or(|tool| tool.id == builtin)
    }

    pub fn runs_comfy(self) -> bool {
        self.active(ToolSlot::Images)
            .is_some_and(|tool| tool.id == COMFY_TOOL)
    }
}

pub fn provide_tools() -> Tools {
    let tools = use_context_provider(|| Tools {
        toolbox: Signal::new(Toolbox::default()),
    });
    let mut toolbox = tools.toolbox;
    use_future(move || async move {
        if let Ok(loaded) = bridge::toolbox().await {
            toolbox.set(loaded);
        }
    });
    tools
}

pub fn use_tools() -> Tools {
    use_context::<Tools>()
}

pub fn slot_for(section: Section) -> Option<ToolSlot> {
    match section {
        Section::Core => Some(ToolSlot::Core),
        Section::ComfyUi => Some(ToolSlot::Images),
        Section::ClaudeDesign => Some(ToolSlot::Interface),
        Section::Pinterest => Some(ToolSlot::References),
        Section::Models => Some(ToolSlot::Models),
        Section::Build => Some(ToolSlot::Build),
        _ => None,
    }
}

pub fn picker_placement(event: &MouseEvent) -> String {
    let viewport = web_sys::window()
        .and_then(|window| window.inner_height().ok())
        .and_then(|height| height.as_f64())
        .unwrap_or(DEFAULT_VIEWPORT_HEIGHT);
    let row = event
        .data()
        .downcast::<web_sys::MouseEvent>()
        .and_then(|raw| raw.target())
        .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
        .and_then(|element| element.closest(".nav-row").ok().flatten())
        .map(|row| row.get_bounding_client_rect());
    let (top, bottom) = row.map_or_else(
        || {
            let y = event.client_coordinates().y;
            (y, y)
        },
        |rect| (rect.top(), rect.bottom()),
    );
    if top < viewport / 2.0 {
        format!("top:{}px", top.max(PICKER_MARGIN))
    } else {
        format!("bottom:{}px", (viewport - bottom).max(PICKER_MARGIN))
    }
}
#[component]
pub fn ToolPicker(slot: ToolSlot, placement: String, on_close: EventHandler<()>) -> Element {
    let tools = use_tools();
    let mut toolbox = tools.toolbox;
    let mut adding = use_signal(|| false);
    let mut name = use_signal(String::new);
    let mut url = use_signal(String::new);
    let mut note = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut pending = use_signal(|| false);
    let mut tab = use_signal(|| PickerTab::Models);
    let comfy = use_comfy();
    let studio = use_studio();

    let snapshot = toolbox.read().clone();
    let active_id = snapshot.active_tool(slot).map(|tool| tool.id.clone());
    let options: Vec<Tool> = snapshot.tools_for(slot).cloned().collect();
    let tool_count = options.len();
    let comfy_slot = slot == ToolSlot::Images && active_id.as_deref() == Some(COMFY_TOOL);
    let (models_on, models_total) = comfy.overview.read().as_ref().map_or((0, 0), |overview| {
        (
            overview.models.iter().filter(|model| model.usable()).count(),
            overview.models.len(),
        )
    });
    let showing_models = comfy_slot && tab() == PickerTab::Models;
    let picker_class = if comfy_slot { "tool-picker has-tabs" } else { "tool-picker" };

    let choose = move |tool_id: String| {
        spawn(async move {
            match bridge::choose_tool(slot, &tool_id).await {
                Ok(updated) => {
                    toolbox.set(updated);
                    on_close.call(());
                }
                Err(problem) => error.set(Some(explain_tool(&problem))),
            }
        });
    };

    let submit = move |event: FormEvent| {
        event.prevent_default();
        if pending() {
            return;
        }
        let draft = ToolDraft {
            slot,
            name: name(),
            note: note(),
            url: url(),
        };
        if let Err(problem) = draft.normalized() {
            error.set(Some(explain_tool(&problem)));
            return;
        }
        pending.set(true);
        error.set(None);
        spawn(async move {
            match bridge::add_tool(&draft).await {
                Ok(updated) => {
                    toolbox.set(updated);
                    on_close.call(());
                }
                Err(problem) => {
                    error.set(Some(explain_tool(&problem)));
                    pending.set(false);
                }
            }
        });
    };

    rsx! {
        div { class: "tool-layer", onclick: move |_| on_close.call(()) }
        div {
            class: picker_class,
            role: "dialog",
            aria_label: "Інструмент для «{slot.purpose()}»",
            style: "{placement}",
            onclick: move |event| event.stop_propagation(),
            onkeydown: move |event| {
                if event.key() == Key::Escape {
                    on_close.call(());
                }
            },
            if comfy_slot {
                div {
                    class: "picker-tabs",
                    role: "tablist",
                    aria_label: "ComfyUI",
                    button {
                        class: if showing_models { "picker-tab is-active" } else { "picker-tab" },
                        r#type: "button",
                        role: "tab",
                        aria_selected: if showing_models { "true" } else { "false" },
                        onclick: move |_| tab.set(PickerTab::Models),
                        "Моделі"
                        span { class: "picker-tab-count", "{models_on}/{models_total}" }
                    }
                    button {
                        class: if showing_models { "picker-tab" } else { "picker-tab is-active" },
                        r#type: "button",
                        role: "tab",
                        aria_selected: if showing_models { "false" } else { "true" },
                        onclick: move |_| tab.set(PickerTab::Tools),
                        "Інструменти"
                        span { class: "picker-tab-count", "{tool_count}" }
                    }
                }
            } else {
                p { class: "mono-label tool-picker-label", "Інструмент · {slot.purpose()}" }
            }
            if showing_models {
                div { class: "picker-models", role: "tabpanel",
                    ModelRoster { compact: true }
                }
                button {
                    class: "picker-link",
                    r#type: "button",
                    onclick: move |_| {
                        studio.open(Section::ComfyUi);
                        on_close.call(());
                    },
                    "Відкрити галерею ComfyUI"
                    ArrowRightIcon {}
                }
            } else {
                div {
                    class: "tool-list",
                    role: "radiogroup",
                    aria_label: "{slot.purpose()}",
                    for tool in options {
                        {
                            let active = active_id.as_deref() == Some(tool.id.as_str());
                            let chosen = tool.id.clone();
                            let opened = tool.id.clone();
                            let removed = tool.id.clone();
                            rsx! {
                                div { key: "{tool.id}", class: if active { "tool-row is-active" } else { "tool-row" },
                                    button {
                                        class: "tool-option",
                                        r#type: "button",
                                        role: "radio",
                                        aria_checked: if active { "true" } else { "false" },
                                        onclick: move |_| choose(chosen.clone()),
                                        span { class: "tool-check",
                                            if active {
                                                CheckIcon {}
                                            }
                                        }
                                        span { class: "tool-text",
                                            span { class: "tool-name", "{tool.name}" }
                                            if !tool.note.is_empty() {
                                                span { class: "tool-note", "{tool.note}" }
                                            }
                                        }
                                        if tool.integrated {
                                            span { class: "tool-tag is-live", "Підключено" }
                                        } else if tool.custom {
                                            span { class: "tool-tag", "Свій" }
                                        }
                                    }
                                    if tool.url.is_some() {
                                        button {
                                            class: "icon-button",
                                            r#type: "button",
                                            title: "Відкрити {tool.name}",
                                            aria_label: "Відкрити {tool.name}",
                                            onclick: move |_| {
                                                let id = opened.clone();
                                                spawn(async move {
                                                    if let Err(problem) = bridge::open_tool(&id).await {
                                                        error.set(Some(explain_tool(&problem)));
                                                    }
                                                });
                                            },
                                            ExternalIcon {}
                                        }
                                    }
                                    if tool.custom {
                                        button {
                                            class: "icon-button is-danger",
                                            r#type: "button",
                                            title: "Прибрати {tool.name}",
                                            aria_label: "Прибрати {tool.name}",
                                            onclick: move |_| {
                                                let id = removed.clone();
                                                spawn(async move {
                                                    match bridge::remove_tool(&id).await {
                                                        Ok(updated) => toolbox.set(updated),
                                                        Err(problem) => error.set(Some(explain_tool(&problem))),
                                                    }
                                                });
                                            },
                                            TrashIcon {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(message) = error() {
                    p { class: "tool-error", role: "alert", "{message}" }
                }
                if adding() {
                    form {
                        class: "tool-form",
                        novalidate: true,
                        onsubmit: submit,
                        input {
                            class: "input",
                            r#type: "text",
                            aria_label: "Назва інструмента",
                            placeholder: "Назва, напр. Automatic1111",
                            maxlength: "{MAX_TOOL_NAME_CHARS}",
                            value: "{name}",
                            oninput: move |event| name.set(event.value()),
                            onmounted: move |event| async move {
                                let _ = event.data().set_focus(true).await;
                            },
                        }
                        input {
                            class: "input is-mono",
                            r#type: "url",
                            aria_label: "Посилання",
                            placeholder: "https://… (необов’язково)",
                            maxlength: "{MAX_TOOL_URL_CHARS}",
                            spellcheck: "false",
                            value: "{url}",
                            oninput: move |event| url.set(event.value()),
                        }
                        input {
                            class: "input",
                            r#type: "text",
                            aria_label: "Для чого",
                            placeholder: "Для чого (необов’язково)",
                            maxlength: "{MAX_TOOL_NOTE_CHARS}",
                            value: "{note}",
                            oninput: move |event| note.set(event.value()),
                        }
                        div { class: "tool-form-actions",
                            button {
                                class: "ghost-button is-small",
                                r#type: "button",
                                onclick: move |_| {
                                    adding.set(false);
                                    error.set(None);
                                },
                                "Скасувати"
                            }
                            button {
                                class: "primary-button is-small",
                                r#type: "submit",
                                disabled: pending() || name.read().trim().is_empty(),
                                "Додати й вибрати"
                            }
                        }
                    }
                } else {
                    button {
                        class: "tool-add",
                        r#type: "button",
                        onclick: move |_| adding.set(true),
                        "+ Додати інструмент"
                    }
                }
            }
        }
    }
}

#[component]
pub fn ToolPage(slot: ToolSlot) -> Element {
    let tools = use_tools();
    let mut toolbox = tools.toolbox;
    let mut notice = use_signal(|| None::<String>);
    let Some(tool) = tools.active(slot) else {
        return rsx! {};
    };
    let tool_id = tool.id.clone();
    let has_link = tool.url.is_some();

    let open = move |_| {
        let id = tool_id.clone();
        spawn(async move {
            if let Err(problem) = bridge::open_tool(&id).await {
                notice.set(Some(explain_tool(&problem)));
            }
        });
    };

    let restore = move |builtin: &'static str| {
        spawn(async move {
            match bridge::choose_tool(slot, builtin).await {
                Ok(updated) => toolbox.set(updated),
                Err(problem) => notice.set(Some(explain_tool(&problem))),
            }
        });
    };

    let builtin = match slot {
        ToolSlot::Core => Some((CLAUDE_TOOL, "AI-ядро")),
        ToolSlot::Interface => Some((CLAUDE_DESIGN_TOOL, "Claude Design")),
        ToolSlot::Build => Some((BUILD_TOOL, "Збірку")),
        _ => None,
    };

    rsx! {
        section { class: "page",
            header { class: "page-head has-action",
                div { class: "page-head-text",
                    p { class: "mono-label", "Система · {slot.purpose()}" }
                    h1 { class: "page-title", "{tool.name}" }
                    p { class: "page-lead",
                        if tool.note.is_empty() {
                            "{slot.purpose()}"
                        } else {
                            "{tool.note}"
                        }
                    }
                }
                if has_link {
                    div { class: "head-actions",
                        button {
                            class: "primary-button",
                            r#type: "button",
                            onclick: open,
                            "Відкрити {tool.name}"
                            ExternalIcon {}
                        }
                    }
                }
            }
            if let Some(message) = notice() {
                div { class: "page-alert", role: "alert",
                    span { "{message}" }
                }
            }
            div { class: "tool-hero",
                p { class: "mono-label",
                    if tool.custom {
                        "Свій інструмент"
                    } else {
                        "Інструмент вибрано"
                    }
                }
                p { class: "tool-hero-text",
                    "Вкладка «{slot.purpose()}» зараз працює з {tool.name}. "
                    "Змінити інструмент або додати свій можна кнопкою ⇅ біля вкладки в лівій панелі."
                }
                if !tool.integrated {
                    p { class: "tool-hero-hint",
                        "Конвеєр поки автоматично працює лише з AI-ядром (Claude) і Claude Design, тож цей інструмент відкривається за посиланням."
                    }
                }
                if let Some((builtin_id, builtin_name)) = builtin {
                    button {
                        class: "ghost-button is-start",
                        r#type: "button",
                        onclick: move |_| restore(builtin_id),
                        "Повернути {builtin_name}"
                    }
                }
            }
            if slot == ToolSlot::Build {
                div { class: "build-stack",
                    PlanCard {}
                }
            }
        }
    }
}

pub fn explain_tool(problem: &ToolError) -> String {
    match problem {
        ToolError::SignedOut => "Сесія завершилась — увійди знову.".to_owned(),
        ToolError::NotFound => "Такого інструмента вже немає.".to_owned(),
        ToolError::BuiltIn => "Вбудовані інструменти прибрати не можна.".to_owned(),
        ToolError::NameMissing => "Вкажи назву інструмента.".to_owned(),
        ToolError::NameTooLong => format!("Назва задовга — максимум {MAX_TOOL_NAME_CHARS} символів."),
        ToolError::NoteTooLong => format!("Опис задовгий — максимум {MAX_TOOL_NOTE_CHARS} символів."),
        ToolError::InvalidUrl => "Посилання має починатися з http:// або https://.".to_owned(),
        ToolError::NoUrl => "У цього інструмента немає посилання.".to_owned(),
        ToolError::Storage(detail) => {
            tracing::error!("toolbox failed: {detail}");
            "Не вдалося зберегти вибір інструмента.".to_owned()
        }
    }
}
