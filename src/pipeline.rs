use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::f64::consts::TAU;
use std::ops::{Add, Mul, Sub};
use std::rc::Rc;

use dioxus::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{CanvasRenderingContext2d, Document, HtmlCanvasElement, Path2d, Window};

struct Station {
    code: &'static str,
    title: &'static str,
    note: &'static str,
    angle: f64,
    tint: Rgb,
    icon: &'static [&'static str],
}

const STATIONS: [Station; 5] = [
    Station {
        code: "01",
        title: "ПРОМПТ",
        note: "Створення ідей",
        angle: 146.0,
        tint: SKY,
        icon: &LAYERS_ICON,
    },
    Station {
        code: "02",
        title: "КОНЦЕПТ",
        note: "Розробка концепції",
        angle: 188.0,
        tint: VIOLET,
        icon: &CHIP_ICON,
    },
    Station {
        code: "03",
        title: "МОДЕЛЬ",
        note: "3D-модель",
        angle: 252.0,
        tint: PURPLE,
        icon: &BOX_ICON,
    },
    Station {
        code: "04",
        title: "МАТЕРІАЛ",
        note: "Матеріали та текстури",
        angle: 336.0,
        tint: VIOLET,
        icon: &DATABASE_ICON,
    },
    Station {
        code: "05",
        title: "АНІМАЦІЯ",
        note: "Анімація об’єктів",
        angle: 32.0,
        tint: CYAN,
        icon: &PLAY_ICON,
    },
];

const CORE_CODE: &str = "06";
const CORE_TITLE: &str = "ЗБІРКА";
const CORE_NOTE: &str = "Фінальна сцена";

const LAYERS_ICON: [&str; 3] = [
    "m12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z",
    "m22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65",
    "m22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65",
];
const CHIP_ICON: [&str; 10] = [
    "M6 4h12a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z",
    "M10 9h4a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1h-4a1 1 0 0 1-1-1v-4a1 1 0 0 1 1-1z",
    "M15 2v2",
    "M15 20v2",
    "M2 15h2",
    "M2 9h2",
    "M20 15h2",
    "M20 9h2",
    "M9 2v2",
    "M9 20v2",
];
const BOX_ICON: [&str; 3] = [
    "M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z",
    "m3.3 7 8.7 5 8.7-5",
    "M12 22V12",
];
const DATABASE_ICON: [&str; 3] = [
    "M3 5a9 3 0 1 0 18 0a9 3 0 1 0-18 0",
    "M3 5v14a9 3 0 0 0 18 0V5",
    "M3 12a9 3 0 0 0 18 0",
];
const PLAY_ICON: [&str; 1] = ["M6 3l14 9-14 9z"];

const ORBIT_RADIUS: f64 = 3.6;
const STATION_HALF: f64 = 0.42;
const CORE_HALF: f64 = 1.0;
const INNER_HALF: f64 = 0.42;
const FLOOR_Y: f64 = -1.6;
const ARC_RISE: f64 = 0.7;
const ARC_SAMPLES: usize = 32;
const RING_SAMPLES: usize = 120;
const DUST_COUNT: usize = 150;
const LEADER: f64 = 30.0;
const TAG_GAP: f64 = 8.0;
const TAG_EDGE: f64 = 6.0;
const COMPACT_WIDTH: f64 = 720.0;
const COMPACT_HEIGHT: f64 = 560.0;

const TRAVEL_TIME: f64 = 0.9;
const PROCESS_TIME: f64 = 0.6;
const STAGE_TIME: f64 = TRAVEL_TIME * 2.0 + PROCESS_TIME;
const BUILD_TIME: f64 = 1.8;
const VISITS_TIME: f64 = STAGE_TIME * STATIONS.len() as f64;
const CYCLE_TIME: f64 = VISITS_TIME + BUILD_TIME;
const DONE_HEAT: f64 = 0.5;
const FROZEN_TIME: f64 = STAGE_TIME * 2.0 + TRAVEL_TIME + 0.3;

const BASE_PITCH: f64 = 0.44;
const CAMERA_DISTANCE: f64 = 10.0;
const TOP_RESERVED: f64 = 64.0;
const BOTTOM_RESERVED: f64 = 170.0;
const LABEL_SPACE: f64 = 92.0;
const LABEL_SIDE: f64 = 104.0;

struct TagStyle {
    number_font: &'static str,
    title_font: &'static str,
    note_font: Option<&'static str>,
    height: f64,
    padding: f64,
    title_baseline: f64,
    note_baseline: f64,
}

const REGULAR_TAG: TagStyle = TagStyle {
    number_font: "700 16px Manrope, \"Segoe UI\", sans-serif",
    title_font: "700 14px Manrope, \"Segoe UI\", sans-serif",
    note_font: Some("500 11.5px Manrope, \"Segoe UI\", sans-serif"),
    height: 50.0,
    padding: 14.0,
    title_baseline: 24.0,
    note_baseline: 39.0,
};

const COMPACT_TAG: TagStyle = TagStyle {
    number_font: "700 13px Manrope, \"Segoe UI\", sans-serif",
    title_font: "700 12px Manrope, \"Segoe UI\", sans-serif",
    note_font: None,
    height: 30.0,
    padding: 10.0,
    title_baseline: 19.5,
    note_baseline: 0.0,
};

const WHITE: Rgb = Rgb(255.0, 255.0, 255.0);
const BLUE: Rgb = Rgb(70.0, 120.0, 255.0);
const SKY: Rgb = Rgb(56.0, 160.0, 255.0);
const VIOLET: Rgb = Rgb(139.0, 108.0, 255.0);
const PURPLE: Rgb = Rgb(176.0, 108.0, 255.0);
const CYAN: Rgb = Rgb(45.0, 212.0, 238.0);
const CORE_TINT: Rgb = Rgb(110.0, 100.0, 255.0);
const HAZE: Rgb = Rgb(80.0, 54.0, 200.0);
const DEEP: Rgb = Rgb(30.0, 36.0, 110.0);
const DUST: Rgb = Rgb(200.0, 210.0, 255.0);
const PANEL: Rgb = Rgb(6.0, 9.0, 22.0);
const NOTE_INK: Rgb = Rgb(190.0, 198.0, 220.0);
const ORIGIN: Vec3 = vec3(0.0, 0.0, 0.0);

const CUBE_FACES: [[usize; 4]; 6] = [
    [0, 2, 6, 4],
    [1, 3, 7, 5],
    [0, 1, 5, 4],
    [2, 3, 7, 6],
    [0, 1, 3, 2],
    [4, 5, 7, 6],
];

const CUBE_EDGES: [[usize; 2]; 12] = [
    [0, 1],
    [2, 3],
    [4, 5],
    [6, 7],
    [0, 2],
    [1, 3],
    [4, 6],
    [5, 7],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
];

type Pointer = Rc<Cell<Option<(f64, f64)>>>;

#[component]
pub fn PipelineCanvas() -> Element {
    let pointer = use_hook(|| Pointer::new(Cell::new(None)));
    let runner = use_hook(|| Rc::new(RefCell::new(None::<Runner>)));

    let cleanup = Rc::clone(&runner);
    use_drop(move || {
        if let Some(active) = cleanup.borrow_mut().take() {
            active.stop();
        }
    });

    let mounted_pointer = Rc::clone(&pointer);
    let moved_pointer = Rc::clone(&pointer);

    rsx! {
        canvas {
            class: "pipeline-canvas",
            "aria-hidden": "true",
            onmounted: move |event| {
                let canvas = event
                    .data()
                    .downcast::<web_sys::Element>()
                    .and_then(|element| element.clone().dyn_into::<HtmlCanvasElement>().ok());
                if let Some(canvas) = canvas {
                    if let Some(previous) = runner.borrow_mut().take() {
                        previous.stop();
                    }
                    *runner.borrow_mut() = Runner::start(canvas, Rc::clone(&mounted_pointer));
                }
            },
            onmousemove: move |event| {
                let point = event.element_coordinates();
                moved_pointer.set(Some((point.x, point.y)));
            },
            onmouseleave: move |_| pointer.set(None),
        }
    }
}

struct Runner {
    window: Window,
    frame: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>,
    request: Rc<Cell<i32>>,
}

impl Runner {
    fn start(canvas: HtmlCanvasElement, pointer: Pointer) -> Option<Self> {
        let window = web_sys::window()?;
        let document = window.document()?;
        let context = context_of(&canvas)?;
        let (glow_canvas, glow) = offscreen(&document)?;
        let (bloom_canvas, bloom) = offscreen(&document)?;
        let reduced_motion = window
            .match_media("(prefers-reduced-motion: reduce)")
            .ok()
            .flatten()
            .is_some_and(|query| query.matches());
        let icons = STATIONS
            .iter()
            .map(|station| {
                station
                    .icon
                    .iter()
                    .filter_map(|path| Path2d::new_with_path_string(path).ok())
                    .collect()
            })
            .collect();
        let mut scene = Scene {
            window: window.clone(),
            canvas,
            context,
            glow_canvas,
            glow,
            bloom_canvas,
            bloom,
            pointer,
            tilt: (0.0, 0.0),
            reduced_motion,
            icons,
            dust: dust_field(),
        };

        let frame: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
        let request = Rc::new(Cell::new(0));
        let next_frame = Rc::clone(&frame);
        let next_request = Rc::clone(&request);
        let scheduler = window.clone();
        *frame.borrow_mut() = Some(Closure::new(move |now: f64| {
            scene.render(now / 1000.0);
            if let Some(callback) = next_frame.borrow().as_ref() {
                if let Ok(id) = scheduler.request_animation_frame(callback.as_ref().unchecked_ref()) {
                    next_request.set(id);
                }
            }
        }));

        let first = frame
            .borrow()
            .as_ref()
            .map(|callback| window.request_animation_frame(callback.as_ref().unchecked_ref()));
        request.set(first?.ok()?);
        Some(Self {
            window,
            frame,
            request,
        })
    }

    fn stop(self) {
        let _ = self.window.cancel_animation_frame(self.request.get());
        self.frame.borrow_mut().take();
    }
}

fn context_of(canvas: &HtmlCanvasElement) -> Option<CanvasRenderingContext2d> {
    canvas
        .get_context("2d")
        .ok()??
        .dyn_into::<CanvasRenderingContext2d>()
        .ok()
}

fn offscreen(document: &Document) -> Option<(HtmlCanvasElement, CanvasRenderingContext2d)> {
    let canvas = document
        .create_element("canvas")
        .ok()?
        .dyn_into::<HtmlCanvasElement>()
        .ok()?;
    let context = context_of(&canvas)?;
    Some((canvas, context))
}

struct Scene {
    window: Window,
    canvas: HtmlCanvasElement,
    context: CanvasRenderingContext2d,
    glow_canvas: HtmlCanvasElement,
    glow: CanvasRenderingContext2d,
    bloom_canvas: HtmlCanvasElement,
    bloom: CanvasRenderingContext2d,
    pointer: Pointer,
    tilt: (f64, f64),
    reduced_motion: bool,
    icons: Vec<Vec<Path2d>>,
    dust: Vec<(Vec3, f64)>,
}

enum Body {
    Station(usize),
    Core,
}

impl Scene {
    fn render(&mut self, seconds: f64) {
        let width = f64::from(self.canvas.client_width());
        let height = f64::from(self.canvas.client_height());
        if width < 2.0 || height < 2.0 {
            return;
        }
        let ratio = self.window.device_pixel_ratio();
        let pixel_width = (width * ratio).round() as u32;
        let pixel_height = (height * ratio).round() as u32;
        if self.canvas.width() != pixel_width || self.canvas.height() != pixel_height {
            self.canvas.set_width(pixel_width);
            self.canvas.set_height(pixel_height);
            self.glow_canvas.set_width(pixel_width);
            self.glow_canvas.set_height(pixel_height);
            self.bloom_canvas.set_width((pixel_width / 2).max(1));
            self.bloom_canvas.set_height((pixel_height / 2).max(1));
        }

        let time = if self.reduced_motion { FROZEN_TIME } else { seconds };
        let target = self.pointer.get().map_or((0.0, 0.0), |(x, y)| {
            ((x / width) * 2.0 - 1.0, (y / height) * 2.0 - 1.0)
        });
        self.tilt.0 += (target.0 - self.tilt.0) * 0.05;
        self.tilt.1 += (target.1 - self.tilt.1) * 0.05;
        let sway = if self.reduced_motion {
            0.0
        } else {
            (time * 0.1).sin() * 0.05
        };
        let camera = frame_camera(
            width,
            height,
            sway + self.tilt.0 * 0.12,
            BASE_PITCH + self.tilt.1 * 0.05,
        );

        for context in [&self.context, &self.glow] {
            let _ = context.set_transform(ratio, 0.0, 0.0, ratio, 0.0, 0.0);
            context.clear_rect(0.0, 0.0, width, height);
            context.set_line_join("round");
            context.set_line_cap("round");
        }

        let painter = Painter {
            main: &self.context,
            glow: &self.glow,
            camera,
            time,
            width,
            height,
        };
        let step = Step::at(time);
        let positions: Vec<Vec3> = (0..STATIONS.len())
            .map(|index| station_position(index, time))
            .collect();

        painter.haze();
        painter.floor_web();
        painter.platform(step.core_heat());
        painter.orbits();
        painter.dust(&self.dust);

        let mut bodies: Vec<(f64, Body)> = positions
            .iter()
            .enumerate()
            .map(|(index, position)| (camera.to_view(*position).z, Body::Station(index)))
            .collect();
        bodies.push((camera.to_view(ORIGIN).z, Body::Core));
        bodies.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, body) in &bodies {
            match body {
                Body::Station(index) => {
                    painter.station(*index, positions[*index], &step, &self.icons[*index]);
                }
                Body::Core => painter.core(&step),
            }
        }
        painter.orbit_dots();

        self.composite(width, height);

        let core_y = camera.project(ORIGIN).y;
        let lift = vec3(0.0, STATION_HALF * 1.55, 0.0);
        let mut tags: Vec<Tag> = STATIONS
            .iter()
            .enumerate()
            .map(|(index, station)| {
                let below = camera.project(positions[index]).y > core_y;
                let anchor = if below {
                    positions[index] - lift
                } else {
                    positions[index] + lift
                };
                Tag {
                    code: station.code,
                    title: station.title,
                    note: station.note,
                    tint: station.tint,
                    anchor: camera.project(anchor),
                    below,
                    heat: step.heat(index),
                }
            })
            .collect();
        tags.push(Tag {
            code: CORE_CODE,
            title: CORE_TITLE,
            note: CORE_NOTE,
            tint: BLUE,
            anchor: camera.project(vec3(0.0, CORE_HALF * 1.75, 0.0)),
            below: false,
            heat: step.core_heat(),
        });
        painter.tags(&tags);
    }

    fn composite(&self, width: f64, height: f64) {
        let bloom_width = f64::from(self.bloom_canvas.width());
        let bloom_height = f64::from(self.bloom_canvas.height());
        let _ = self.bloom.set_transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
        self.bloom.clear_rect(0.0, 0.0, bloom_width, bloom_height);
        let _ = self.bloom.draw_image_with_html_canvas_element_and_dw_and_dh(
            &self.glow_canvas,
            0.0,
            0.0,
            bloom_width,
            bloom_height,
        );

        let main = &self.context;
        let _ = main.set_global_composite_operation("lighter");
        let _ = main.draw_image_with_html_canvas_element_and_dw_and_dh(
            &self.glow_canvas,
            0.0,
            0.0,
            width,
            height,
        );
        for (blur, strength) in [("blur(5px)", 1.0), ("blur(16px)", 0.85)] {
            main.set_filter(blur);
            main.set_global_alpha(strength);
            let _ = main.draw_image_with_html_canvas_element_and_dw_and_dh(
                &self.bloom_canvas,
                0.0,
                0.0,
                width,
                height,
            );
        }
        main.set_filter("none");
        main.set_global_alpha(1.0);
        let _ = main.set_global_composite_operation("source-over");
    }
}

#[derive(Clone, Copy)]
enum Step {
    Visit { station: usize, phase: f64 },
    Build { phase: f64 },
}

impl Step {
    fn at(time: f64) -> Self {
        let cycle = time.rem_euclid(CYCLE_TIME);
        if cycle < VISITS_TIME {
            let station = ((cycle / STAGE_TIME) as usize).min(STATIONS.len() - 1);
            Self::Visit {
                station,
                phase: cycle - station as f64 * STAGE_TIME,
            }
        } else {
            Self::Build {
                phase: cycle - VISITS_TIME,
            }
        }
    }

    fn heat(&self, index: usize) -> f64 {
        match *self {
            Self::Build { .. } => DONE_HEAT,
            Self::Visit { station, phase } => match index.cmp(&station) {
                Ordering::Less => DONE_HEAT,
                Ordering::Greater => 0.0,
                Ordering::Equal if phase < TRAVEL_TIME => 0.25 * phase / TRAVEL_TIME,
                Ordering::Equal if phase < TRAVEL_TIME + PROCESS_TIME => 1.0,
                Ordering::Equal => {
                    let back = (phase - TRAVEL_TIME - PROCESS_TIME) / TRAVEL_TIME;
                    1.0 - (1.0 - DONE_HEAT) * back
                }
            },
        }
    }

    fn packet(&self, index: usize) -> Option<(f64, bool)> {
        match *self {
            Self::Visit { station, phase } if station == index => {
                if phase < TRAVEL_TIME {
                    Some((ease_in_out(phase / TRAVEL_TIME), false))
                } else if phase >= TRAVEL_TIME + PROCESS_TIME {
                    let back = (phase - TRAVEL_TIME - PROCESS_TIME) / TRAVEL_TIME;
                    Some((1.0 - ease_in_out(back), true))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn core_heat(&self) -> f64 {
        match *self {
            Self::Build { phase } => 1.0 - 0.7 * (phase / BUILD_TIME),
            Self::Visit { phase, .. } => 0.25 + 0.35 * (-phase * 4.0).exp(),
        }
    }
}

struct Tag {
    code: &'static str,
    title: &'static str,
    note: &'static str,
    tint: Rgb,
    anchor: Point,
    below: bool,
    heat: f64,
}

struct Painter<'a> {
    main: &'a CanvasRenderingContext2d,
    glow: &'a CanvasRenderingContext2d,
    camera: Camera,
    time: f64,
    width: f64,
    height: f64,
}

impl Painter<'_> {
    fn haze(&self) {
        let center = self.camera.project(ORIGIN);
        let radius = self.width.max(self.height) * 0.72;
        if let Ok(gradient) = self
            .main
            .create_radial_gradient(center.x, center.y, 0.0, center.x, center.y, radius)
        {
            let _ = gradient.add_color_stop(0.0, &HAZE.with_alpha(0.24));
            let _ = gradient.add_color_stop(0.45, &DEEP.with_alpha(0.09));
            let _ = gradient.add_color_stop(1.0, &DEEP.with_alpha(0.0));
            self.main.set_fill_style_canvas_gradient(&gradient);
            self.main.fill_rect(0.0, 0.0, self.width, self.height);
        }
    }

    fn floor_web(&self) {
        let color = VIOLET.with_alpha(0.06);
        for spoke in 0..16 {
            let (sin, cos) = (f64::from(spoke) * TAU / 16.0).sin_cos();
            polyline(
                self.main,
                &self.camera,
                &[
                    vec3(2.5 * cos, FLOOR_Y, 2.5 * sin),
                    vec3(6.8 * cos, FLOOR_Y, 6.8 * sin),
                ],
                &color,
                1.0,
            );
        }
        for radius in [ORBIT_RADIUS, 5.3] {
            polyline(self.main, &self.camera, &ring(radius, FLOOR_Y, 0.0, 0.0), &color, 1.0);
        }
    }

    fn platform(&self, heat: f64) {
        let center = self.camera.project(vec3(0.0, FLOOR_Y, 0.0));
        let rim = self.camera.project(vec3(2.6, FLOOR_Y, 0.0));
        let front = self.camera.project(vec3(0.0, FLOOR_Y, -2.6));
        let radius_x = (rim.x - center.x).abs().max(1.0);
        let radius_y = (front.y - center.y).abs().max(1.0);
        self.main.save();
        let _ = self.main.translate(center.x, center.y);
        let _ = self.main.scale(1.0, radius_y / radius_x);
        if let Ok(gradient) = self
            .main
            .create_radial_gradient(0.0, 0.0, 0.0, 0.0, 0.0, radius_x)
        {
            let _ = gradient.add_color_stop(0.0, &BLUE.with_alpha(0.32 + 0.2 * heat));
            let _ = gradient.add_color_stop(0.55, &VIOLET.with_alpha(0.1));
            let _ = gradient.add_color_stop(1.0, &VIOLET.with_alpha(0.0));
            self.main.set_fill_style_canvas_gradient(&gradient);
            self.main
                .fill_rect(-radius_x, -radius_x, radius_x * 2.0, radius_x * 2.0);
        }
        self.main.restore();

        let lift = FLOOR_Y + 0.02;
        for (radius, color, alpha, width) in [
            (1.25, BLUE.mix(WHITE, 0.35), 0.75 + 0.25 * heat, 2.0),
            (1.62, VIOLET, 0.5, 1.4),
            (2.25, BLUE, 0.28, 1.0),
        ] {
            polyline(
                self.glow,
                &self.camera,
                &ring(radius, lift, 0.0, 0.0),
                &color.with_alpha(alpha),
                width,
            );
        }
    }

    fn orbits(&self) {
        polyline(
            self.glow,
            &self.camera,
            &ring(ORBIT_RADIUS, 0.0, 0.0, 0.0),
            &VIOLET.mix(BLUE, 0.4).with_alpha(0.42),
            1.1,
        );
        polyline(
            self.glow,
            &self.camera,
            &ring(4.9, 0.0, 0.17, -0.1),
            &BLUE.with_alpha(0.22),
            1.0,
        );
        polyline(
            self.glow,
            &self.camera,
            &ring(2.45, 0.0, -0.12, 0.2),
            &VIOLET.with_alpha(0.26),
            1.0,
        );
    }

    fn orbit_dots(&self) {
        for index in 0..10 {
            let angle = f64::from(index) * TAU / 10.0 + self.time * 0.05;
            let position = vec3(ORBIT_RADIUS * angle.cos(), 0.0, ORBIT_RADIUS * angle.sin());
            let color = if index % 3 == 0 { CYAN } else { VIOLET };
            self.glow_dot(self.camera.project(position), 6.0, color, 0.85);
        }
        for index in 0..6 {
            let angle = f64::from(index) * TAU / 6.0 - self.time * 0.03;
            let position = vec3(4.9 * angle.cos(), 0.0, 4.9 * angle.sin())
                .rotate_x(0.17)
                .rotate_z(-0.1);
            self.glow_dot(self.camera.project(position), 5.0, SKY, 0.6);
        }
    }

    fn dust(&self, dust: &[(Vec3, f64)]) {
        for (position, seed) in dust {
            let drift = vec3(0.0, (self.time * 0.2 + seed * 10.0).sin() * 0.06, 0.0);
            let point = self.camera.project(*position + drift);
            if point.x < 0.0 || point.x > self.width || point.y < 0.0 || point.y > self.height {
                continue;
            }
            let twinkle = 0.55 + 0.45 * (self.time * (0.6 + seed) + seed * 20.0).sin();
            let nearness = (CAMERA_DISTANCE / point.depth).clamp(0.5, 1.3);
            let size = (0.8 + seed * 1.2) * nearness;
            self.glow
                .set_fill_style_str(&DUST.with_alpha((0.12 + 0.35 * seed) * twinkle * nearness));
            self.glow
                .fill_rect(point.x - size / 2.0, point.y - size / 2.0, size, size);
        }
    }

    fn station(&self, index: usize, position: Vec3, step: &Step, icon: &[Path2d]) {
        let station = &STATIONS[index];
        let heat = step.heat(index);
        let arc = arc_to(position);
        polyline(
            self.glow,
            &self.camera,
            &arc,
            &station.tint.mix(BLUE, 0.3).with_alpha(0.18 + 0.6 * heat),
            1.2,
        );
        if let Some((progress, returning)) = step.packet(index) {
            self.packet(&arc, progress, returning, station.tint);
        }

        let spin = self.time * 0.35 + index as f64 * 1.3;
        let offsets = cube(STATION_HALF).map(|offset| offset.rotate_y(spin).rotate_x(0.4));
        let center = self.camera.project(position);
        let size = self.camera.focal * STATION_HALF / center.depth;
        self.glow_dot(center, size * (1.6 + 0.8 * heat), station.tint, 0.22 + 0.45 * heat);
        self.glass_cube(position, offsets, station.tint, 0.55 + 0.45 * heat);
        self.icon(
            icon,
            center,
            size * 1.25,
            &station.tint.mix(WHITE, 0.55).with_alpha(0.75 + 0.25 * heat),
        );
    }

    fn core(&self, step: &Step) {
        let heat = step.core_heat();
        let spin = self.time * 0.16;
        let offsets = cube(CORE_HALF).map(|offset| offset.rotate_y(spin).rotate_x(0.5).rotate_z(0.18));
        let center = self.camera.project(ORIGIN);
        let size = self.camera.focal * CORE_HALF / center.depth;
        self.glow_dot(center, size * (1.5 + 0.4 * heat), CORE_TINT, 0.5 + 0.4 * heat);
        self.glow_dot(center, size * 0.45, WHITE.mix(VIOLET, 0.4), 0.6);

        let inner_spin = -self.time * 0.45;
        let inner = cube(INNER_HALF).map(|offset| offset.rotate_y(inner_spin).rotate_x(0.9));
        let inner_points = inner.map(|offset| self.camera.project(offset));
        self.glow.begin_path();
        for [from, to] in CUBE_EDGES {
            self.glow.move_to(inner_points[from].x, inner_points[from].y);
            self.glow.line_to(inner_points[to].x, inner_points[to].y);
        }
        self.glow
            .set_stroke_style_str(&VIOLET.mix(WHITE, 0.45).with_alpha(0.55 + 0.3 * heat));
        self.glow.set_line_width(1.2);
        self.glow.stroke();

        self.glass_cube(ORIGIN, offsets, CORE_TINT, 0.75 + 0.25 * heat);
    }

    fn glass_cube(&self, center: Vec3, offsets: [Vec3; 8], tint: Rgb, intensity: f64) {
        let view = offsets.map(|offset| self.camera.to_view(center + offset));
        let middle = self.camera.to_view(center);
        let screen = view.map(|point| self.camera.screen(point));

        let mut faces: Vec<(f64, bool, [usize; 4])> = CUBE_FACES
            .iter()
            .map(|face| {
                let corners = face.map(|vertex| view[vertex]);
                let centroid = (corners[0] + corners[1] + corners[2] + corners[3]) * 0.25;
                let mut normal = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
                if normal.dot(centroid - middle) < 0.0 {
                    normal = normal * -1.0;
                }
                (centroid.z, normal.dot(centroid) < 0.0, *face)
            })
            .collect();
        faces.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, facing, face) in &faces {
            let points = face.map(|vertex| screen[vertex]);
            self.main.begin_path();
            self.main.move_to(points[0].x, points[0].y);
            for point in &points[1..] {
                self.main.line_to(point.x, point.y);
            }
            self.main.close_path();
            if *facing {
                let top = points.iter().map(|point| point.y).fold(f64::MAX, f64::min);
                let bottom = points.iter().map(|point| point.y).fold(f64::MIN, f64::max);
                let gradient = self.main.create_linear_gradient(0.0, top, 0.0, bottom);
                let _ = gradient.add_color_stop(0.0, &tint.mix(WHITE, 0.3).with_alpha(0.3 * intensity));
                let _ = gradient.add_color_stop(1.0, &tint.with_alpha(0.08 * intensity));
                self.main.set_fill_style_canvas_gradient(&gradient);
            } else {
                self.main.set_fill_style_str(&tint.with_alpha(0.06));
            }
            self.main.fill();
        }

        let nearest = view.iter().map(|point| point.z).fold(f64::MAX, f64::min);
        let farthest = view.iter().map(|point| point.z).fold(f64::MIN, f64::max);
        let span = (farthest - nearest).max(0.001);
        self.glow.set_line_width(1.4);
        for [from, to] in CUBE_EDGES {
            let depth = (view[from].z + view[to].z) * 0.5;
            let nearness = 1.0 - 0.7 * (depth - nearest) / span;
            self.glow.begin_path();
            self.glow.move_to(screen[from].x, screen[from].y);
            self.glow.line_to(screen[to].x, screen[to].y);
            self.glow
                .set_stroke_style_str(&tint.mix(WHITE, 0.55).with_alpha(nearness * intensity));
            self.glow.stroke();
        }
        for (corner, point) in view.iter().zip(screen.iter()) {
            if corner.z - nearest < span * 0.5 {
                self.glow.set_fill_style_str(&WHITE.with_alpha(0.8 * intensity));
                self.glow.fill_rect(point.x - 1.25, point.y - 1.25, 2.5, 2.5);
            }
        }
    }

    fn packet(&self, arc: &[Vec3], progress: f64, returning: bool, tint: Rgb) {
        let behind = if returning { 1.0 } else { -1.0 };
        for (offset, fade) in [(0.0, 1.0), (0.03, 0.6), (0.06, 0.35), (0.09, 0.18)] {
            let point = self.camera.project(along(arc, progress + behind * offset));
            let radius = if offset == 0.0 { 9.0 } else { 6.0 };
            self.glow_dot(point, radius, tint, fade);
        }
    }

    fn glow_dot(&self, at: Point, radius: f64, color: Rgb, alpha: f64) {
        if radius <= 0.0 {
            return;
        }
        if let Ok(gradient) = self
            .glow
            .create_radial_gradient(at.x, at.y, 0.0, at.x, at.y, radius)
        {
            let _ = gradient.add_color_stop(0.0, &color.mix(WHITE, 0.6).with_alpha(alpha));
            let _ = gradient.add_color_stop(0.35, &color.with_alpha(alpha * 0.55));
            let _ = gradient.add_color_stop(1.0, &color.with_alpha(0.0));
            self.glow.set_fill_style_canvas_gradient(&gradient);
            self.glow
                .fill_rect(at.x - radius, at.y - radius, radius * 2.0, radius * 2.0);
        }
    }

    fn icon(&self, paths: &[Path2d], at: Point, size: f64, color: &str) {
        let scale = size / 24.0;
        self.glow.save();
        let _ = self.glow.translate(at.x - size / 2.0, at.y - size / 2.0);
        let _ = self.glow.scale(scale, scale);
        self.glow.set_line_width(1.7);
        self.glow.set_stroke_style_str(color);
        for path in paths {
            self.glow.stroke_with_path(path);
        }
        self.glow.restore();
    }

    fn tags(&self, tags: &[Tag]) {
        let style = if self.width < COMPACT_WIDTH || self.height < COMPACT_HEIGHT {
            &COMPACT_TAG
        } else {
            &REGULAR_TAG
        };
        let mut cards: Vec<Card> = tags.iter().map(|tag| self.card_for(tag, style)).collect();
        separate(&mut cards, self.width, self.height);
        for (tag, card) in tags.iter().zip(&cards) {
            self.leader(tag, card);
        }
        for (tag, card) in tags.iter().zip(&cards) {
            self.card(tag, card, style);
        }
    }

    fn card_for(&self, tag: &Tag, style: &TagStyle) -> Card {
        let context = self.main;
        context.set_font(style.number_font);
        let code_width = text_width(context, tag.code);
        context.set_font(style.title_font);
        let mut text_width_max = text_width(context, tag.title);
        if let Some(note_font) = style.note_font {
            context.set_font(note_font);
            text_width_max = text_width_max.max(text_width(context, tag.note));
        }
        let width = style.padding * 2.0 + code_width + 9.0 + text_width_max;
        Card {
            x: tag.anchor.x - width / 2.0,
            y: if tag.below {
                tag.anchor.y + LEADER
            } else {
                tag.anchor.y - LEADER - style.height
            },
            width,
            height: style.height,
            code_width,
        }
    }

    fn leader(&self, tag: &Tag, card: &Card) {
        let end_x = tag.anchor.x.clamp(card.x + 10.0, card.x + card.width - 10.0);
        let end_y = tag.anchor.y.clamp(card.y, card.y + card.height);
        let context = self.main;
        context.begin_path();
        context.move_to(tag.anchor.x, tag.anchor.y);
        context.line_to(end_x, end_y);
        context.set_line_width(1.0);
        context.set_stroke_style_str(&tag.tint.with_alpha(0.45 + 0.4 * tag.heat));
        context.stroke();
        context.set_fill_style_str(&tag.tint.mix(WHITE, 0.4).with_alpha(0.9));
        context.fill_rect(tag.anchor.x - 2.0, tag.anchor.y - 2.0, 4.0, 4.0);
    }

    fn card(&self, tag: &Tag, card: &Card, style: &TagStyle) {
        let context = self.main;
        rounded_rect(context, card.x, card.y, card.width, card.height, 8.0);
        context.set_fill_style_str(&PANEL.with_alpha(0.84));
        context.fill();
        context.set_line_width(1.0);
        context.set_stroke_style_str(&tag.tint.with_alpha(0.3 + 0.45 * tag.heat));
        context.stroke();

        context.set_text_baseline("alphabetic");
        context.set_font(style.number_font);
        context.set_fill_style_str(&tag.tint.mix(WHITE, 0.15).with_alpha(0.75 + 0.25 * tag.heat));
        let _ = context.fill_text(tag.code, card.x + style.padding, card.y + style.title_baseline);
        let text_x = card.x + style.padding + card.code_width + 9.0;
        context.set_font(style.title_font);
        context.set_fill_style_str(&WHITE.with_alpha(0.82 + 0.18 * tag.heat));
        let _ = context.fill_text(tag.title, text_x, card.y + style.title_baseline - 1.0);
        if let Some(note_font) = style.note_font {
            context.set_font(note_font);
            context.set_fill_style_str(&NOTE_INK.with_alpha(0.62));
            let _ = context.fill_text(tag.note, text_x, card.y + style.note_baseline);
        }
    }
}

#[derive(Clone, Copy)]
struct Card {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    code_width: f64,
}

fn separate(cards: &mut [Card], width: f64, height: f64) {
    for _ in 0..16 {
        let mut moved = false;
        for first in 0..cards.len() {
            for second in first + 1..cards.len() {
                let (a, b) = (cards[first], cards[second]);
                let overlap_x = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x) + TAG_GAP;
                let overlap_y = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y) + TAG_GAP;
                if overlap_x <= 0.0 || overlap_y <= 0.0 {
                    continue;
                }
                let push = overlap_y / 2.0;
                if a.y + a.height / 2.0 <= b.y + b.height / 2.0 {
                    cards[first].y -= push;
                    cards[second].y += push;
                } else {
                    cards[first].y += push;
                    cards[second].y -= push;
                }
                moved = true;
            }
        }
        for card in cards.iter_mut() {
            card.x = card.x.clamp(TAG_EDGE, (width - card.width - TAG_EDGE).max(TAG_EDGE));
            card.y = card.y.clamp(TAG_EDGE, (height - card.height - TAG_EDGE).max(TAG_EDGE));
        }
        if !moved {
            break;
        }
    }
}

fn polyline(
    context: &CanvasRenderingContext2d,
    camera: &Camera,
    points: &[Vec3],
    color: &str,
    width: f64,
) {
    let Some(first) = points.first() else {
        return;
    };
    let start = camera.project(*first);
    context.begin_path();
    context.move_to(start.x, start.y);
    for point in &points[1..] {
        let next = camera.project(*point);
        context.line_to(next.x, next.y);
    }
    context.set_line_width(width);
    context.set_stroke_style_str(color);
    context.stroke();
}

fn rounded_rect(context: &CanvasRenderingContext2d, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    context.begin_path();
    context.move_to(x + radius, y);
    let _ = context.arc_to(x + width, y, x + width, y + height, radius);
    let _ = context.arc_to(x + width, y + height, x, y + height, radius);
    let _ = context.arc_to(x, y + height, x, y, radius);
    let _ = context.arc_to(x, y, x + width, y, radius);
    context.close_path();
}

fn text_width(context: &CanvasRenderingContext2d, text: &str) -> f64 {
    context
        .measure_text(text)
        .map_or(text.chars().count() as f64 * 8.0, |metrics| metrics.width())
}

#[derive(Clone, Copy)]
struct Camera {
    yaw: f64,
    pitch: f64,
    focal: f64,
    center_x: f64,
    center_y: f64,
}

#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
    depth: f64,
}

impl Camera {
    fn to_view(&self, point: Vec3) -> Vec3 {
        let rotated = point.rotate_y(self.yaw).rotate_x(self.pitch);
        vec3(rotated.x, rotated.y, rotated.z + CAMERA_DISTANCE)
    }

    fn screen(&self, view: Vec3) -> Point {
        let scale = self.focal / view.z.max(0.1);
        Point {
            x: self.center_x + view.x * scale,
            y: self.center_y - view.y * scale,
            depth: view.z,
        }
    }

    fn project(&self, point: Vec3) -> Point {
        self.screen(self.to_view(point))
    }
}

fn frame_camera(width: f64, height: f64, yaw: f64, pitch: f64) -> Camera {
    let probe = Camera {
        yaw: 0.0,
        pitch: BASE_PITCH,
        focal: 1.0,
        center_x: 0.0,
        center_y: 0.0,
    };
    let reach = STATION_HALF * 1.6;
    let mut extremes: Vec<Point> = (0..STATIONS.len())
        .flat_map(|index| {
            let position = station_position(index, 0.0);
            [
                position + vec3(-reach, reach, 0.0),
                position + vec3(reach, -reach, 0.0),
            ]
        })
        .map(|point| probe.project(point))
        .collect();
    extremes.push(probe.project(vec3(0.0, CORE_HALF * 1.8, 0.0)));
    extremes.push(probe.project(vec3(0.0, FLOOR_Y, -2.3)));
    let (min_x, max_x) = extremes
        .iter()
        .fold((f64::MAX, f64::MIN), |(low, high), point| (low.min(point.x), high.max(point.x)));
    let (min_y, max_y) = extremes
        .iter()
        .fold((f64::MAX, f64::MIN), |(low, high), point| (low.min(point.y), high.max(point.y)));
    let usable_width = (width - 2.0 * LABEL_SIDE).max(width * 0.5);
    let top = TOP_RESERVED + LABEL_SPACE;
    let usable_height = (height - BOTTOM_RESERVED - LABEL_SPACE - top).max(height * 0.3);
    let focal = (usable_width / (max_x - min_x)).min(usable_height / (max_y - min_y));
    Camera {
        yaw,
        pitch,
        focal,
        center_x: width / 2.0 - (min_x + max_x) / 2.0 * focal,
        center_y: top + usable_height / 2.0 - (min_y + max_y) / 2.0 * focal,
    }
}

fn station_position(index: usize, time: f64) -> Vec3 {
    let angle = STATIONS[index].angle.to_radians();
    let bob = (time * 0.9 + index as f64 * 1.7).sin() * 0.08;
    vec3(ORBIT_RADIUS * angle.cos(), bob, ORBIT_RADIUS * angle.sin())
}

fn arc_to(target: Vec3) -> Vec<Vec3> {
    let direction = vec3(target.x, 0.0, target.z).normalized();
    let start = direction * (CORE_HALF * 1.05);
    let end = target - direction * (STATION_HALF * 1.4);
    let control = (start + end) * 0.5 + vec3(0.0, ARC_RISE, 0.0);
    (0..=ARC_SAMPLES)
        .map(|step| {
            let t = step as f64 / ARC_SAMPLES as f64;
            let rest = 1.0 - t;
            start * (rest * rest) + control * (2.0 * rest * t) + end * (t * t)
        })
        .collect()
}

fn along(arc: &[Vec3], progress: f64) -> Vec3 {
    let last = arc.len() - 1;
    let scaled = progress.clamp(0.0, 1.0) * last as f64;
    let index = (scaled.floor() as usize).min(last - 1);
    let local = scaled - index as f64;
    arc[index] + (arc[index + 1] - arc[index]) * local
}

fn ring(radius: f64, height: f64, tilt_x: f64, tilt_z: f64) -> Vec<Vec3> {
    (0..=RING_SAMPLES)
        .map(|step| {
            let angle = step as f64 * TAU / RING_SAMPLES as f64;
            vec3(radius * angle.cos(), height, radius * angle.sin())
                .rotate_x(tilt_x)
                .rotate_z(tilt_z)
        })
        .collect()
}

fn dust_field() -> Vec<(Vec3, f64)> {
    let mut seed: u32 = 0x9e37_79b9;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        f64::from(seed) / f64::from(u32::MAX)
    };
    (0..DUST_COUNT)
        .map(|_| {
            let position = vec3(next() * 16.0 - 8.0, next() * 6.0 - 2.4, next() * 12.0 - 5.0);
            (position, next())
        })
        .collect()
}

fn cube(half: f64) -> [Vec3; 8] {
    std::array::from_fn(|corner| {
        let axis = |bit: usize| if corner & bit == 0 { -half } else { half };
        vec3(axis(1), axis(2), axis(4))
    })
}

fn lerp(from: f64, to: f64, amount: f64) -> f64 {
    from + (to - from) * amount
}

fn ease_in_out(amount: f64) -> f64 {
    let t = amount.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Clone, Copy)]
struct Rgb(f64, f64, f64);

impl Rgb {
    fn mix(self, other: Rgb, amount: f64) -> Rgb {
        Rgb(
            lerp(self.0, other.0, amount),
            lerp(self.1, other.1, amount),
            lerp(self.2, other.2, amount),
        )
    }

    fn with_alpha(self, alpha: f64) -> String {
        format!(
            "rgba({:.0}, {:.0}, {:.0}, {:.3})",
            self.0,
            self.1,
            self.2,
            alpha.clamp(0.0, 1.0)
        )
    }
}

#[derive(Clone, Copy)]
struct Vec3 {
    x: f64,
    y: f64,
    z: f64,
}

const fn vec3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 { x, y, z }
}

impl Add for Vec3 {
    type Output = Vec3;

    fn add(self, other: Vec3) -> Vec3 {
        vec3(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;

    fn sub(self, other: Vec3) -> Vec3 {
        vec3(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Vec3;

    fn mul(self, factor: f64) -> Vec3 {
        vec3(self.x * factor, self.y * factor, self.z * factor)
    }
}

impl Vec3 {
    fn dot(self, other: Vec3) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    fn cross(self, other: Vec3) -> Vec3 {
        vec3(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    fn normalized(self) -> Vec3 {
        let length = self.dot(self).sqrt();
        if length > 0.0 { self * (1.0 / length) } else { self }
    }

    fn rotate_y(self, angle: f64) -> Vec3 {
        let (sin, cos) = angle.sin_cos();
        vec3(self.x * cos + self.z * sin, self.y, self.z * cos - self.x * sin)
    }

    fn rotate_x(self, angle: f64) -> Vec3 {
        let (sin, cos) = angle.sin_cos();
        vec3(self.x, self.y * cos + self.z * sin, self.z * cos - self.y * sin)
    }

    fn rotate_z(self, angle: f64) -> Vec3 {
        let (sin, cos) = angle.sin_cos();
        vec3(self.x * cos - self.y * sin, self.x * sin + self.y * cos, self.z)
    }
}
