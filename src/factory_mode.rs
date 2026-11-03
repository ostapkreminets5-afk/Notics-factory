use dioxus::prelude::*;

use crate::icons::{LayoutIcon, NodesIcon};
use crate::studio::local_storage;

const MODE_KEY: &str = "noctics.factory-mode";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FactoryMode {
    Classic,
    Nodes,
}

impl FactoryMode {
    const ALL: [Self; 2] = [Self::Classic, Self::Nodes];

    fn code(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Nodes => "nodes",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.code() == code)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Classic => "Класичний",
            Self::Nodes => "Вузли",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Classic => "Класичний режим: бічна панель і сторінки систем",
            Self::Nodes => "Режим вузлів: уся фабрика як граф, прогрес видно на кожному вузлі",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct Mode {
    current: Signal<FactoryMode>,
}

impl Mode {
    pub fn get(self) -> FactoryMode {
        (self.current)()
    }

    pub fn set(mut self, mode: FactoryMode) {
        self.current.set(mode);
        if let Some(storage) = local_storage() {
            let _ = storage.set_item(MODE_KEY, mode.code());
        }
    }
}

pub fn provide_factory_mode() -> Mode {
    use_context_provider(|| Mode {
        current: Signal::new(load_mode()),
    })
}

pub fn use_factory_mode() -> Mode {
    use_context::<Mode>()
}

fn load_mode() -> FactoryMode {
    local_storage()
        .and_then(|storage| storage.get_item(MODE_KEY).ok().flatten())
        .and_then(|code| FactoryMode::from_code(&code))
        .unwrap_or(FactoryMode::Classic)
}

#[component]
pub fn ModeSwitch() -> Element {
    let mode = use_factory_mode();
    let current = mode.get();

    rsx! {
        div { class: "mode-switch", role: "radiogroup", aria_label: "Режим фабрики",
            for choice in FactoryMode::ALL {
                button {
                    key: "{choice.code()}",
                    class: if choice == current { "mode-switch-option is-active" } else { "mode-switch-option" },
                    r#type: "button",
                    role: "radio",
                    aria_checked: if choice == current { "true" } else { "false" },
                    aria_label: choice.label(),
                    title: choice.hint(),
                    onclick: move |_| {
                        if choice != current {
                            mode.set(choice);
                        }
                    },
                    match choice {
                        FactoryMode::Classic => rsx! {
                            LayoutIcon {}
                        },
                        FactoryMode::Nodes => rsx! {
                            NodesIcon {}
                        },
                    }
                    span { class: "mode-switch-text", "{choice.label()}" }
                }
            }
        }
    }
}
