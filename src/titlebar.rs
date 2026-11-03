use dioxus::prelude::*;

use crate::bridge;
use crate::icons::{CloseGlyph, LogoMark, MaximizeGlyph, MinimizeGlyph, RestoreGlyph};

#[component]
pub fn Titlebar(overlay: bool) -> Element {
    let mut maximized = use_signal(|| false);

    rsx! {
        header {
            class: if overlay { "titlebar is-overlay" } else { "titlebar" },
            "data-tauri-drag-region": "true",
            onresize: move |_| async move {
                if let Some(state) = bridge::window_is_maximized().await {
                    maximized.set(state);
                }
            },
            if !overlay {
                div { class: "brand",
                    LogoMark {}
                    span { class: "wordmark", "NOCTICS" }
                    span { class: "brand-divider" }
                    span { class: "brand-sub", "factory" }
                }
            }
            div { class: "window-controls",
                button {
                    class: "window-button",
                    r#type: "button",
                    tabindex: "-1",
                    aria_label: "Згорнути",
                    onclick: move |_| bridge::minimize_window(),
                    MinimizeGlyph {}
                }
                button {
                    class: "window-button",
                    r#type: "button",
                    tabindex: "-1",
                    aria_label: if maximized() { "Відновити" } else { "Розгорнути" },
                    onclick: move |_| async move {
                        if let Some(state) = bridge::toggle_maximize_window().await {
                            maximized.set(state);
                        }
                    },
                    if maximized() {
                        RestoreGlyph {}
                    } else {
                        MaximizeGlyph {}
                    }
                }
                button {
                    class: "window-button is-close",
                    r#type: "button",
                    tabindex: "-1",
                    aria_label: "Закрити",
                    onclick: move |_| bridge::close_window(),
                    CloseGlyph {}
                }
            }
        }
    }
}
