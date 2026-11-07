use dioxus::prelude::*;

#[component]
pub fn ScreenPreview(
    html: Option<String>,
    label: String,
    #[props(default)] src: Option<String>,
    #[props(default)] busy: bool,
    #[props(default)] live: bool,
) -> Element {
    rsx! {
        div { class: if live { "screen-preview is-live" } else { "screen-preview" },
            if let Some(address) = src {
                iframe {
                    class: "screen-frame",
                    title: "{label}",
                    tabindex: "-1",
                    "sandbox": "",
                    "scrolling": "no",
                    "loading": "lazy",
                    src: "{address}",
                }
            } else if let Some(document) = html {
                iframe {
                    class: "screen-frame",
                    title: "{label}",
                    tabindex: "-1",
                    "sandbox": "",
                    "scrolling": "no",
                    "srcdoc": "{document}",
                }
            } else {
                span { class: "screen-sketch", "[ЕСКІЗ:{label}]" }
            }
            if busy {
                span { class: "screen-busy",
                    span { class: "spinner" }
                }
            }
        }
    }
}
