use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use wasm_bindgen::JsCast;

use crate::sidebar_layout::capture_pointer;
use crate::studio::local_storage;

const WIDTH_KEY: &str = "noctics.inspector-width";
const WIDTH_PROPERTY: &str = "--nf-inspector-width";
const DEFAULT_WIDTH: f64 = 384.0;
const MIN_WIDTH: f64 = 320.0;
const MIN_CANVAS_WIDTH: f64 = 240.0;
const KEYBOARD_STEP: f64 = 24.0;

#[derive(Clone, Copy, PartialEq)]
struct Drag {
    origin_x: f64,
    origin_width: f64,
}

#[derive(Clone, Copy, PartialEq)]
pub struct InspectorLayout {
    width: Signal<f64>,
    settled: Signal<f64>,
    drag: Signal<Option<Drag>>,
}

impl InspectorLayout {
    pub fn settled_width(self) -> f64 {
        (self.settled)().min(viewport_width())
    }

    pub fn resizing(self) -> bool {
        self.drag.read().is_some()
    }

    fn begin_resize(mut self, pointer_x: f64) {
        self.drag.set(Some(Drag {
            origin_x: pointer_x,
            origin_width: *self.width.peek(),
        }));
    }

    fn resize_to(mut self, pointer_x: f64) {
        let Some(drag) = *self.drag.peek() else {
            return;
        };
        let wanted = clamp_width(drag.origin_width + drag.origin_x - pointer_x);
        if *self.width.peek() != wanted {
            self.width.set(wanted);
        }
    }

    fn end_resize(mut self) {
        if self.drag.write().take().is_some() {
            self.settle();
        }
    }

    fn nudge(mut self, delta: f64) {
        let wanted = clamp_width(*self.width.peek() + delta);
        self.width.set(wanted);
        self.settle();
    }

    fn reset(mut self) {
        self.width.set(clamp_width(DEFAULT_WIDTH));
        self.settle();
    }

    fn settle(mut self) {
        let width = *self.width.peek();
        if *self.settled.peek() != width {
            self.settled.set(width);
        }
        if let Some(storage) = local_storage() {
            let _ = storage.set_item(WIDTH_KEY, &width.to_string());
        }
    }
}

pub fn provide_inspector_layout() -> InspectorLayout {
    let layout = use_context_provider(|| {
        let width = load_width();
        InspectorLayout {
            width: Signal::new(width),
            settled: Signal::new(width),
            drag: Signal::new(None),
        }
    });
    use_effect(move || publish_width((layout.width)()));
    layout
}

pub fn use_inspector_layout() -> InspectorLayout {
    use_context::<InspectorLayout>()
}

#[component]
pub fn InspectorResizer() -> Element {
    let layout = use_inspector_layout();
    let resizing = layout.resizing();
    let width = (layout.width)();

    rsx! {
        div {
            class: if resizing { "nf-inspector-resizer is-active" } else { "nf-inspector-resizer" },
            role: "separator",
            tabindex: "0",
            aria_orientation: "vertical",
            aria_label: "Ширина панелі вузла",
            aria_valuemin: "{MIN_WIDTH}",
            aria_valuenow: "{width}",
            title: "Тягни, щоб змінити ширину · подвійний клік — стандартна ширина",
            onpointerdown: move |event| {
                if event.trigger_button() != Some(MouseButton::Primary) {
                    return;
                }
                event.prevent_default();
                capture_pointer(&event);
                layout.begin_resize(event.client_coordinates().x);
            },
            onpointermove: move |event| {
                if !layout.resizing() {
                    return;
                }
                if event.held_buttons().is_empty() {
                    layout.end_resize();
                } else {
                    layout.resize_to(event.client_coordinates().x);
                }
            },
            onpointerup: move |_| layout.end_resize(),
            onpointercancel: move |_| layout.end_resize(),
            onlostpointercapture: move |_| layout.end_resize(),
            ondoubleclick: move |_| layout.reset(),
            onkeydown: move |event| {
                let delta = match event.key() {
                    Key::ArrowLeft => KEYBOARD_STEP,
                    Key::ArrowRight => -KEYBOARD_STEP,
                    _ => return,
                };
                event.prevent_default();
                event.stop_propagation();
                layout.nudge(delta);
            },
        }
    }
}

fn viewport_width() -> f64 {
    web_sys::window()
        .and_then(|window| window.inner_width().ok())
        .and_then(|width| width.as_f64())
        .unwrap_or(f64::INFINITY)
}

fn clamp_width(wanted: f64) -> f64 {
    let ceiling = (viewport_width() - MIN_CANVAS_WIDTH).max(MIN_WIDTH);
    wanted.clamp(MIN_WIDTH, ceiling).round()
}

fn load_width() -> f64 {
    local_storage()
        .and_then(|storage| storage.get_item(WIDTH_KEY).ok().flatten())
        .and_then(|stored| stored.parse::<f64>().ok())
        .filter(|width| width.is_finite())
        .map_or(DEFAULT_WIDTH, clamp_width)
}

fn publish_width(width: f64) {
    let root = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
        .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok());
    if let Some(root) = root {
        let _ = root.style().set_property(WIDTH_PROPERTY, &format!("{width}px"));
    }
}
