use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use crate::bridge;
use crate::studio::local_storage;

const MIN_ZOOM: f64 = 0.5;
const MAX_ZOOM: f64 = 2.0;
const ACTUAL_ZOOM: f64 = 1.0;
const PINCH_RATE: f64 = 0.01;
const STEP_LIMIT: f64 = 10.0;
const SNAP_RANGE: f64 = 0.03;
const PERCENT: f64 = 100.0;
const LINE_PIXELS: f64 = 16.0;
const PAGE_PIXELS: f64 = 400.0;
const SETTLE_MILLIS: i32 = 1400;
const STORAGE_KEY: &str = "nf-ui-zoom";
const SELF_ZOOMING_SURFACES: &str = ".nf-viewport";

struct PinchListener {
    window: web_sys::Window,
    handler: Closure<dyn FnMut(web_sys::WheelEvent)>,
}

impl PinchListener {
    fn listen(mut wanted: Signal<f64>, mut pulse: Signal<u64>) -> Option<Self> {
        let window = web_sys::window()?;
        let handler = Closure::<dyn FnMut(web_sys::WheelEvent)>::new(move |event: web_sys::WheelEvent| {
            if !(event.ctrl_key() || event.meta_key()) || event.default_prevented() || zooms_by_itself(&event) {
                return;
            }
            event.prevent_default();
            let next = (*wanted.peek() * pinch_factor(&event)).clamp(MIN_ZOOM, MAX_ZOOM);
            wanted.set(next);
            *pulse.write() += 1;
        });
        let options = web_sys::AddEventListenerOptions::new();
        options.set_passive(false);
        window
            .add_event_listener_with_callback_and_add_event_listener_options(
                "wheel",
                handler.as_ref().unchecked_ref(),
                &options,
            )
            .ok()?;
        Some(Self { window, handler })
    }
}

impl Drop for PinchListener {
    fn drop(&mut self) {
        let _ = self
            .window
            .remove_event_listener_with_callback("wheel", self.handler.as_ref().unchecked_ref());
    }
}

#[component]
pub fn UiZoom() -> Element {
    let mut wanted = use_signal(|| stored_zoom().unwrap_or(ACTUAL_ZOOM));
    let mut pulse = use_signal(|| 0_u64);
    let mut visible = use_signal(|| false);
    let mut hovered = use_signal(|| false);
    let level = use_memo(move || settle(wanted()));

    let listener = use_hook(|| {
        let listening = if bridge::desktop_shell_present() {
            PinchListener::listen(wanted, pulse)
        } else {
            None
        };
        Rc::new(RefCell::new(listening))
    });
    use_drop(move || {
        listener.borrow_mut().take();
    });

    use_effect(move || {
        let scale = level();
        store_zoom(scale);
        spawn(async move {
            let _ = bridge::set_ui_zoom(scale).await;
        });
    });

    use_effect(move || {
        let seen = pulse();
        if seen == 0 {
            return;
        }
        visible.set(true);
        spawn(async move {
            pause(SETTLE_MILLIS).await;
            if *pulse.peek() == seen && !*hovered.peek() {
                visible.set(false);
            }
        });
    });

    let current = level();
    let shown = visible();
    let percent = (current * PERCENT).round();
    let fill = ((current - MIN_ZOOM) / (MAX_ZOOM - MIN_ZOOM) * PERCENT).clamp(0.0, PERCENT);
    let at_actual = current == ACTUAL_ZOOM;

    rsx! {
        div {
            class: if shown { "nf-ui-zoom is-visible" } else { "nf-ui-zoom" },
            role: "status",
            aria_live: "polite",
            aria_hidden: if shown { "false" } else { "true" },
            onmouseenter: move |_| hovered.set(true),
            onmouseleave: move |_| {
                hovered.set(false);
                *pulse.write() += 1;
            },
            span { class: "nf-ui-zoom-label", "Масштаб" }
            span { class: "nf-ui-zoom-track",
                span { class: "nf-ui-zoom-fill", style: "width: {fill}%;" }
            }
            span { class: "nf-ui-zoom-value", "{percent} %" }
            button {
                class: "nf-ui-zoom-reset",
                r#type: "button",
                title: "Повернути масштаб 100 %",
                tabindex: if shown { "0" } else { "-1" },
                disabled: at_actual,
                onclick: move |_| {
                    wanted.set(ACTUAL_ZOOM);
                    *pulse.write() += 1;
                },
                "100 %"
            }
        }
    }
}

fn settle(scale: f64) -> f64 {
    if (scale - ACTUAL_ZOOM).abs() < SNAP_RANGE {
        return ACTUAL_ZOOM;
    }
    (scale * PERCENT).round() / PERCENT
}

fn pinch_factor(event: &web_sys::WheelEvent) -> f64 {
    let pixels = match event.delta_mode() {
        web_sys::WheelEvent::DOM_DELTA_LINE => event.delta_y() * LINE_PIXELS,
        web_sys::WheelEvent::DOM_DELTA_PAGE => event.delta_y() * PAGE_PIXELS,
        _ => event.delta_y(),
    };
    (-pixels.clamp(-STEP_LIMIT, STEP_LIMIT) * PINCH_RATE).exp()
}

fn zooms_by_itself(event: &web_sys::WheelEvent) -> bool {
    event
        .target()
        .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
        .and_then(|element| element.closest(SELF_ZOOMING_SURFACES).ok().flatten())
        .is_some()
}

fn stored_zoom() -> Option<f64> {
    local_storage()?
        .get_item(STORAGE_KEY)
        .ok()
        .flatten()?
        .parse::<f64>()
        .ok()
        .filter(|scale| scale.is_finite())
        .map(|scale| scale.clamp(MIN_ZOOM, MAX_ZOOM))
}

fn store_zoom(scale: f64) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(STORAGE_KEY, &scale.to_string());
    }
}

async fn pause(millis: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, millis);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
