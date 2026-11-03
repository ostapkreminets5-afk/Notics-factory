use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use wasm_bindgen::JsCast;

use crate::studio::local_storage;

const WIDTH_KEY: &str = "noctics.sidebar-width";
const COLLAPSED_KEY: &str = "noctics.sidebar-collapsed";
const WIDTH_PROPERTY: &str = "--sidebar-width";
const STORED_TRUE: &str = "1";
const STORED_FALSE: &str = "0";

const DEFAULT_WIDTH: f64 = 218.0;
const MIN_WIDTH: f64 = 200.0;
const MAX_WIDTH: f64 = 460.0;
const RAIL_WIDTH: f64 = 64.0;
const COLLAPSE_BELOW: f64 = 140.0;
const MIN_VIEW_WIDTH: f64 = 360.0;
const KEYBOARD_STEP: f64 = 16.0;

#[derive(Clone, Copy, PartialEq)]
struct Drag {
    origin_x: f64,
    origin_width: f64,
}

#[derive(Clone, Copy, PartialEq)]
pub struct SidebarLayout {
    width: Signal<f64>,
    collapsed: Signal<bool>,
    drag: Signal<Option<Drag>>,
}

impl SidebarLayout {
    pub fn collapsed(self) -> bool {
        (self.collapsed)()
    }

    pub fn resizing(self) -> bool {
        self.drag.read().is_some()
    }

    pub fn shown_width(self) -> f64 {
        if self.collapsed() {
            RAIL_WIDTH
        } else {
            (self.width)()
        }
    }

    pub fn set_collapsed(mut self, collapsed: bool) {
        self.collapsed.set(collapsed);
        self.save();
    }

    pub fn toggle(self) {
        self.set_collapsed(!self.collapsed());
    }

    pub fn reset(mut self) {
        self.width.set(DEFAULT_WIDTH);
        self.set_collapsed(false);
    }

    fn begin_resize(mut self, pointer_x: f64) {
        self.drag.set(Some(Drag {
            origin_x: pointer_x,
            origin_width: self.shown_width(),
        }));
    }

    fn resize_to(mut self, pointer_x: f64) {
        let Some(drag) = *self.drag.peek() else {
            return;
        };
        let wanted = drag.origin_width + pointer_x - drag.origin_x;
        let collapse = wanted < COLLAPSE_BELOW;
        if *self.collapsed.peek() != collapse {
            self.collapsed.set(collapse);
        }
        if !collapse {
            self.width.set(clamp_width(wanted));
        }
    }

    fn end_resize(mut self) {
        if self.drag.write().take().is_some() {
            self.save();
        }
    }

    fn nudge(mut self, delta: f64) {
        let width = (self.width)();
        match (self.collapsed(), delta > 0.0) {
            (true, false) => return,
            (true, true) => self.collapsed.set(false),
            (false, false) if width <= MIN_WIDTH => self.collapsed.set(true),
            (false, _) => self.width.set(clamp_width(width + delta)),
        }
        self.save();
    }

    fn save(self) {
        let Some(storage) = local_storage() else {
            return;
        };
        let collapsed = if *self.collapsed.peek() { STORED_TRUE } else { STORED_FALSE };
        let _ = storage.set_item(WIDTH_KEY, &self.width.peek().to_string());
        let _ = storage.set_item(COLLAPSED_KEY, collapsed);
    }
}

pub fn provide_sidebar_layout() -> SidebarLayout {
    let layout = use_context_provider(|| SidebarLayout {
        width: Signal::new(load_width()),
        collapsed: Signal::new(load_collapsed()),
        drag: Signal::new(None),
    });
    use_effect(move || publish_width(layout.shown_width()));
    layout
}

pub fn use_sidebar_layout() -> SidebarLayout {
    use_context::<SidebarLayout>()
}

#[component]
pub fn SidebarResizer() -> Element {
    let layout = use_sidebar_layout();
    let resizing = layout.resizing();
    let width = layout.shown_width();

    rsx! {
        div {
            class: if resizing { "sidebar-resizer is-active" } else { "sidebar-resizer" },
            role: "separator",
            tabindex: "0",
            aria_orientation: "vertical",
            aria_label: "Ширина бічної панелі",
            aria_valuemin: "{RAIL_WIDTH}",
            aria_valuemax: "{MAX_WIDTH}",
            aria_valuenow: "{width}",
            title: "Тягни, щоб змінити ширину · подвійний клік — стандартна ширина",
            onpointerdown: move |event| {
                if event.trigger_button() != Some(MouseButton::Primary) {
                    return;
                }
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
                    Key::ArrowLeft => -KEYBOARD_STEP,
                    Key::ArrowRight => KEYBOARD_STEP,
                    Key::Enter => {
                        event.prevent_default();
                        layout.toggle();
                        return;
                    }
                    _ => return,
                };
                event.prevent_default();
                layout.nudge(delta);
            },
        }
    }
}

fn capture_pointer(event: &PointerEvent) {
    let pointer = event.data().pointer_id();
    let handle = event
        .data()
        .downcast::<web_sys::PointerEvent>()
        .and_then(|raw| raw.current_target())
        .and_then(|target| target.dyn_into::<web_sys::Element>().ok());
    if let Some(handle) = handle {
        let _ = handle.set_pointer_capture(pointer);
    }
}

fn clamp_width(wanted: f64) -> f64 {
    let viewport = web_sys::window()
        .and_then(|window| window.inner_width().ok())
        .and_then(|width| width.as_f64())
        .unwrap_or(f64::INFINITY);
    let ceiling = MAX_WIDTH.min(viewport - MIN_VIEW_WIDTH).max(MIN_WIDTH);
    wanted.clamp(MIN_WIDTH, ceiling).round()
}

fn load_width() -> f64 {
    local_storage()
        .and_then(|storage| storage.get_item(WIDTH_KEY).ok().flatten())
        .and_then(|stored| stored.parse::<f64>().ok())
        .filter(|width| width.is_finite())
        .map_or(DEFAULT_WIDTH, |width| width.clamp(MIN_WIDTH, MAX_WIDTH))
}

fn load_collapsed() -> bool {
    local_storage()
        .and_then(|storage| storage.get_item(COLLAPSED_KEY).ok().flatten())
        .is_some_and(|stored| stored == STORED_TRUE)
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
