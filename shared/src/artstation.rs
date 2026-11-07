use std::fmt;

use serde::{Deserialize, Serialize};

pub const ARTWORKS_PER_REFERENCE: usize = 8;
pub const ARTSTATION_API_PORT: u16 = 47833;
pub const MAX_ARTWORK_TEXT_CHARS: usize = 140;
pub const GRID_ARTWORK_SIZE: &str = "smaller_square";
pub const DETAIL_ARTWORK_SIZE: &str = "medium";

const MAX_ARTWORK_ID_CHARS: usize = 16;
const MAX_ARTWORK_IMAGE_CHARS: usize = 400;
const IMAGE_SCHEME: &str = "https://";
const IMAGE_HOST_PREFIX: &str = "cdn";
const IMAGE_HOST_SUFFIX: &str = ".artstation.com";
const IMAGE_PATH_PREFIX: &str = "p/";
const ARTWORK_PAGE: &str = "https://www.artstation.com/artwork/";
const IMAGE_SIZES: [&str; 7] = [
    "micro_square",
    "smaller_square",
    "small_square",
    "small",
    "medium",
    "large",
    "4k",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artwork {
    pub id: String,
    pub image: String,
    pub title: String,
    #[serde(default)]
    pub author: String,
}

impl Artwork {
    pub fn checked(id: &str, image: &str, title: &str, author: &str) -> Option<Self> {
        let id = id.trim();
        let image = image.trim();
        if !is_artwork_id(id) || !is_artwork_image(image) {
            return None;
        }
        Some(Self {
            id: id.to_owned(),
            image: resized(image, GRID_ARTWORK_SIZE),
            title: tidy(title),
            author: tidy(author),
        })
    }

    pub fn sized(&self, size: &str) -> String {
        resized(&self.image, size)
    }

    pub fn page(&self) -> String {
        format!("{ARTWORK_PAGE}{}", self.id)
    }
}

pub fn is_artwork_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ARTWORK_ID_CHARS && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

pub fn artwork_page(id: &str) -> Option<String> {
    is_artwork_id(id).then(|| format!("{ARTWORK_PAGE}{id}"))
}

fn tidy(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_ARTWORK_TEXT_CHARS)
        .collect()
}

fn is_artwork_image(url: &str) -> bool {
    let Some((host, path)) = url
        .strip_prefix(IMAGE_SCHEME)
        .and_then(|rest| rest.split_once('/'))
    else {
        return false;
    };
    let known_host = host
        .strip_prefix(IMAGE_HOST_PREFIX)
        .and_then(|rest| rest.strip_suffix(IMAGE_HOST_SUFFIX))
        .is_some_and(|shard| shard.len() == 1 && shard.bytes().all(|byte| byte.is_ascii_lowercase()));
    let file = path.split('?').next().unwrap_or_default();
    known_host
        && path.starts_with(IMAGE_PATH_PREFIX)
        && file.split('/').any(|segment| IMAGE_SIZES.contains(&segment))
        && !file.ends_with('/')
        && url.len() <= MAX_ARTWORK_IMAGE_CHARS
        && url
            .chars()
            .all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\'' | '\\' | '(' | ')' | '<' | '>'))
}

fn resized(url: &str, size: &str) -> String {
    let (address, stamp) = url.split_once('?').map_or((url, None), |(address, stamp)| (address, Some(stamp)));
    let segments: Vec<&str> = address
        .split('/')
        .map(|segment| if IMAGE_SIZES.contains(&segment) { size } else { segment })
        .collect();
    let address = segments.join("/");
    match stamp {
        Some(stamp) => format!("{address}?{stamp}"),
        None => address,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ArtstationStatus {
    pub loading: Vec<i64>,
    pub empty: Vec<i64>,
}

impl ArtstationStatus {
    pub fn is_loading(&self, project_id: i64) -> bool {
        self.loading.contains(&project_id)
    }

    pub fn came_back_empty(&self, project_id: i64) -> bool {
        self.empty.contains(&project_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum ArtstationError {
    SignedOut,
    NotFound,
    NoReferences,
    Network(String),
    Storage(String),
}

impl fmt::Display for ArtstationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignedOut => f.write_str("no account is signed in"),
            Self::NotFound => f.write_str("project or artwork does not exist"),
            Self::NoReferences => f.write_str("the project has no reference board yet"),
            Self::Network(detail) => write!(f, "ArtStation request failed: {detail}"),
            Self::Storage(detail) => write!(f, "reference storage failed: {detail}"),
        }
    }
}

impl std::error::Error for ArtstationError {}

#[cfg(test)]
mod tests {
    use super::*;

    const COVER: &str =
        "https://cdnb.artstation.com/p/assets/covers/images/075/971/033/smaller_square/leartes-thumb.jpg?1715853091";

    #[test]
    fn keeps_real_artworks_in_grid_size() {
        let artwork = Artwork::checked(
            " lD0q0k ",
            "https://cdna.artstation.com/p/assets/images/images/085/683/878/large/sketch.jpg?1741371096",
            "  Dark \n castle ",
            " Martin  Deschambault ",
        )
        .unwrap();
        assert_eq!(artwork.id, "lD0q0k");
        assert_eq!(
            artwork.image,
            "https://cdna.artstation.com/p/assets/images/images/085/683/878/smaller_square/sketch.jpg?1741371096"
        );
        assert_eq!(artwork.title, "Dark castle");
        assert_eq!(artwork.author, "Martin Deschambault");
        assert_eq!(
            artwork.sized(DETAIL_ARTWORK_SIZE),
            "https://cdna.artstation.com/p/assets/images/images/085/683/878/medium/sketch.jpg?1741371096"
        );
        assert_eq!(artwork.page(), "https://www.artstation.com/artwork/lD0q0k");
        assert!(Artwork::checked("2B1Xae", COVER, "", "").is_some());
    }

    #[test]
    fn rejects_foreign_or_broken_artworks() {
        assert!(Artwork::checked("", COVER, "", "").is_none());
        assert!(Artwork::checked("ab/../x", COVER, "", "").is_none());
        assert!(Artwork::checked("ab", "https://evil.example/p/assets/smaller_square/a.jpg", "", "").is_none());
        assert!(Artwork::checked("ab", "https://cdnab.artstation.com/p/assets/smaller_square/a.jpg", "", "").is_none());
        assert!(Artwork::checked("ab", "https://cdna.artstation.com.evil/p/smaller_square/a.jpg", "", "").is_none());
        assert!(Artwork::checked("ab", "https://cdna.artstation.com/p/assets/a.jpg", "", "").is_none());
        assert!(Artwork::checked("ab", "https://cdna.artstation.com/p/assets/smaller_square/", "", "").is_none());
        assert!(Artwork::checked("ab", "https://cdna.artstation.com/p/smaller_square/a.jpg\" onerror=\"x", "", "").is_none());
        assert_eq!(artwork_page("2B1Xae").as_deref(), Some("https://www.artstation.com/artwork/2B1Xae"));
        assert_eq!(artwork_page("2B1/../x"), None);
        assert_eq!(artwork_page(""), None);
    }

    #[test]
    fn errors_keep_their_wire_format() {
        let json = serde_json::to_string(&ArtstationError::Network("timeout".into())).unwrap();
        assert_eq!(json, r#"{"kind":"network","detail":"timeout"}"#);
    }
}
