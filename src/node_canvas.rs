use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use dioxus::html::geometry::WheelDelta;
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use crate::icons::{CheckIcon, ChevronDownIcon, NodesIcon, RefreshIcon};
use crate::inspector_layout::use_inspector_layout;
use crate::node_graph::{Body, Cable, Graph, Node, NodeId, Phase, Port, WireState};
use crate::studio::local_storage;

const LAYOUT_KEY: &str = "noctics.node-layout";
const TRAY_KEY: &str = "noctics.node-tray";
const TRAY_OPEN_KEY: &str = "noctics.node-tray-open";
const TRAY_SEPARATOR: char = ',';
const TRAY_CLOSED: &str = "0";
const TRAY_OPENED: &str = "1";
const TRAY_HEIGHT: f64 = 150.0;
const GRAB_DEPTH: f64 = 20.0;
const LAYOUT_SEPARATOR: char = ';';
const POINT_SEPARATOR: char = ':';
const PORTS_TOP: f64 = 46.0;
const PORT_ROW: f64 = 22.0;
const ESTIMATED_BODY: f64 = 150.0;
const MIN_ZOOM: f64 = 0.05;
const READABLE_ZOOM: f64 = 0.8;
const MAX_ZOOM: f64 = 2.5;
const ZOOM_STEP: f64 = 1.2;
const WHEEL_RATE: f64 = 0.0015;
const WHEEL_LIMIT: f64 = 120.0;
const PINCH_BOOST: f64 = 8.0;
const MIN_GRID: f64 = 12.0;
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
enum ZoomKey {
    In,
    Out,
    Actual,
}

struct ZoomKeys {
    window: web_sys::Window,
    handler: Closure<dyn FnMut(web_sys::KeyboardEvent)>,
}

impl ZoomKeys {
    fn listen(slot: ElementSlot, room: Rc<Cell<Insets>>, view: Signal<View>, mut touched: Signal<bool>) -> Option<Self> {
        let window = web_sys::window()?;
        let handler = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |event: web_sys::KeyboardEvent| {
            if !(event.ctrl_key() || event.meta_key()) || event.alt_key() || event.default_prevented() {
                return;
            }
            let Some(key) = event.code().parse::<Code>().ok().as_ref().and_then(zoom_key) else {
                return;
            };
            event.prevent_default();
            touched.set(true);
            step_zoom(&slot, view, room.get(), key);
        });
        window
            .add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref())
            .ok()?;
        Some(Self { window, handler })
    }
}

impl Drop for ZoomKeys {
    fn drop(&mut self) {
        let _ = self
            .window
            .remove_event_listener_with_callback("keydown", self.handler.as_ref().unchecked_ref());
    }
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

#[derive(Clone, Copy, PartialEq)]
struct Carry {
    id: NodeId,
    start: (f64, f64),
    point: (f64, f64),
    moved: bool,
    over_tray: bool,
}

#[derive(Clone, Copy, PartialEq)]
struct Insets {
    right: f64,
    bottom: f64,
}

#[derive(Clone)]
struct Pocket {
    sheet: ElementSlot,
    button: ElementSlot,
}

impl Pocket {
    fn holds(&self, point: (f64, f64)) -> bool {
        inside(&self.sheet, point) || inside(&self.button, point)
    }
}

#[derive(Clone, Copy)]
struct Board {
    view: Signal<View>,
    layout: Signal<Layout>,
    stowed: Signal<Stowed>,
    touched: Signal<bool>,
}

impl Board {
    fn land(mut self, slot: &ElementSlot, node: &Node, insets: Insets, client: Option<(f64, f64)>) {
        let Some((left, top, width, height)) = rect_of(slot) else {
            return;
        };
        let (screen_x, screen_y) = client.map_or(
            ((width - insets.right) / 2.0, (height - insets.bottom) / 2.0),
            |(x, y)| (x - left, y - top),
        );
        if screen_x < 0.0 || screen_y < 0.0 || screen_x > width - insets.right || screen_y > height {
            return;
        }
        let origin = *self.view.peek();
        let world_x = (screen_x - origin.x) / origin.zoom;
        let world_y = (screen_y - origin.y) / origin.zoom;
        self.layout
            .write()
            .insert(node.id.key(), (world_x - node.width / 2.0, world_y - GRAB_DEPTH));
        save_layout(&self.layout.peek());
        self.stowed.write().remove(&node.id.key());
        save_stowed(&self.stowed.peek());
        self.touched.set(true);
    }

    fn stow(mut self, id: NodeId) {
        self.stowed.write().insert(id.key());
        save_stowed(&self.stowed.peek());
        self.touched.set(true);
    }

    fn pick(self, slot: &ElementSlot, node: &Node, insets: Insets, on_select: EventHandler<Option<NodeId>>) {
        if self.stowed.peek().contains(&node.id.key()) {
            self.land(slot, node, insets, None);
        } else {
            on_select.call(Some(node.id));
        }
    }
}

type Layout = HashMap<String, (f64, f64)>;
type Stowed = HashSet<String>;
type ElementSlot = Rc<RefCell<Option<web_sys::Element>>>;

#[component]
pub fn NodeCanvas(
    graph: Graph,
    selected: Option<NodeId>,
    focus: Option<NodeId>,
    reveal: u64,
    inspector_open: bool,
    stalled: Vec<NodeId>,
    resuming: bool,
    on_select: EventHandler<Option<NodeId>>,
    on_resume: EventHandler<NodeId>,
) -> Element {
    let mut view = use_signal(|| View {
        x: FIT_MARGIN,
        y: FIT_MARGIN,
        zoom: 0.8,
    });
    let mut layout = use_signal(load_layout);
    let mut drag = use_signal(|| None::<Drag>);
    let mut touched = use_signal(|| false);
    let mut stowed = use_signal(load_stowed);
    let mut tray_open = use_signal(load_tray_open);
    let mut carry = use_signal(|| None::<Carry>);
    let mut catching = use_signal(|| false);
    let inspector = use_inspector_layout();
    let viewport: ElementSlot = use_hook(|| Rc::new(RefCell::new(None)));
    let room = use_hook(|| {
        Rc::new(Cell::new(Insets {
            right: 0.0,
            bottom: 0.0,
        }))
    });
    let zoom_keys = use_hook(|| {
        Rc::new(RefCell::new(ZoomKeys::listen(
            Rc::clone(&viewport),
            Rc::clone(&room),
            view,
            touched,
        )))
    });
    use_drop(move || {
        zoom_keys.borrow_mut().take();
    });
    let pocket = use_hook(|| Pocket {
        sheet: Rc::new(RefCell::new(None)),
        button: Rc::new(RefCell::new(None)),
    });
    let board = Board {
        view,
        layout,
        stowed,
        touched,
    };

    let scene = visible_part(&graph, &stowed.read());
    let bounds = bounds_of(&scene, &layout.read());
    let signature: String = scene
        .nodes
        .iter()
        .map(|node| node.id.key())
        .collect::<Vec<_>>()
        .join(",");

    let focus_point = focus
        .and_then(|id| scene.node(id))
        .map(|node| center_of(node, &layout.read()));
    let reserve = if inspector_open { inspector.settled_width() } else { 0.0 };
    let dock_right = if inspector_open {
        "right: min(var(--nf-inspector-width, 384px), 100%);"
    } else {
        "right: 0px;"
    };
    let floor_space = DOCK_SPACE + if tray_open() { TRAY_HEIGHT } else { 0.0 };
    let follow = Insets {
        right: 0.0,
        bottom: floor_space,
    };
    let insets = Insets {
        right: reserve,
        bottom: floor_space,
    };
    room.set(insets);

    let effect_slot = Rc::clone(&viewport);
    use_effect(use_reactive(
        (&signature, &bounds, &focus_point, &floor_space),
        move |(_, bounds, focus_point, floor_space)| {
            if !*touched.peek() {
                let follow = Insets {
                    right: 0.0,
                    bottom: floor_space,
                };
                fit_into(&effect_slot, view, bounds, follow, focus_point, Framing::Readable);
            }
        },
    ));

    let reveal_slot = Rc::clone(&viewport);
    use_effect(use_reactive((&reveal,), move |(reveal,)| {
        if reveal > 0
            && let Some(point) = focus_point
        {
            touched.set(false);
            centre_on(&reveal_slot, view, point, insets);
        }
    }));

    let selected_span = selected.and_then(|id| scene.node(id)).map(|node| {
        let (x, _) = position_of(node, &layout.read());
        (x, x + node.width)
    });
    let selection_slot = Rc::clone(&viewport);
    use_effect(use_reactive((&selected,), move |(selected,)| {
        if selected.is_some()
            && let Some((left, right)) = selected_span
        {
            keep_visible(&selection_slot, view, left, right, insets);
        }
    }));

    let mounted_slot = Rc::clone(&viewport);
    let pointer_slot = Rc::clone(&viewport);
    let node_slot = Rc::clone(&viewport);
    let wheel_slot = Rc::clone(&viewport);
    let key_slot = Rc::clone(&viewport);
    let button_slot = Rc::clone(&viewport);
    let zoom_slot = Rc::clone(&viewport);
    let catch_pocket = pocket.clone();
    let release_pocket = pocket.clone();
    let carry_pocket = pocket.clone();
    let drop_pocket = pocket.clone();
    let sheet_mount = Rc::clone(&pocket.sheet);
    let button_mount = Rc::clone(&pocket.button);
    let carry_view = Rc::clone(&viewport);
    let drop_view = Rc::clone(&viewport);

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
    let placed: Vec<Placement> = scene
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
                patched_in: scene.patched_inputs(node.id, node.inputs.len()),
                patched_out: scene.patched_outputs(node.id, node.outputs.len()),
            }
        })
        .collect();
    let lookup: HashMap<NodeId, (f64, f64, f64)> = placed
        .iter()
        .map(|card| (card.id, (card.x, card.y, card.node.width)))
        .collect();
    let cords: Vec<(String, &'static str, &'static str, bool)> = scene
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
    let grid = grid_size(current.zoom);
    let percent = (current.zoom * FULL_PERCENT).round();
    let total = graph.nodes.len();
    let shown = scene.nodes.len();
    let carried = carry().filter(|active| active.moved);
    let ghost = carried.and_then(|active| graph.node(active.id).cloned().map(|node| (active, node)));
    let bottom_class = if catching() { "nf-bottom is-catching" } else { "nf-bottom" };
    let toggle_class = if tray_open() { "nf-tray-toggle is-open" } else { "nf-tray-toggle" };
    let mut tray_class = String::from(if tray_open() { "nf-tray is-open" } else { "nf-tray" });
    if carried.is_some() {
        tray_class.push_str(" is-carrying");
    }
    let tray_hint = if catching() {
        "Відпусти, і вузол сховається сюди"
    } else {
        "Тягни вузол на полотно, а з полотна сюди, щоб сховати"
    };
    let tray_graph = graph.clone();

    rsx! {
        div {
            class: if panning { "nf-viewport is-panning" } else { "nf-viewport" },
            tabindex: "0",
            role: "application",
            aria_label: "Граф фабрики. Тягни полотно лівою або середньою кнопкою, щоб рухатися. Колесо миші, Ctrl + і Ctrl − змінюють масштаб, Ctrl 0 повертає 100 %, F показує весь граф. Enter на вузлі відкриває його.",
            style: "background-size: {grid}px {grid}px; background-position: {current.x}px {current.y}px;",
            onmounted: move |event| {
                let element = event.data().downcast::<web_sys::Element>().cloned();
                *mounted_slot.borrow_mut() = element;
                if !*touched.peek() {
                    fit_into(&mounted_slot, view, bounds, follow, focus_point, Framing::Readable);
                }
            },
            onpointerdown: move |event| {
                let button = event.trigger_button();
                if button == Some(MouseButton::Auxiliary) {
                    event.prevent_default();
                } else if button != Some(MouseButton::Primary) {
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
                        let over = catch_pocket.holds((point.x, point.y));
                        if *catching.peek() != over {
                            catching.set(over);
                        }
                    }
                }
            },
            onpointerup: move |event| {
                let Some(active) = drag() else {
                    return;
                };
                drag.set(None);
                let point = event.client_coordinates();
                let into_tray = active.moved && release_pocket.holds((point.x, point.y));
                if *catching.peek() {
                    catching.set(false);
                }
                match active.grip {
                    Grip::Node { id, .. } if !active.moved => on_select.call(Some(id)),
                    Grip::Node { id, x, y } if into_tray => {
                        layout.write().insert(id.key(), (x, y));
                        save_layout(&layout.read());
                        board.stow(id);
                        if selected == Some(id) {
                            on_select.call(None);
                        }
                    }
                    Grip::Node { .. } => save_layout(&layout.read()),
                    Grip::Pan { .. } if !active.moved => on_select.call(None),
                    Grip::Pan { .. } => {}
                }
            },
            onpointercancel: move |_| {
                drag.set(None);
                if *catching.peek() {
                    catching.set(false);
                }
            },
            onwheel: move |event| {
                event.prevent_default();
                let Some((left, top, _, _)) = rect_of(&wheel_slot) else {
                    return;
                };
                let lift = wheel_pixels(event.delta());
                let pinch = event.modifiers().ctrl();
                let point = event.client_coordinates();
                touched.set(true);
                let origin = view();
                let zoom = origin.zoom * wheel_factor(lift, pinch);
                view.set(zoom_around(origin, zoom, point.x - left, point.y - top));
            },
            onkeydown: move |event| {
                let modifiers = event.modifiers();
                if modifiers.ctrl() || modifiers.meta() || modifiers.alt() {
                    return;
                }
                if let Some(key) = zoom_key(&event.code()) {
                    event.prevent_default();
                    touched.set(true);
                    step_zoom(&key_slot, view, insets, key);
                    return;
                }
                if event.code() == Code::KeyF {
                    event.prevent_default();
                    touched.set(true);
                    fit_into(&key_slot, view, bounds, insets, None, Framing::Whole);
                    return;
                }
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
                if event.key() == Key::Escape {
                    on_select.call(None);
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
        div { class: "{bottom_class}", style: "{dock_right}",
            div { class: "nf-dock",
                button {
                    class: "{toggle_class}",
                    r#type: "button",
                    title: if tray_open() { "Сховати панель вузлів" } else { "Показати панель вузлів" },
                    aria_expanded: if tray_open() { "true" } else { "false" },
                    onmounted: move |event| {
                        *button_mount.borrow_mut() = event.data().downcast::<web_sys::Element>().cloned();
                    },
                    onclick: move |_| {
                        let next = !tray_open();
                        tray_open.set(next);
                        save_tray_open(next);
                    },
                    NodesIcon {}
                    span { "Вузли" }
                    span { class: "nf-tray-count", title: "На полотні {shown} з {total}", "{shown}/{total}" }
                    span { class: "nf-tray-chevron", ChevronDownIcon {} }
                }
                div { class: "nf-controls", role: "toolbar", aria_label: "Масштаб полотна",
                    button {
                        class: "nf-control",
                        r#type: "button",
                        title: "Зменшити (Ctrl −)",
                        aria_label: "Зменшити",
                        onclick: {
                            let slot = Rc::clone(&zoom_slot);
                            move |_| {
                                touched.set(true);
                                step_zoom(&slot, view, insets, ZoomKey::Out);
                            }
                        },
                        "−"
                    }
                    button {
                        class: "nf-control nf-zoom",
                        r#type: "button",
                        title: "Масштаб 100% (Ctrl 0)",
                        onclick: {
                            let slot = Rc::clone(&zoom_slot);
                            move |_| {
                                touched.set(true);
                                step_zoom(&slot, view, insets, ZoomKey::Actual);
                            }
                        },
                        "{percent}%"
                    }
                    button {
                        class: "nf-control",
                        r#type: "button",
                        title: "Збільшити (Ctrl +)",
                        aria_label: "Збільшити",
                        onclick: {
                            let slot = Rc::clone(&zoom_slot);
                            move |_| {
                                touched.set(true);
                                step_zoom(&slot, view, insets, ZoomKey::In);
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
                            fit_into(&button_slot, view, bounds, insets, None, Framing::Whole);
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
            aside {
                class: "{tray_class}",
                aria_label: "Панель вузлів",
                onmounted: move |event| {
                    *sheet_mount.borrow_mut() = event.data().downcast::<web_sys::Element>().cloned();
                },
                onpointermove: move |event| {
                    let Some(mut active) = carry() else {
                        return;
                    };
                    let point = event.client_coordinates();
                    let shift = (point.x - active.start.0, point.y - active.start.1);
                    if !active.moved && shift.0.hypot(shift.1) < DRAG_THRESHOLD {
                        return;
                    }
                    let Some((left, top, _, _)) = rect_of(&carry_view) else {
                        return;
                    };
                    active.moved = true;
                    active.point = (point.x - left, point.y - top);
                    active.over_tray = carry_pocket.holds((point.x, point.y));
                    carry.set(Some(active));
                },
                onpointerup: move |event| {
                    let Some(active) = carry() else {
                        return;
                    };
                    carry.set(None);
                    let Some(node) = tray_graph.node(active.id) else {
                        return;
                    };
                    let point = event.client_coordinates();
                    if !active.moved {
                        board.pick(&drop_view, node, insets, on_select);
                    } else if !drop_pocket.holds((point.x, point.y)) {
                        board.land(&drop_view, node, insets, Some((point.x, point.y)));
                    }
                },
                onpointercancel: move |_| carry.set(None),
                div { class: "nf-tray-sheet",
                    div { class: "nf-tray-bar",
                        p { class: "nf-tray-hint", "{tray_hint}" }
                        if shown < total {
                            button {
                                class: "nf-tray-restore",
                                r#type: "button",
                                title: "Повернути всі сховані вузли на полотно",
                                onclick: move |_| {
                                    stowed.write().clear();
                                    save_stowed(&stowed.peek());
                                },
                                "Повернути всі"
                            }
                        }
                    }
                    div { class: "nf-tray-list",
                        for node in graph.nodes.iter().cloned() {
                            TrayCard {
                                key: "{node.id.key()}",
                                placed: scene.node(node.id).is_some(),
                                carried: carried.is_some_and(|active| active.id == node.id),
                                stalled: stalled.contains(&node.id),
                                busy: resuming,
                                on_resume,
                                on_lift: {
                                    let slot = Rc::clone(&pocket.sheet);
                                    move |(id, client_x, client_y, pointer): (NodeId, f64, f64, i32)| {
                                        capture(&slot, pointer);
                                        carry.set(Some(Carry {
                                            id,
                                            start: (client_x, client_y),
                                            point: (0.0, 0.0),
                                            moved: false,
                                            over_tray: true,
                                        }));
                                    }
                                },
                                on_pick: {
                                    let slot = Rc::clone(&viewport);
                                    let picked = node.clone();
                                    move |_| board.pick(&slot, &picked, insets, on_select)
                                },
                                on_stow: move |id: NodeId| {
                                    board.stow(id);
                                    if selected == Some(id) {
                                        on_select.call(None);
                                    }
                                },
                                node,
                            }
                        }
                    }
                }
            }
        }
        if let Some((active, node)) = ghost {
            NodeGhost {
                x: active.point.0 - node.width * current.zoom / 2.0,
                y: active.point.1 - GRAB_DEPTH * current.zoom,
                zoom: current.zoom,
                void: active.over_tray,
                node,
            }
        }
    }
}

#[component]
fn TrayCard(
    node: Node,
    placed: bool,
    carried: bool,
    stalled: bool,
    busy: bool,
    on_lift: EventHandler<(NodeId, f64, f64, i32)>,
    on_pick: EventHandler<()>,
    on_stow: EventHandler<NodeId>,
    on_resume: EventHandler<NodeId>,
) -> Element {
    let id = node.id;
    let phase = node.phase;
    let mut class = format!("nf-tray-card {}", node.tint.tint());
    if placed {
        class.push_str(" is-placed");
    }
    if carried {
        class.push_str(" is-carried");
    }
    if stalled {
        class.push_str(" is-stalled");
    }
    let state = if placed { "на полотні" } else { "сховано" };
    let usage = if placed {
        "На полотні. Тягни, щоб перенести; клік відкриє вузол; Delete сховає"
    } else {
        "Тягни на полотно або натисни, щоб поставити в центр"
    };
    let hint = if phase == Phase::Failed && !node.summary.is_empty() {
        format!("{}\n{usage}", node.summary)
    } else {
        usage.to_owned()
    };

    rsx! {
        div { class: "nf-tray-slot",
            div {
                class: "{class}",
                role: "button",
                tabindex: "0",
                title: "{hint}",
                aria_label: "{node.title}: {state}, {phase.label()}",
                onpointerdown: move |event| {
                    if event.trigger_button() != Some(MouseButton::Primary) {
                        return;
                    }
                    let point = event.client_coordinates();
                    on_lift.call((id, point.x, point.y, event.pointer_id()));
                },
                onkeydown: move |event| {
                    match event.key() {
                        Key::Enter => {
                            event.prevent_default();
                            on_pick.call(());
                        }
                        Key::Character(text) if text == " " => {
                            event.prevent_default();
                            on_pick.call(());
                        }
                        Key::Delete | Key::Backspace if placed => {
                            event.prevent_default();
                            on_stow.call(id);
                        }
                        _ => {}
                    }
                },
                span { class: "nf-tray-jacks is-in",
                    for (index, port) in node.inputs.iter().enumerate() {
                        span { key: "{index}", class: "nf-tray-jack {port.cable.class()}" }
                    }
                }
                span { class: "nf-led is-{phase.code()}" }
                span { class: "nf-tray-name", "{node.title}" }
                span { class: "nf-tray-system", "{node.system}" }
                span { class: "nf-tray-jacks is-out",
                    for (index, port) in node.outputs.iter().enumerate() {
                        span { key: "{index}", class: "nf-tray-jack {port.cable.class()}" }
                    }
                }
            }
            if stalled {
                button {
                    class: "nf-button is-primary nf-tray-resume",
                    r#type: "button",
                    title: "Запустити «{node.title}» знову з місця, де зупинився",
                    disabled: busy,
                    onclick: move |_| on_resume.call(id),
                    if busy {
                        span { class: "nf-spinner" }
                    } else {
                        RefreshIcon {}
                    }
                    "Продовжити"
                }
            }
        }
    }
}

#[component]
fn NodeGhost(node: Node, x: f64, y: f64, zoom: f64, void: bool) -> Element {
    let mut class = format!("nf-ghost {}", node.tint.tint());
    if void {
        class.push_str(" is-void");
    }

    rsx! {
        div {
            class: "{class}",
            style: "width: {node.width}px; transform: translate({x}px, {y}px) scale({zoom});",
            "aria-hidden": "true",
            div { class: "nf-node-head",
                span { class: "nf-led is-{node.phase.code()}" }
                span { class: "nf-node-title", "{node.title}" }
                span { class: "nf-node-system", "{node.system}" }
            }
            div { class: "nf-ports",
                GhostPorts { ports: node.inputs.clone(), inbound: true }
                GhostPorts { ports: node.outputs.clone(), inbound: false }
            }
        }
    }
}

#[component]
fn GhostPorts(ports: Vec<Port>, inbound: bool) -> Element {
    let side = if inbound { "is-in" } else { "is-out" };

    rsx! {
        div { class: "nf-port-column",
            for (index, port) in ports.into_iter().enumerate() {
                span { key: "{index}", class: "nf-port {side} {port.cable.class()}",
                    if inbound {
                        span { class: "nf-jack" }
                        "{port.label}"
                    } else {
                        "{port.label}"
                        span { class: "nf-jack" }
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
    let passed = node.passed();
    if passed {
        class.push_str(" is-passed");
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
                if event.trigger_button() != Some(MouseButton::Primary) {
                    return;
                }
                event.stop_propagation();
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
            if passed {
                span { class: "nf-passed", title: "Етап пройдено", CheckIcon {} }
            }
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
        Body::Frames { images } => rsx! {
            div { class: "nf-frames",
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
    insets: Insets,
    focus: Option<(f64, f64)>,
    framing: Framing,
) {
    let Some((_, _, width, height)) = rect_of(slot) else {
        return;
    };
    if bounds.right <= bounds.left || width < 2.0 || height < 2.0 {
        return;
    }
    let room_width = (width - insets.right - FIT_MARGIN * 2.0).max(1.0);
    let room_height = (height - insets.bottom - FIT_MARGIN * 2.0).max(1.0);
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

fn centre_on(slot: &ElementSlot, mut view: Signal<View>, point: (f64, f64), insets: Insets) {
    let Some((_, _, width, height)) = rect_of(slot) else {
        return;
    };
    let zoom = view.peek().zoom.max(READABLE_ZOOM);
    view.set(View {
        x: (width - insets.right).max(1.0) / 2.0 - point.0 * zoom,
        y: (height - insets.bottom).max(1.0) / 2.0 - point.1 * zoom,
        zoom,
    });
}

fn keep_visible(slot: &ElementSlot, mut view: Signal<View>, left: f64, right: f64, insets: Insets) {
    let Some((_, _, width, _)) = rect_of(slot) else {
        return;
    };
    let origin = *view.peek();
    let screen_left = origin.x + left * origin.zoom;
    let screen_right = origin.x + right * origin.zoom;
    let limit = width - insets.right - FIT_MARGIN;
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

fn zoom_center(slot: &ElementSlot, mut view: Signal<View>, zoom: f64, insets: Insets) {
    let Some((_, _, width, height)) = rect_of(slot) else {
        return;
    };
    let origin = *view.peek();
    let anchor_x = (width - insets.right).max(1.0) / 2.0;
    let anchor_y = (height - insets.bottom).max(1.0) / 2.0;
    view.set(zoom_around(origin, zoom, anchor_x, anchor_y));
}

fn step_zoom(slot: &ElementSlot, view: Signal<View>, insets: Insets, key: ZoomKey) {
    let zoom = view.peek().zoom;
    let target = match key {
        ZoomKey::In => zoom * ZOOM_STEP,
        ZoomKey::Out => zoom / ZOOM_STEP,
        ZoomKey::Actual => 1.0,
    };
    zoom_center(slot, view, target, insets);
}

fn zoom_key(code: &Code) -> Option<ZoomKey> {
    match code {
        Code::Equal | Code::NumpadAdd => Some(ZoomKey::In),
        Code::Minus | Code::NumpadSubtract => Some(ZoomKey::Out),
        Code::Digit0 | Code::Numpad0 => Some(ZoomKey::Actual),
        _ => None,
    }
}

fn wheel_factor(lift: f64, pinch: bool) -> f64 {
    let push = if pinch { lift * PINCH_BOOST } else { lift };
    (-push.clamp(-WHEEL_LIMIT, WHEEL_LIMIT) * WHEEL_RATE).exp()
}

fn grid_size(zoom: f64) -> f64 {
    let mut size = GRID_STEP * zoom;
    while size < MIN_GRID {
        size *= 2.0;
    }
    size
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

fn inside(slot: &ElementSlot, point: (f64, f64)) -> bool {
    rect_of(slot).is_some_and(|(left, top, width, height)| {
        (left..=left + width).contains(&point.0) && (top..=top + height).contains(&point.1)
    })
}

fn visible_part(graph: &Graph, stowed: &Stowed) -> Graph {
    let nodes: Vec<Node> = graph
        .nodes
        .iter()
        .filter(|node| !stowed.contains(&node.id.key()))
        .cloned()
        .collect();
    let shown: HashSet<NodeId> = nodes.iter().map(|node| node.id).collect();
    let wires = graph
        .wires
        .iter()
        .filter(|wire| shown.contains(&wire.from) && shown.contains(&wire.to))
        .cloned()
        .collect();
    Graph { nodes, wires }
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

fn stored(key: &str) -> Option<String> {
    local_storage().and_then(|storage| storage.get_item(key).ok().flatten())
}

fn store(key: &str, value: &str) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(key, value);
    }
}

fn load_stowed() -> Stowed {
    stored(TRAY_KEY)
        .unwrap_or_default()
        .split(TRAY_SEPARATOR)
        .filter(|key| !key.is_empty())
        .map(str::to_owned)
        .collect()
}

fn save_stowed(stowed: &Stowed) {
    let encoded = stowed
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(&TRAY_SEPARATOR.to_string());
    store(TRAY_KEY, &encoded);
}

fn load_tray_open() -> bool {
    stored(TRAY_OPEN_KEY).as_deref() != Some(TRAY_CLOSED)
}

fn save_tray_open(open: bool) {
    store(TRAY_OPEN_KEY, if open { TRAY_OPENED } else { TRAY_CLOSED });
}

