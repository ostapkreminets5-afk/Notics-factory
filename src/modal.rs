use dioxus::prelude::*;

#[component]
pub fn Modal(
    label: String,
    #[props(default)] wide: bool,
    #[props(default)] locked: bool,
    on_close: EventHandler<()>,
    children: Element,
) -> Element {
    let mut backdrop_pressed = use_signal(|| false);

    rsx! {
        div {
            class: "editor-backdrop",
            onmousedown: move |_| backdrop_pressed.set(true),
            onclick: move |_| {
                if backdrop_pressed() && !locked {
                    on_close.call(());
                }
            },
            onkeydown: move |event| {
                if event.key() == Key::Escape && !locked {
                    on_close.call(());
                }
            },
            div {
                class: if wide { "editor is-wide" } else { "editor" },
                role: "dialog",
                aria_modal: "true",
                aria_label: "{label}",
                onmousedown: move |event| {
                    event.stop_propagation();
                    backdrop_pressed.set(false);
                },
                onclick: move |event| event.stop_propagation(),
                {children}
            }
        }
    }
}
