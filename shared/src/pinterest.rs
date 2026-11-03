use std::fmt;

use serde::{Deserialize, Serialize};

pub const PINS_PER_REFERENCE: usize = 8;
pub const PINTEREST_API_PORT: u16 = 47821;
pub const MAX_PIN_TITLE_CHARS: usize = 140;
pub const GRID_PIN_SIZE: &str = "474x";
pub const DETAIL_PIN_SIZE: &str = "736x";

const MAX_PIN_ID_CHARS: usize = 24;
const MAX_PIN_IMAGE_CHARS: usize = 400;
const PIN_IMAGE_HOST: &str = "https://i.pinimg.com/";
const PIN_PAGE: &str = "https://www.pinterest.com/pin/";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub id: String,
    pub image: String,
    pub title: String,
}

impl Pin {
    pub fn checked(id: &str, image: &str, title: &str) -> Option<Self> {
        let id = id.trim();
        let image = image.trim();
        if !(id.is_empty() || is_pin_id(id)) || !is_pin_image(image) {
            return None;
        }
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        Some(Self {
            id: id.to_owned(),
            image: resized(image, GRID_PIN_SIZE),
            title: title.chars().take(MAX_PIN_TITLE_CHARS).collect(),
        })
    }

    pub fn sized(&self, size: &str) -> String {
        resized(&self.image, size)
    }

    pub fn is_linked(&self) -> bool {
        is_pin_id(&self.id)
    }

    pub fn same_as(&self, other: &Pin) -> bool {
        self.image == other.image || (self.is_linked() && self.id == other.id)
    }
}

pub fn is_pin_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_PIN_ID_CHARS && id.bytes().all(|byte| byte.is_ascii_digit())
}

pub fn pin_page(id: &str) -> Option<String> {
    is_pin_id(id).then(|| format!("{PIN_PAGE}{id}/"))
}

fn is_pin_image(url: &str) -> bool {
    let Some((size, rest)) = url
        .strip_prefix(PIN_IMAGE_HOST)
        .and_then(|path| path.split_once('/'))
    else {
        return false;
    };
    (size == "originals" || is_width_segment(size))
        && !rest.is_empty()
        && url.len() <= MAX_PIN_IMAGE_CHARS
        && url
            .chars()
            .all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\'' | '\\' | '(' | ')' | '<' | '>'))
}

fn resized(url: &str, size: &str) -> String {
    let Some(path) = url.strip_prefix(PIN_IMAGE_HOST) else {
        return url.to_owned();
    };
    match path.split_once('/') {
        Some((current, rest)) if is_width_segment(current) => {
            format!("{PIN_IMAGE_HOST}{size}/{rest}")
        }
        _ => url.to_owned(),
    }
}

fn is_width_segment(segment: &str) -> bool {
    segment
        .strip_suffix('x')
        .is_some_and(|width| !width.is_empty() && width.bytes().all(|byte| byte.is_ascii_digit()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PinterestStatus {
    pub signed_in: Option<bool>,
    pub loading: Vec<i64>,
    pub empty: Vec<i64>,
}

impl PinterestStatus {
    pub fn is_loading(&self, project_id: i64) -> bool {
        self.loading.contains(&project_id)
    }

    pub fn came_back_empty(&self, project_id: i64) -> bool {
        self.empty.contains(&project_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum PinterestError {
    SignedOut,
    NotFound,
    NoReferences,
    Browser(String),
    Storage(String),
}

impl fmt::Display for PinterestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignedOut => f.write_str("no account is signed in"),
            Self::NotFound => f.write_str("project or pin does not exist"),
            Self::NoReferences => f.write_str("the project has no reference board yet"),
            Self::Browser(detail) => write!(f, "Pinterest browser failed: {detail}"),
            Self::Storage(detail) => write!(f, "reference storage failed: {detail}"),
        }
    }
}

impl std::error::Error for PinterestError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_real_pins_in_grid_size() {
        let pin = Pin::checked(
            " 123456789 ",
            "https://i.pinimg.com/236x/ab/cd/ef/abcdef.jpg",
            "  Rainy \n rooftops ",
        )
        .unwrap();
        assert_eq!(pin.id, "123456789");
        assert_eq!(pin.image, "https://i.pinimg.com/474x/ab/cd/ef/abcdef.jpg");
        assert_eq!(pin.title, "Rainy rooftops");
        assert_eq!(
            pin.sized(DETAIL_PIN_SIZE),
            "https://i.pinimg.com/736x/ab/cd/ef/abcdef.jpg"
        );
        let original = Pin::checked("1", "https://i.pinimg.com/originals/ab/cd.png", "").unwrap();
        assert_eq!(original.sized(DETAIL_PIN_SIZE), "https://i.pinimg.com/originals/ab/cd.png");
    }

    #[test]
    fn keeps_gated_pins_without_a_link() {
        let gated = Pin::checked("", "https://i.pinimg.com/236x/db/72/c5/db72.jpg", "").unwrap();
        assert!(!gated.is_linked());
        assert_eq!(gated.image, "https://i.pinimg.com/474x/db/72/c5/db72.jpg");
        let linked = Pin::checked("7", "https://i.pinimg.com/736x/db/72/c5/db72.jpg", "").unwrap();
        assert!(linked.is_linked() && linked.same_as(&gated));
        let other = Pin::checked("7", "https://i.pinimg.com/236x/aa/bb.jpg", "").unwrap();
        assert!(other.same_as(&linked) && !other.same_as(&gated));
    }

    #[test]
    fn rejects_foreign_or_broken_pins() {
        let image = "https://i.pinimg.com/236x/ab/cd.jpg";
        assert!(Pin::checked("12a", image, "").is_none());
        assert!(Pin::checked("1", "https://evil.example/236x/a.jpg", "").is_none());
        assert!(Pin::checked("1", "https://i.pinimg.com/a.jpg\" onerror=\"x", "").is_none());
        assert!(Pin::checked("1", "https://i.pinimg.com/", "").is_none());
        assert!(Pin::checked("1", "https://i.pinimg.com/75x75_RS/ab/cd.jpg", "").is_none());
        assert!(Pin::checked("1", "https://i.pinimg.com/236x/", "").is_none());
        assert_eq!(pin_page("42").as_deref(), Some("https://www.pinterest.com/pin/42/"));
        assert_eq!(pin_page("42/../x"), None);
        assert_eq!(pin_page(""), None);
    }

    #[test]
    fn errors_keep_their_wire_format() {
        let json = serde_json::to_string(&PinterestError::NoReferences).unwrap();
        assert_eq!(json, r#"{"kind":"no_references"}"#);
    }
}
