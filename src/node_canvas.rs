use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use dioxus::html::geometry::WheelDelta;
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;

use crate::node_graph::{Body, Cable, Graph, Node, NodeId, WireState};
use crate::studio::local_storage;

const LAYOUT_KEY: &str = "noctics.node-layout";
const LAYOUT_SEPARATOR: char = ';';
const POINT_SEPARATOR: char = ':';
const PORTS_TOP: f64 = 46.0;
const PORT_ROW: f64 = 22.0;
const ESTIMATED_BODY: f64 = 150.0;
const MIN_ZOOM: f64 = 0.3;
const READABLE_ZOOM: f64 = 0.8;
const MAX_ZOOM: f64 = 1.6;
const ZOOM_STEP: f64 = 1.2;
const WHEEL_RATE: f64 = 0.0015;
const LINE_PIXELS: f64 = 16.0;
const PAGE_PIXELS: f64 = 400.0;
const FIT_MARGIN: f64 = 56.0;
const DOCK_SPACE: f64 = 56.0;
const DRAG_THRESHOLD: f64 = 4.0;
const SAG_RATE: f64 = 0.14;
const MAX_SAG: f64 = 70.0;
const MIN_HANDLE: f64 = 50.0;
const GRID_STEP: f64 = 24.0;
const KEY_PAN: f64 = 60.0;
const INSPECTOR_WIDTH: f64 = 384.0;
const FULL_PERCENT: f64 = 100.0;

#[derive(Clone, Copy, PartialEq)]
struct View {
    x: f64,
    y: f64,
    zoom: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Grip {
    Pan { x: f64, y: f64 },
    Node { id: NodeId, x: f64, y: f64 },
}

#[derive(Clone, Copy, PartialEq)]
struct Drag {
    grip: Grip,
    start: (f64, f64),
    moved: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Framing {
    Whole,
    Readable,
}

#[derive(Clone, Copy, PartialEq)]
struct Bounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

#[derive(Clone)]
struct Placement {
    id: NodeId,
    key: String,
    node: Node,
    x: f64,
    y: f64,
    patched_in: Vec<bool>,
    patched_out: Vec<bool>,
}

type Layout = HashMap<String, (f64, f64)>;
type ElementSlot = Rc<RefCell<Option<web_sys::Element>>>;

#[component]
pub fn NodeCanvas(
    graph: Graph,
    selected: Option<NodeId>,
    focus: Option<NodeId>,
    reveal: u64,
    inspector_open: bool,
    on_select: EventHandler<Option<NodeId>>,
) -> Element {
    let mut view = use_signal(|| View {
        x: FIT_MARGIN,
        y: FIT_MARGIN,
        zoom: 0.8,
    });
    let mut layout = use_signal(load_layout);
    let mut drag = use_signal(|| None::<Drag>);
    let mut touched = use_signal(|| false);
    let viewport: ElementSlot = use_hook(|| Rc::new(RefCell::new(None)));

    let bounds = bounds_of(&graph, &layout.read());
    let signature: String = graph
        .nodes
        .iter()
        .map(|node| node.id.key())
        .collect::<Vec<_>>()
        .join(",");

    let focus_point = focus
        .and_then(|id| graph.node(id))
        .map(|node| center_of(node, &layout.read()));
    let reserve = if inspector_open { INSPECTOR_WIDTH } else { 0.0 };

    let effect_slot = Rc::clone(&viewport);
    use_effect(use_reactive(
        (&signature, &bounds, &focus_point),
        move |(_, bounds, focus_point)| {
            if !*touched.peek() {
                fit_into(&effect_slot, view, bounds, 0.0, focus_point, Framing::Readable);
            }
        },
    ));

    let reveal_slot = Rc::clone(&viewport);
    use_effect(use_reactive((&reveal,), move |(reveal,)| {
        if reveal > 0
            && let Some(point) = focus_point
        {
            touched.set(false);
            centre_on(&reveal_slot, view, point, reserve);
        }
    }));

    let selected_span = selected.and_then(|id| graph.node(id)).map(|node| {
        let (x, _) = position_of(node, &layout.read());
        (x, x + node.width)
    });
    let selection_slot = Rc::clone(&viewport);
    use_effect(use_reactive((&selected,), move |(selected,)| {
        if selected.is_some()
            && let Some((left, right)) = selected_span
        {
            keep_visible(&selection_slot, view, left, right, reserve);
        }
    }));

    let mounted_slot = Rc::clone(&viewport);
    let pointer_slot = Rc::clone(&viewport);
    let node_slot = Rc::clone(&viewport);
    let wheel_slot = Rc::clone(&viewport);
    let key_slot = Rc::clone(&viewport);
    let button_slot = Rc::clone(&viewport);
    let zoom_slot = Rc::clone(&viewport);

    let current = view();
    let dragging_node = match drag() {
        Some(Drag {
            grip: Grip::Node { id, .. },
            moved: true,
            ..
        }) => Some(id),
        _ => None,
    };
    let panning = matches!(
        drag(),
        Some(Drag {
            grip: Grip::Pan { .. },
            moved: true,
            ..
        })
    );
    let placed: Vec<Placement> = graph
        .nodes
        .iter()
        .map(|node| {
            let (x, y) = position_of(node, &layout.read());
            Placement {
                id: node.id,
                key: node.id.key(),
                node: node.clone(),
                x,
                y,
                patched_in: graph.patched_inputs(node.id, node.inputs.len()),
                patched_out: graph.patched_outputs(node.id, node.outputs.len()),
            }
        })
        .collect();
    let lookup: HashMap<NodeId, (f64, f64, f64)> = placed
        .iter()
        .map(|card| (card.id, (card.x, card.y, card.node.width)))
        .collect();
    let cords: Vec<(String, &'static str, &'static str, bool)> = graph
        .wires
        .iter()
        .filter_map(|wire| {
            let (from_x, from_y, from_width) = *lookup.get(&wire.from)?;
            let (to_x, to_y, _) = *lookup.get(&wire.to)?;
            let start = (from_x + from_width, from_y + port_offset(wire.output));
            let end = (to_x, to_y + port_offset(wire.input));
            Some((
                cord_path(start, end),
                wire.cable.class(),
                wire.state.class(),
                wire.state == WireState::Flowing,
            ))
        })
        .collect();
    let grid = GRID_STEP * current.zoom;
    let percent = (current.zoom * FULL_PERCENT).round();

    rsx! {
        div {
            class: if panning { "nf-viewport is-panning" } else { "nf-viewport" },
            tabindex: "0",
            role: "application",
            aria_label: "Граф фабрики. Тягни полотно, щоб рухатися, колесо — масштаб, Enter на вузлі — відкрити його.",
            style: "background-size: {grid}px {grid}px; background-position: {current.x}px {current.y}px;",
            onmounted: move |event| {
                let element = event.data().downcast::<web_sys::Element>().cloned();
                *mounted_slot.borrow_mut() = element;
                if !*touched.peek() {
                    fit_into(&mounted_slot, view, bounds, 0.0, focus_point, Framing::Readable);
                }
            },
            onpointerdown: move |event| {
                if event.trigger_button() != Some(MouseButton::Primary) {
                    return;
                }
                let point = event.client_coordinates();
                capture(&pointer_slot, event.pointer_id());
                let origin = view();
                drag.set(Some(Drag {
                    grip: Grip::Pan { x: origin.x, y: origin.y },
                    start: (point.x, point.y),
                    moved: false,
                }));
            },
            onpointermove: move |event| {
                let Some(mut active) = drag() else {
                    return;
                };
                let point = event.client_coordinates();
                let shift = (point.x - active.start.0, point.y - active.start.1);
                if !active.moved && shift.0.hypot(shift.1) < DRAG_THRESHOLD {
                    return;
                }
                if !active.moved {
                    active.moved = true;
                    touched.set(true);
                    drag.set(Some(active));
                }
                match active.grip {
                    Grip::Pan { x, y } => view.with_mut(|state| {
                        state.x = x + shift.0;
                        state.y = y + shift.1;
                    }),
                    Grip::Node { id, x, y } => {
                        let zoom = view.peek().zoom;
                        layout.write().insert(id.key(), (x + shift.0 / zoom, y + shift.1 / zoom));
                    }
                }
            },
            onpointerup: move |_| {
                let Some(active) = drag() else {
                    return;
                };
                drag.set(None);
                match active.grip {
                    Grip::Node { id, .. } if !active.moved => on_select.call(Some(id)),
                    Grip::Node { .. } => save_layout(&layout.read()),
                    Grip::Pan { .. } if !active.moved => on_select.call(None),
                    Grip::Pan { .. } => {}
                }
            },
            onpointercancel: move |_| drag.set(None),
            onwheel: move |event| {
                let lift = wheel_pixels(event.delta());
                let point = event.client_coordinates();
                let Some((left, top, _, _)) = rect_of(&wheel_slot) else {
                    return;
                };
                touched.set(true);
                let origin = view();
                let zoom = origin.zoom * (-lift * WHEEL_RATE).exp();
                view.set(zoom_around(origin, zoom, point.x - left, point.y - top));
            },
            onkeydown: move |event| {
                let origin = view();
                let step = match event.key() {
                    Key::ArrowLeft => Some((KEY_PAN, 0.0)),
                    Key::ArrowRight => Some((-KEY_PAN, 0.0)),
                    Key::ArrowUp => Some((0.0, KEY_PAN)),
                    Key::ArrowDown => Some((0.0, -KEY_PAN)),
                    _ => None,
                };
                if let Some((dx, dy)) = step {
                    event.prevent_default();
                    touched.set(true);
                    view.set(View { x: origin.x + dx, y: origin.y + dy, ..origin });
                    return;
                }
                match event.key() {
                    Key::Character(text) if text == "+" || text == "=" => {
                        touched.set(true);
                        zoom_center(&key_slot, view, origin.zoom * ZOOM_STEP);
                    }
                    Key::Character(text) if text == "-" => {
                        touched.set(true);
                        zoom_center(&key_slot, view, origin.zoom / ZOOM_STEP);
                    }
                    Key::Character(text) if text == "0" || text.eq_ignore_ascii_case("f") => {
                        touched.set(true);
                        fit_into(&key_slot, view, bounds, reserve, None, Framing::Whole);
                    }
                    Key::Escape => on_select.call(None),
                    _ => {}
                }
            },
            div {
                class: "nf-world",
                style: "transform: translate({current.x}px, {current.y}px) scale({current.zoom});",
                svg { class: "nf-wires", "aria-hidden": "true",
                    for (index, (path, cable, state, flowing)) in cords.into_iter().enumerate() {
                        g { key: "{index}", class: "{cable}",
                            path { class: "nf-cord-shade", d: "{path}" }
                            path { class: "{state}", d: "{path}" }
                            if flowing {
                                path { class: "nf-pulse", d: "{path}", path_length: "100" }
                            }
                        }
                    }
                }
                for card in placed {
                    NodeCard {
                        key: "{card.key}",
                        selected: selected == Some(card.id),
                        dragged: dragging_node == Some(card.id),
                        x: card.x,
                        y: card.y,
                        patched_in: card.patched_in,
                        patched_out: card.patched_out,
                        on_grab: {
                            let slot = Rc::clone(&node_slot);
                            let (x, y) = (card.x, card.y);
                            move |(id, client_x, client_y, pointer): (NodeId, f64, f64, i32)| {
                                capture(&slot, pointer);
                                drag.set(Some(Drag {
                                    grip: Grip::Node { id, x, y },
                                    start: (client_x, client_y),
                                    moved: false,
                                }));
                            }
                        },
                        on_select: move |id| on_select.call(Some(id)),
                        node: card.node,
                    }
                }
            }
        }
        div { class: "nf-dock",
            div { class: "nf-controls", role: "toolbar", aria_label: "Масштаб полотна",
                button {
                    class: "nf-control",
                    r#type: "button",
                    title: "Зменшити (−)",
                    aria_label: "Зменшити",
                    onclick: {
                        let slot = Rc::clone(&zoom_slot);
                        move |_| {
                            touched.set(true);
                            zoom_center(&slot, view, view.peek().zoom / ZOOM_STEP);
                        }
                    },
                    "−"
                }
                button {
                    class: "nf-control nf-zoom",
                    r#type: "button",
                    title: "Масштаб 100%",
                    onclick: {
                        let slot = Rc::clone(&zoom_slot);
                        move |_| {
                            touched.set(true);
                            zoom_center(&slot, view, 1.0);
                        }
                    },
                    "{percent}%"
                }
                button {
                    class: "nf-control",
                    r#type: "button",
                    title: "Збільшити (+)",
                    aria_label: "Збільшити",
                    onclick: {
                        let slot = Rc::clone(&zoom_slot);
                        move |_| {
                            touched.set(true);
                            zoom_center(&slot, view, view.peek().zoom * ZOOM_STEP);
                        }
                    },
                    "+"
                }
                button {
                    class: "nf-control",
                    r#type: "button",
                    title: "Показати весь граф (F)",
                    onclick: move |_| {
                        touched.set(true);
                        fit_into(&button_slot, view, bounds, reserve, None, Framing::Whole);
                    },
                    "Вмістити"
                }
                button {
                    class: "nf-control",
                    r#type: "button",
                    title: "Повернути вузли на початкові місця",
                    onclick: move |_| {
                        layout.set(Layout::new());
                        save_layout(&Layout::new());
                        touched.set(false);
                    },
                    "Розкласти"
                }
            }
            div { class: "nf-legend", aria_label: "Кольори кабелів",
                for cable in Cable::ALL {
                    span { key: "{cable.class()}", class: "nf-legend-item {cable.class()}",
                        span { class: "nf-legend-cord" }
                        "{cable.label()}"
                    }
                }
            }
        }
    }
}

#[component]
fn NodeCard(
    node: Node,
    x: f64,
    y: f64,
    selected: bool,
    dragged: bool,
    patched_in: Vec<bool>,
    patched_out: Vec<bool>,
    on_grab: EventHandler<(NodeId, f64, f64, i32)>,
    on_select: EventHandler<NodeId>,
) -> Element {
    let id = node.id;
    let phase = node.phase;
    let mut class = format!("nf-node {} is-{}", node.tint.tint(), phase.code());
    if selected {
        class.push_str(" is-selected");
    }
    if dragged {
        class.push_str(" is-dragged");
    }
    let rows = node.inputs.len().max(node.outputs.len());
    let percent = node
        .progress
        .filter(|(_, total)| *total > 0)
        .map(|(done, total)| done as f64 * FULL_PERCENT / total as f64);

    rsx! {
        div {
            class: "{class}",
            style: "left: {x}px; top: {y}px; width: {node.width}px;",
            role: "button",
            tabindex: "0",
            aria_pressed: if selected { "true" } else { "false" },
            aria_label: "{node.title}: {phase.label()}",
            onpointerdown: move |event| {
                event.stop_propagation();
                if event.trigger_button() != Some(MouseButton::Primary) {
                    return;
                }
                let point = event.client_coordinates();
                on_grab.call((id, point.x, point.y, event.pointer_id()));
            },
            onkeydown: move |event| {
                if event.key() == Key::Enter || event.key() == Key::Character(" ".to_owned()) {
                    event.prevent_default();
                    event.stop_propagation();
                    on_select.call(id);
                }
            },
            div { class: "nf-node-head",
                span { class: "nf-led is-{phase.code()}", title: "{phase.label()}" }
                span { class: "nf-node-title", "{node.title}" }
                span { class: "nf-node-system", "{node.system}" }
            }
            div { class: "nf-ports",
                div { class: "nf-port-column",
                    for index in 0..rows {
                        if let Some(port) = node.inputs.get(index) {
                            span { key: "in-{index}", class: "nf-port is-in {port.cable.class()}",
                                span { class: if patched_in.get(index).copied().unwrap_or(false) { "nf-jack is-patched" } else { "nf-jack" } }
                                "{port.label}"
                            }
                        } else {
                            span { key: "in-{index}", class: "nf-port is-in" }
                        }
                    }
                }
                div { class: "nf-port-column",
                    for index in 0..rows {
                        if let Some(port) = node.outputs.get(index) {
                            span { key: "out-{index}", class: "nf-port is-out {port.cable.class()}",
                                "{port.label}"
                                span { class: if patched_out.get(index).copied().unwrap_or(false) { "nf-jack is-patched" } else { "nf-jack" } }
                            }
                        } else {
                            span { key: "out-{index}", class: "nf-port is-out" }
                        }
                    }
                }
            }
            div { class: "nf-body",
                if !node.summary.is_empty() {
                    p { class: "nf-summary", title: "{node.summary}", "{node.summary}" }
                }
                NodeBody { body: node.body.clone() }
                if let (Some(percent), Some((done, total))) = (percent, node.progress) {
                    div { class: "nf-meter",
                        span { class: "nf-meter-track",
                            span { class: "nf-meter-fill", style: "width: {percent}%;" }
                        }
                        span { class: "nf-meter-count", "{done}/{total}" }
                    }
                }
            }
        }
    }
}

#[component]
fn NodeBody(body: Body) -> Element {
    match body {
        Body::Empty => rsx! {},
        Body::Prompt { text, chips } => rsx! {
            if !text.is_empty() {
                p { class: "nf-quote", "{text}" }
            }
            div { class: "nf-chips",
                for chip in chips {
                    span { key: "{chip}", class: "nf-chip", "{chip}" }
                }
            }
        },
        Body::Plan { name, pitch, palette, chips } => rsx! {
            p { class: "nf-game-name", "{name}" }
            if !pitch.is_empty() {
                p { class: "nf-summary", "{pitch}" }
            }
            if !palette.is_empty() {
                div { class: "nf-swatches", aria_label: "Палітра гри",
                    for color in palette {
                        span { key: "{color}", class: "nf-swatch", style: "background: {color};", title: "{color}" }
                    }
                }
            }
            div { class: "nf-chips",
                for chip in chips {
                    span { key: "{chip}", class: "nf-chip", "{chip}" }
                }
            }
        },
        Body::Gallery { images } => rsx! {
            div { class: "nf-gallery",
                for (index, image) in images.into_iter().enumerate() {
                    img { key: "{index}", src: "{image}", alt: "", draggable: "false", "loading": "lazy" }
                }
            }
        },
        Body::Rows { rows, hidden } => rsx! {
            ul { class: "nf-rows",
                for (index, row) in rows.into_iter().enumerate() {
                    li { key: "{index}", class: "nf-row",
                        span { class: "nf-led is-{row.phase.code()}", title: "{row.phase.label()}" }
                        if let Some(tag) = row.tag {
                            span { class: "nf-row-tag", "{tag}" }
                        }
                        span { class: "nf-row-text", title: "{row.text}", "{row.text}" }
                    }
                }
                if hidden > 0 {
                    li { class: "nf-row-more", "ще {hidden}" }
                }
            }
        },
        Body::Log { lines } => rsx! {
            ul { class: "nf-log",
                for (index, line) in lines.into_iter().enumerate() {
                    li { key: "{index}", title: "{line}", "{line}" }
                }
            }
        },
    }
}

fn port_offset(index: usize) -> f64 {
    PORTS_TOP + index as f64 * PORT_ROW + PORT_ROW / 2.0
}

fn cord_path(start: (f64, f64), end: (f64, f64)) -> String {
    let reach = (end.0 - start.0).abs();
    let handle = (reach * 0.5).max(MIN_HANDLE);
    let sag = ((end.0 - start.0).hypot(end.1 - start.1) * SAG_RATE).min(MAX_SAG);
    format!(
        "M {:.1} {:.1} C {:.1} {:.1}, {:.1} {:.1}, {:.1} {:.1}",
        start.0,
        start.1,
        start.0 + handle,
        start.1 + sag,
        end.0 - handle,
        end.1 + sag,
        end.0,
        end.1
    )
}

fn position_of(node: &Node, layout: &Layout) -> (f64, f64) {
    layout.get(&node.id.key()).copied().unwrap_or(node.home)
}

fn center_of(node: &Node, layout: &Layout) -> (f64, f64) {
    let (x, y) = position_of(node, layout);
    (x + node.width / 2.0, y + estimated_height(node) / 2.0)
}

fn estimated_height(node: &Node) -> f64 {
    PORTS_TOP + node.inputs.len().max(node.outputs.len()) as f64 * PORT_ROW + ESTIMATED_BODY
}

fn bounds_of(graph: &Graph, layout: &Layout) -> Bounds {
    graph.nodes.iter().fold(
        Bounds {
            left: f64::MAX,
            top: f64::MAX,
            right: f64::MIN,
            bottom: f64::MIN,
        },
        |bounds, node| {
            let (x, y) = position_of(node, layout);
            Bounds {
                left: bounds.left.min(x),
                top: bounds.top.min(y),
                right: bounds.right.max(x + node.width),
                bottom: bounds.bottom.max(y + estimated_height(node)),
            }
        },
    )
}

fn fit_into(
    slot: &ElementSlot,
    mut view: Signal<View>,
    bounds: Bounds,
    reserve: f64,
    focus: Option<(f64, f64)>,
    framing: Framing,
) {
    let Some((_, _, width, height)) = rect_of(slot) else {
        return;
    };
    if bounds.right <= bounds.left || width < 2.0 || height < 2.0 {
        return;
    }
    let room_width = (width - reserve - FIT_MARGIN * 2.0).max(1.0);
    let room_height = (height - DOCK_SPACE - FIT_MARGIN * 2.0).max(1.0);
    let span_width = bounds.right - bounds.left;
    let span_height = bounds.bottom - bounds.top;
    let whole = (room_width / span_width).min(room_height / span_height).min(1.0);
    let zoom = match framing {
        Framing::Whole => whole.max(MIN_ZOOM),
        Framing::Readable => whole.max(READABLE_ZOOM),
    };
    view.set(View {
        x: place(bounds.left, span_width, room_width, zoom, focus.map(|point| point.0)),
        y: place(bounds.top, span_height, room_height, zoom, focus.map(|point| point.1)),
        zoom,
    });
}

fn place(start: f64, span: f64, room: f64, zoom: f64, focus: Option<f64>) -> f64 {
    let content = span * zoom;
    if content <= room {
        return FIT_MARGIN + (room - content) / 2.0 - start * zoom;
    }
    let earliest = FIT_MARGIN + room - (start + span) * zoom;
    let latest = FIT_MARGIN - start * zoom;
    focus
        .map_or(latest, |point| FIT_MARGIN + room / 2.0 - point * zoom)
        .clamp(earliest, latest)
}

fn centre_on(slot: &ElementSlot, mut view: Signal<View>, point: (f64, f64), reserve: f64) {
    let Some((_, _, width, height)) = rect_of(slot) else {
        return;
    };
    let zoom = view.peek().zoom.max(READABLE_ZOOM);
    view.set(View {
        x: (width - reserve).max(1.0) / 2.0 - point.0 * zoom,
        y: (height - DOCK_SPACE).max(1.0) / 2.0 - point.1 * zoom,
        zoom,
    });
}

fn keep_visible(slot: &ElementSlot, mut view: Signal<View>, left: f64, right: f64, reserve: f64) {
    let Some((_, _, width, _)) = rect_of(slot) else {
        return;
    };
    let origin = *view.peek();
    let screen_left = origin.x + left * origin.zoom;
    let screen_right = origin.x + right * origin.zoom;
    let limit = width - reserve - FIT_MARGIN;
    let mut shift = 0.0;
    if screen_right > limit {
        shift = limit - screen_right;
    }
    if screen_left + shift < FIT_MARGIN {
        shift = FIT_MARGIN - screen_left;
    }
    if shift.abs() > 0.5 {
        view.set(View {
            x: origin.x + shift,
            ..origin
        });
    }
}

fn zoom_around(origin: View, zoom: f64, anchor_x: f64, anchor_y: f64) -> View {
    let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    let world_x = (anchor_x - origin.x) / origin.zoom;
    let world_y = (anchor_y - origin.y) / origin.zoom;
    View {
        x: anchor_x - world_x * zoom,
        y: anchor_y - world_y * zoom,
        zoom,
    }
}

fn zoom_center(slot: &ElementSlot, mut view: Signal<View>, zoom: f64) {
    let Some((_, _, width, height)) = rect_of(slot) else {
        return;
    };
    let origin = *view.peek();
    view.set(zoom_around(origin, zoom, width / 2.0, height / 2.0));
}

fn wheel_pixels(delta: WheelDelta) -> f64 {
    match delta {
        WheelDelta::Pixels(amount) => amount.y,
        WheelDelta::Lines(amount) => amount.y * LINE_PIXELS,
        WheelDelta::Pages(amount) => amount.y * PAGE_PIXELS,
    }
}

fn rect_of(slot: &ElementSlot) -> Option<(f64, f64, f64, f64)> {
    let element = slot.borrow().clone()?;
    let rect = element.get_bounding_client_rect();
    Some((rect.left(), rect.top(), rect.width(), rect.height()))
}

fn capture(slot: &ElementSlot, pointer: i32) {
    if let Some(element) = slot.borrow().as_ref() {
        let _ = element.set_pointer_capture(pointer);
    }
}

fn load_layout() -> Layout {
    let stored = local_storage()
        .and_then(|storage| storage.get_item(LAYOUT_KEY).ok().flatten())
        .unwrap_or_default();
    stored
        .split(LAYOUT_SEPARATOR)
        .filter_map(|entry| {
            let mut parts = entry.split(POINT_SEPARATOR);
            let key = parts.next()?.to_owned();
            let x = parts.next()?.parse().ok()?;
            let y = parts.next()?.parse().ok()?;
            Some((key, (x, y)))
        })
        .collect()
}

fn save_layout(layout: &Layout) {
    let encoded = layout
        .iter()
        .map(|(key, (x, y))| format!("{key}{POINT_SEPARATOR}{x:.0}{POINT_SEPARATOR}{y:.0}"))
        .collect::<Vec<_>>()
        .join(&LAYOUT_SEPARATOR.to_string());
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(LAYOUT_KEY, &encoded);
    }
}

