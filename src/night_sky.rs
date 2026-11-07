use dioxus::prelude::*;

static NIGHT_ART: Asset = asset!("/assets/images/auth/night-city.webp");

#[derive(Clone, Copy, PartialEq)]
struct Star {
    code: &'static str,
    title: &'static str,
    note: &'static str,
    x: f64,
    y: f64,
    label_above: bool,
    label_left: bool,
}

const STARS: [Star; 6] = [
    Star {
        code: "01",
        title: "ПРОМПТ",
        note: "Створення ідей",
        x: 6.0,
        y: 72.0,
        label_above: false,
        label_left: false,
    },
    Star {
        code: "02",
        title: "КОНЦЕПТ",
        note: "Розробка концепції",
        x: 21.0,
        y: 46.0,
        label_above: true,
        label_left: false,
    },
    Star {
        code: "03",
        title: "МОДЕЛЬ",
        note: "3D-модель",
        x: 36.0,
        y: 58.0,
        label_above: false,
        label_left: false,
    },
    Star {
        code: "04",
        title: "МАТЕРІАЛ",
        note: "Матеріали та текстури",
        x: 48.0,
        y: 24.0,
        label_above: true,
        label_left: true,
    },
    Star {
        code: "05",
        title: "АНІМАЦІЯ",
        note: "Анімація об’єктів",
        x: 67.0,
        y: 14.0,
        label_above: true,
        label_left: false,
    },
    Star {
        code: "06",
        title: "ЗБІРКА",
        note: "Фінальна сцена",
        x: 84.0,
        y: 40.0,
        label_above: false,
        label_left: true,
    },
];

#[component]
pub fn NightSky() -> Element {
    let last = STARS.len() - 1;
    rsx! {
        img {
            class: "night-art",
            src: NIGHT_ART,
            alt: "",
            draggable: "false",
        }
        div { class: "night-shade" }
        div { class: "sky",
            svg { class: "sky-lines", "aria-hidden": "true",
                for (index, pair) in STARS.windows(2).enumerate() {
                    g { key: "{index}",
                        line {
                            class: "sky-line",
                            x1: "{pair[0].x}%",
                            y1: "{pair[0].y}%",
                            x2: "{pair[1].x}%",
                            y2: "{pair[1].y}%",
                        }
                        line {
                            class: "sky-trace",
                            style: "--i: {index};",
                            "pathLength": "1",
                            x1: "{pair[0].x}%",
                            y1: "{pair[0].y}%",
                            x2: "{pair[1].x}%",
                            y2: "{pair[1].y}%",
                        }
                    }
                }
            }
            ol { class: "sky-stars", aria_label: "Шість етапів створення гри",
                for (index, star) in STARS.iter().enumerate() {
                    li {
                        key: "{star.code}",
                        class: star_class(star, index == last),
                        style: "--x: {star.x}%; --y: {star.y}%; --i: {index};",
                        span { class: "star-core" }
                        div { class: "star-label",
                            span { class: "star-code", "{star.code}" }
                            span { class: "star-title", "{star.title}" }
                            span { class: "star-note", "{star.note}" }
                        }
                    }
                }
            }
        }
    }
}

fn star_class(star: &Star, is_final: bool) -> String {
    let mut class = String::from("star");
    if star.label_above {
        class.push_str(" is-above");
    }
    if star.label_left {
        class.push_str(" is-left");
    }
    if is_final {
        class.push_str(" is-final");
    }
    class
}
