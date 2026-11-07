use dioxus::html::geometry::WheelDelta;
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use noctics_shared::Screen;

use crate::icons::{ChevronLeftIcon, ChevronRightIcon, CloseIcon};
use crate::node_graph::screen_phase;
use crate::sidebar_layout::capture_pointer;
use crate::studio::screen_status_label;

const SWIPE_DISTANCE: f64 = 60.0;
const WHEEL_DISTANCE: f64 = 60.0;
const GESTURE_GAP_MILLIS: f64 = 240.0;
const LINE_PIXELS: f64 = 16.0;
const PAGE_PIXELS: f64 = 400.0;

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Previous,
    Next,
}

impl Step {
    fn class(self) -> &'static str {
        match self {
            Self::Previous => "is-previous",
            Self::Next => "is-next",
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct Slide {
    pub id: i64,
    pub title: String,
    pub summary: String,
    pub phase: &'static str,
    pub status: &'static str,
    pub html: Option<String>,
    pub src: Option<String>,
}

impl Slide {
    pub fn from_screen(screen: &Screen) -> Self {
        Self {
            id: screen.id,
            title: screen.title.clone(),
            summary: screen.summary.clone(),
            phase: screen_phase(screen.status).code(),
            status: screen_status_label(screen.status),
            html: screen.html.clone(),
            src: None,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct WheelGesture {
    travel: f64,
    last_at: f64,
    spent: bool,
}

impl WheelGesture {
    fn feed(&mut self, amount: f64, now: f64) -> Option<Step> {
        if now - self.last_at > GESTURE_GAP_MILLIS {
            *self = Self::default();
        }
        self.last_at = now;
        if self.spent {
            return None;
        }
        self.travel += amount;
        if self.travel.abs() < WHEEL_DISTANCE {
            return None;
        }
        self.spent = true;
        Some(if self.travel > 0.0 { Step::Next } else { Step::Previous })
    }
}

#[component]
pub fn ScreenGallery(
    slides: Vec<Slide>,
    current: i64,
    on_pick: EventHandler<i64>,
    on_close: EventHandler<()>,
) -> Element {
    let mut arrival = use_signal(|| None::<Step>);
    let mut gesture = use_signal(WheelGesture::default);

    let count = slides.len();
    let index = slides.iter().position(|slide| slide.id == current).unwrap_or(0);
    let Some(screen) = slides.get(index).cloned() else {
        return rsx! {};
    };
    let alone = count < 2;
    let previous = slides[(index + count - 1) % count].id;
    let next = slides[(index + 1) % count].id;
    let position = index + 1;
    let phase = screen.phase;
    let status = screen.status;
    let entering = arrival();

    let mut turn = move |step: Step| {
        if alone {
            return;
        }
        let target = match step {
            Step::Previous => previous,
            Step::Next => next,
        };
        arrival.set(Some(step));
        on_pick.call(target);
    };

    rsx! {
        div {
            class: "nf-viewer",
            role: "dialog",
            aria_modal: "true",
            aria_label: "Перегляд екранів гри",
            tabindex: "-1",
            onmounted: move |event| async move {
                let _ = event.data().set_focus(true).await;
            },
            onkeydown: move |event| {
                match event.key() {
                    Key::Escape => on_close.call(()),
                    Key::ArrowLeft => turn(Step::Previous),
                    Key::ArrowRight => turn(Step::Next),
                    _ => return,
                }
                event.prevent_default();
                event.stop_propagation();
            },
            onwheel: move |event| {
                let modifiers = event.modifiers();
                if modifiers.ctrl() || modifiers.meta() {
                    return;
                }
                let (across, down) = wheel_pixels(event.delta());
                let amount = if across.abs() >= down.abs() { across } else { down };
                let stepped = gesture.write().feed(amount, js_sys::Date::now());
                if let Some(step) = stepped {
                    turn(step);
                }
            },
            div { class: "nf-viewer-backdrop", onclick: move |_| on_close.call(()) }
            header { class: "nf-viewer-head",
                span { class: "nf-led is-{phase}" }
                h2 { class: "nf-viewer-title", title: "{screen.title}", "{screen.title}" }
                span { class: "nf-viewer-state", "{status}" }
                span { class: "nf-viewer-count", "{position} / {count}" }
                button {
                    class: "nf-inspector-close",
                    r#type: "button",
                    title: "Закрити (Esc)",
                    aria_label: "Закрити перегляд",
                    onclick: move |_| on_close.call(()),
                    CloseIcon {}
                }
            }
            div { class: "nf-viewer-stage",
                button {
                    class: "nf-viewer-step",
                    r#type: "button",
                    title: "Попередній екран (←)",
                    aria_label: "Попередній екран",
                    disabled: alone,
                    onclick: move |_| turn(Step::Previous),
                    ChevronLeftIcon {}
                }
                div { class: "nf-viewer-slot",
                    for shown in std::iter::once(screen.clone()) {
                        ViewerScreen {
                            key: "{shown.id}",
                            screen: shown,
                            arrival: entering,
                            on_swipe: move |step: Step| turn(step),
                        }
                    }
                }
                button {
                    class: "nf-viewer-step",
                    r#type: "button",
                    title: "Наступний екран (→)",
                    aria_label: "Наступний екран",
                    disabled: alone,
                    onclick: move |_| turn(Step::Next),
                    ChevronRightIcon {}
                }
            }
            footer { class: "nf-viewer-foot",
                if !screen.summary.is_empty() {
                    p { class: "nf-viewer-note", "{screen.summary}" }
                }
                if count > 1 {
                    nav { class: "nf-viewer-strip", aria_label: "Усі екрани",
                        for (at, item) in slides.iter().enumerate() {
                            button {
                                key: "{item.id}",
                                class: if at == index { "nf-viewer-chip is-current" } else { "nf-viewer-chip" },
                                r#type: "button",
                                aria_current: if at == index { "true" } else { "false" },
                                onclick: {
                                    let id = item.id;
                                    let step = if at < index { Step::Previous } else { Step::Next };
                                    move |_| {
                                        if id != current {
                                            arrival.set(Some(step));
                                            on_pick.call(id);
                                        }
                                    }
                                },
                                span { class: "nf-led is-{item.phase}" }
                                "{item.title}"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ViewerScreen(screen: Slide, arrival: Option<Step>, on_swipe: EventHandler<Step>) -> Element {
    let mut origin = use_signal(|| None::<f64>);
    let class = arrival.map_or_else(
        || "nf-viewer-screen".to_owned(),
        |step| format!("nf-viewer-screen {}", step.class()),
    );

    rsx! {
        div {
            class: "{class}",
            onpointerdown: move |event| {
                if event.trigger_button() != Some(MouseButton::Primary) {
                    return;
                }
                capture_pointer(&event);
                origin.set(Some(event.client_coordinates().x));
            },
            onpointerup: move |event| {
                let Some(start) = origin.write().take() else {
                    return;
                };
                let travel = event.client_coordinates().x - start;
                if travel <= -SWIPE_DISTANCE {
                    on_swipe.call(Step::Next);
                } else if travel >= SWIPE_DISTANCE {
                    on_swipe.call(Step::Previous);
                }
            },
            onpointercancel: move |_| origin.set(None),
            if let Some(html) = &screen.html {
                iframe {
                    title: "{screen.title}",
                    tabindex: "-1",
                    "sandbox": "",
                    "scrolling": "no",
                    "srcdoc": "{html}",
                }
            } else if let Some(address) = &screen.src {
                iframe {
                    title: "{screen.title}",
                    tabindex: "-1",
                    "sandbox": "",
                    "scrolling": "no",
                    src: "{address}",
                }
            } else {
                p { class: "nf-viewer-empty", "Цей екран ще не намальовано" }
            }
        }
    }
}

fn wheel_pixels(delta: WheelDelta) -> (f64, f64) {
    match delta {
        WheelDelta::Pixels(amount) => (amount.x, amount.y),
        WheelDelta::Lines(amount) => (amount.x * LINE_PIXELS, amount.y * LINE_PIXELS),
        WheelDelta::Pages(amount) => (amount.x * PAGE_PIXELS, amount.y * PAGE_PIXELS),
    }
}
