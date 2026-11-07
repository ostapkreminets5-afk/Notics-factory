use serde::{Deserialize, Serialize};

use crate::StudioError;

pub const MAX_HUNT_REQUEST_CHARS: usize = 300;
pub const MAX_ASSET_FINDS: usize = 24;
pub const MAX_KEPT_FINDS: usize = 48;
pub const MAX_FIND_TEXT_CHARS: usize = 220;
pub const MAX_FIND_URL_CHARS: usize = 500;

const SECURE_SCHEME: &str = "https://";
const PLAIN_SCHEME: &str = "http://";
const WWW_PREFIX: &str = "www.";
const UNSAFE_URL_CHARS: [char; 7] = ['"', '\'', '\\', '<', '>', '`', ' '];
const PRIVATE_SUFFIXES: [&str; 4] = [".localhost", ".local", ".internal", ".lan"];
const KNOWN_SITES: [(&str, &str); 22] = [
    ("itch.io", "itch.io"),
    ("opengameart.org", "OpenGameArt"),
    ("kenney.nl", "Kenney"),
    ("polyhaven.com", "Poly Haven"),
    ("ambientcg.com", "ambientCG"),
    ("freesound.org", "Freesound"),
    ("sketchfab.com", "Sketchfab"),
    ("quaternius.com", "Quaternius"),
    ("craftpix.net", "CraftPix"),
    ("gamedevmarket.net", "GameDev Market"),
    ("assetstore.unity.com", "Unity Asset Store"),
    ("fab.com", "Fab"),
    ("pixabay.com", "Pixabay"),
    ("fonts.google.com", "Google Fonts"),
    ("dafont.com", "DaFont"),
    ("fontlibrary.org", "Font Library"),
    ("game-icons.net", "Game-icons"),
    ("lospec.com", "Lospec"),
    ("mixkit.co", "Mixkit"),
    ("incompetech.com", "Incompetech"),
    ("poly.pizza", "Poly Pizza"),
    ("textures.com", "Textures.com"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindKind {
    Characters,
    Environment,
    Tiles,
    Props,
    Interface,
    Icons,
    Effects,
    Models,
    Textures,
    Sounds,
    Music,
    Fonts,
}

impl FindKind {
    pub const ALL: [Self; 12] = [
        Self::Characters,
        Self::Environment,
        Self::Tiles,
        Self::Props,
        Self::Interface,
        Self::Icons,
        Self::Effects,
        Self::Models,
        Self::Textures,
        Self::Sounds,
        Self::Music,
        Self::Fonts,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Characters => "characters",
            Self::Environment => "environment",
            Self::Tiles => "tiles",
            Self::Props => "props",
            Self::Interface => "interface",
            Self::Icons => "icons",
            Self::Effects => "effects",
            Self::Models => "models",
            Self::Textures => "textures",
            Self::Sounds => "sounds",
            Self::Music => "music",
            Self::Fonts => "fonts",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code.trim())
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Characters => "Персонажі",
            Self::Environment => "Оточення",
            Self::Tiles => "Тайли",
            Self::Props => "Предмети",
            Self::Interface => "Інтерфейс",
            Self::Icons => "Іконки",
            Self::Effects => "VFX",
            Self::Models => "3D-моделі",
            Self::Textures => "Текстури",
            Self::Sounds => "Звуки",
            Self::Music => "Музика",
            Self::Fonts => "Шрифти",
        }
    }

    pub fn is_audio(self) -> bool {
        matches!(self, Self::Sounds | Self::Music)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetCost {
    Free,
    PayWhatYouWant,
    Paid,
}

impl AssetCost {
    pub const ALL: [Self; 3] = [Self::Free, Self::PayWhatYouWant, Self::Paid];

    pub fn code(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::PayWhatYouWant => "pay_what_you_want",
            Self::Paid => "paid",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|cost| cost.code() == code.trim())
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Free => "Безкоштовно",
            Self::PayWhatYouWant => "Скільки не шкода",
            Self::Paid => "Платно",
        }
    }

    pub fn downloadable(self) -> bool {
        !matches!(self, Self::Paid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetFind {
    pub title: String,
    pub url: String,
    pub site: String,
    pub kind: FindKind,
    pub author: String,
    pub license: String,
    pub cost: AssetCost,
    pub price: String,
    pub fit: String,
    #[serde(default)]
    pub image: Option<String>,
}

pub struct RawFind<'a> {
    pub title: &'a str,
    pub url: &'a str,
    pub kind: &'a str,
    pub author: &'a str,
    pub license: &'a str,
    pub cost: &'a str,
    pub price: &'a str,
    pub fit: &'a str,
}

impl AssetFind {
    pub fn checked(raw: &RawFind<'_>) -> Option<Self> {
        let url = raw.url.trim();
        let title = tidy(raw.title);
        if title.is_empty() || !is_web_link(url) {
            return None;
        }
        let cost = AssetCost::from_code(raw.cost).unwrap_or(AssetCost::Paid);
        Some(Self {
            title,
            url: url.to_owned(),
            site: site_label(url)?,
            kind: FindKind::from_code(raw.kind)?,
            author: tidy(raw.author),
            license: tidy(raw.license),
            cost,
            price: if cost == AssetCost::Paid { tidy(raw.price) } else { String::new() },
            fit: tidy(raw.fit),
            image: None,
        })
    }

    pub fn cost_label(&self) -> String {
        if self.cost == AssetCost::Paid && !self.price.is_empty() {
            format!("{} · {}", self.cost.label(), self.price)
        } else {
            self.cost.label().to_owned()
        }
    }
}

pub fn is_web_link(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix(SECURE_SCHEME)
        .or_else(|| url.strip_prefix(PLAIN_SCHEME))
    else {
        return false;
    };
    let host = host_of(rest).to_ascii_lowercase();
    url.len() <= MAX_FIND_URL_CHARS
        && host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        && is_public_host(&host)
        && url
            .chars()
            .all(|c| c.is_ascii_graphic() && !UNSAFE_URL_CHARS.contains(&c))
}

fn is_public_host(host: &str) -> bool {
    let numeric = host
        .split('.')
        .all(|label| !label.is_empty() && label.bytes().all(|byte| byte.is_ascii_digit()));
    !numeric && !PRIVATE_SUFFIXES.iter().any(|suffix| host.ends_with(suffix))
}

pub fn is_image_link(url: &str) -> bool {
    url.starts_with(SECURE_SCHEME) && is_web_link(url)
}

pub fn site_label(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix(SECURE_SCHEME)
        .or_else(|| url.strip_prefix(PLAIN_SCHEME))?;
    let host = host_of(rest).to_ascii_lowercase();
    let host = host.strip_prefix(WWW_PREFIX).unwrap_or(&host).to_owned();
    if host.is_empty() {
        return None;
    }
    let known = KNOWN_SITES.iter().find(|(domain, _)| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    });
    Some(known.map_or(host.clone(), |(_, label)| (*label).to_owned()))
}

fn host_of(rest: &str) -> &str {
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let without_user = authority.rsplit('@').next().unwrap_or(authority);
    without_user.split(':').next().unwrap_or(without_user)
}

fn tidy(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_FIND_TEXT_CHARS)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HuntStatus {
    Queued,
    Searching,
    Done,
    Failed,
}

impl HuntStatus {
    pub const ALL: [Self; 4] = [Self::Queued, Self::Searching, Self::Done, Self::Failed];

    pub fn code(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Searching => "searching",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.code() == code)
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Searching)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "У черзі",
            Self::Searching => "Шукає",
            Self::Done => "Готово",
            Self::Failed => "Помилка",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetHunt {
    pub project_id: i64,
    pub status: HuntStatus,
    pub request: String,
    pub summary: String,
    pub finds: Vec<AssetFind>,
    pub error: Option<StudioError>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

impl AssetHunt {
    pub fn kinds(&self) -> Vec<FindKind> {
        FindKind::ALL
            .into_iter()
            .filter(|kind| self.finds.iter().any(|find| find.kind == *kind))
            .collect()
    }

    pub fn free_count(&self) -> usize {
        self.finds.iter().filter(|find| find.cost.downloadable()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw<'a>(url: &'a str, kind: &'a str, cost: &'a str) -> RawFind<'a> {
        RawFind {
            title: "  Ninja \n Adventure  ",
            url,
            kind,
            author: " Pixel  Frog ",
            license: "CC0",
            cost,
            price: "$4.99",
            fit: "Той самий піксельний стиль",
        }
    }

    #[test]
    fn keeps_real_finds_and_names_their_sites() {
        let find = AssetFind::checked(&raw("https://pixel-boy.itch.io/ninja-adventure-asset-pack", "characters", "free"))
            .unwrap();
        assert_eq!(find.title, "Ninja Adventure");
        assert_eq!(find.site, "itch.io");
        assert_eq!(find.author, "Pixel Frog");
        assert_eq!(find.kind, FindKind::Characters);
        assert_eq!(find.price, "");
        assert_eq!(find.cost_label(), "Безкоштовно");

        let paid = AssetFind::checked(&raw("https://www.gamedevmarket.net/asset/forest", "tiles", "paid")).unwrap();
        assert_eq!(paid.site, "GameDev Market");
        assert_eq!(paid.cost_label(), "Платно · $4.99");

        let other = AssetFind::checked(&raw("https://assets.example.org/pack", "music", "pay_what_you_want")).unwrap();
        assert_eq!(other.site, "assets.example.org");
        assert!(other.cost.downloadable());
    }

    #[test]
    fn rejects_broken_or_unsafe_finds() {
        assert!(AssetFind::checked(&raw("javascript:alert(1)", "characters", "free")).is_none());
        assert!(AssetFind::checked(&raw("https://itch.io/a\" onerror=\"x", "characters", "free")).is_none());
        assert!(AssetFind::checked(&raw("https://localhost/pack", "characters", "free")).is_none());
        assert!(!is_web_link("http://127.0.0.1:47822/tasks"));
        assert!(!is_web_link("https://192.168.1.10/pack"));
        assert!(!is_web_link("https://nas.local/pack"));
        assert!(!is_web_link("https://comfy.LOCALHOST/view"));
        assert!(is_web_link("https://2dgamesart.itch.io/pack-3"));
        assert!(AssetFind::checked(&raw("https://itch.io/pack", "spaceships", "free")).is_none());
        assert_eq!(site_label("https://notitch.io/pack").as_deref(), Some("notitch.io"));
        assert_eq!(site_label("https://user@kenney.nl:443/assets").as_deref(), Some("Kenney"));
        assert!(is_image_link("https://img.itch.zone/aW1n/315x250%23c/cover.png"));
        assert!(!is_image_link("http://img.itch.zone/cover.png"));
    }

    #[test]
    fn hunts_keep_their_wire_format() {
        let json = serde_json::to_string(&HuntStatus::Searching).unwrap();
        assert_eq!(json, r#""searching""#);
        assert_eq!(serde_json::to_string(&AssetCost::PayWhatYouWant).unwrap(), r#""pay_what_you_want""#);
        assert_eq!(HuntStatus::from_code("done"), Some(HuntStatus::Done));
    }
}
