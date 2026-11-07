use std::fmt;

use serde::{Deserialize, Serialize};

use crate::GameEngine;

pub const MIXAMO_API_PORT: u16 = 47834;
pub const MIXAMO_CHANGED_EVENT: &str = "mixamo-changed";
pub const MAX_MOTION_NAME_CHARS: usize = 48;
pub const MAX_MOTION_QUERY_CHARS: usize = 120;
pub const MOTION_FPS_CHOICES: [u8; 3] = [24, 30, 60];
pub const DEFAULT_MOTION_FPS: u8 = 30;
pub const FULL_TRIM: u8 = 100;

const MAX_CLIP_ID_CHARS: usize = 64;

pub fn engine_takes_motions(engine: GameEngine) -> bool {
    !matches!(engine, GameEngine::Phaser | GameEngine::Defold)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MotionOptions {
    pub in_place: bool,
    pub mirror: bool,
    pub skin: bool,
    pub fps: u8,
    pub trim_start: u8,
    pub trim_end: u8,
}

impl Default for MotionOptions {
    fn default() -> Self {
        Self {
            in_place: false,
            mirror: false,
            skin: false,
            fps: DEFAULT_MOTION_FPS,
            trim_start: 0,
            trim_end: FULL_TRIM,
        }
    }
}

impl MotionOptions {
    pub fn checked(self) -> Self {
        let fps = MOTION_FPS_CHOICES
            .into_iter()
            .min_by_key(|choice| choice.abs_diff(self.fps))
            .unwrap_or(DEFAULT_MOTION_FPS);
        let trim_end = self.trim_end.min(FULL_TRIM);
        let trim_start = self.trim_start.min(trim_end.saturating_sub(1));
        let (trim_start, trim_end) = if trim_end == 0 { (0, FULL_TRIM) } else { (trim_start, trim_end) };
        Self {
            fps,
            trim_start,
            trim_end,
            ..self
        }
    }

    pub fn is_trimmed(&self) -> bool {
        self.trim_start > 0 || self.trim_end < FULL_TRIM
    }

    pub fn tags(&self) -> Vec<String> {
        let mut tags = Vec::new();
        if self.in_place {
            tags.push("на місці".to_owned());
        }
        if self.mirror {
            tags.push("дзеркально".to_owned());
        }
        if self.skin {
            tags.push("зі скіном".to_owned());
        }
        if self.is_trimmed() {
            tags.push(format!("{}–{}%", self.trim_start, self.trim_end));
        }
        tags.push(format!("{} fps", self.fps));
        tags
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MotionClip {
    pub id: String,
    pub name: String,
    pub description: String,
    pub thumbnail: String,
    pub animated: String,
    pub supports_in_place: bool,
}

impl MotionClip {
    pub fn preview(&self) -> &str {
        if self.animated.is_empty() { &self.thumbnail } else { &self.animated }
    }

    pub fn title(&self) -> &str {
        if self.description.is_empty() { &self.name } else { &self.description }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionStatus {
    Queued,
    Searching,
    Exporting,
    Downloading,
    Done,
    Failed,
}

impl MotionStatus {
    pub const ALL: [Self; 6] = [
        Self::Queued,
        Self::Searching,
        Self::Exporting,
        Self::Downloading,
        Self::Done,
        Self::Failed,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Searching => "searching",
            Self::Exporting => "exporting",
            Self::Downloading => "downloading",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.code() == code)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "У черзі",
            Self::Searching => "Шукає на Mixamo",
            Self::Exporting => "Mixamo запікає",
            Self::Downloading => "Завантажує FBX",
            Self::Done => "Готово",
            Self::Failed => "Помилка",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Searching | Self::Exporting | Self::Downloading)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MotionOrder {
    pub id: i64,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
    pub name: String,
    pub query: String,
    pub status: MotionStatus,
    pub clip: Option<MotionClip>,
    pub options: MotionOptions,
    pub file: Option<String>,
    pub size_bytes: Option<u64>,
    pub error: Option<String>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MotionDraft {
    pub name: String,
    pub query: String,
    pub clip_id: Option<String>,
    pub project_id: Option<i64>,
    pub options: MotionOptions,
}

impl MotionDraft {
    pub fn normalized(&self) -> Result<Self, MixamoError> {
        let query = collapse(&self.query);
        let clip_id = self
            .clip_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_owned);
        if let Some(id) = &clip_id {
            let valid = id.len() <= MAX_CLIP_ID_CHARS
                && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
            if !valid {
                return Err(MixamoError::NotFound);
            }
        }
        if query.is_empty() && clip_id.is_none() {
            return Err(MixamoError::QueryMissing);
        }
        if query.chars().count() > MAX_MOTION_QUERY_CHARS {
            return Err(MixamoError::QueryTooLong);
        }
        let name = collapse(&self.name);
        let name = if name.is_empty() { query.clone() } else { name };
        if name.is_empty() {
            return Err(MixamoError::NameMissing);
        }
        if name.chars().count() > MAX_MOTION_NAME_CHARS {
            return Err(MixamoError::NameTooLong);
        }
        Ok(Self {
            name,
            query,
            clip_id,
            project_id: self.project_id,
            options: self.options.checked(),
        })
    }
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MixamoStatus {
    pub signed_in: Option<bool>,
    pub character: Option<String>,
    pub working: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MixamoOverview {
    pub status: MixamoStatus,
    pub api: String,
    pub orders: Vec<MotionOrder>,
}

impl MixamoOverview {
    pub fn for_project(&self, project_id: i64) -> Vec<&MotionOrder> {
        self.orders
            .iter()
            .filter(|order| order.project_id == Some(project_id))
            .collect()
    }

    pub fn busy(&self) -> bool {
        self.orders
            .iter()
            .any(|order| order.status.is_active() || order.status == MotionStatus::Queued)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum MixamoError {
    SignedOut,
    NotFound,
    NameMissing,
    NameTooLong,
    QueryMissing,
    QueryTooLong,
    NoMatch,
    Busy,
    Network(String),
    Browser(String),
    Storage(String),
}

impl fmt::Display for MixamoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignedOut => f.write_str("Mixamo session is missing or expired"),
            Self::NotFound => f.write_str("animation or order does not exist"),
            Self::NameMissing => f.write_str("animation name is empty"),
            Self::NameTooLong => write!(f, "animation name is longer than {MAX_MOTION_NAME_CHARS} characters"),
            Self::QueryMissing => f.write_str("search query is empty"),
            Self::QueryTooLong => write!(f, "search query is longer than {MAX_MOTION_QUERY_CHARS} characters"),
            Self::NoMatch => f.write_str("Mixamo has no animation for this query"),
            Self::Busy => f.write_str("the order is still being processed"),
            Self::Network(detail) => write!(f, "Mixamo request failed: {detail}"),
            Self::Browser(detail) => write!(f, "Mixamo window failed: {detail}"),
            Self::Storage(detail) => write!(f, "animation storage failed: {detail}"),
        }
    }
}

impl std::error::Error for MixamoError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_options_inside_mixamo_limits() {
        let wild = MotionOptions {
            fps: 50,
            trim_start: 120,
            trim_end: 140,
            ..MotionOptions::default()
        }
        .checked();
        assert_eq!(wild.fps, 60);
        assert_eq!((wild.trim_start, wild.trim_end), (99, 100));
        let empty = MotionOptions {
            trim_end: 0,
            ..MotionOptions::default()
        }
        .checked();
        assert_eq!((empty.trim_start, empty.trim_end), (0, FULL_TRIM));
        assert!(!MotionOptions::default().is_trimmed());
        assert_eq!(MotionOptions::default().tags(), ["30 fps"]);
    }

    #[test]
    fn normalizes_orders() {
        let draft = MotionDraft {
            name: "  ".into(),
            query: "  sword   slash ".into(),
            ..MotionDraft::default()
        };
        let normal = draft.normalized().unwrap();
        assert_eq!(normal.query, "sword slash");
        assert_eq!(normal.name, "sword slash");
        assert_eq!(MotionDraft::default().normalized(), Err(MixamoError::QueryMissing));
        let picked = MotionDraft {
            name: "attack".into(),
            clip_id: Some("a1b2-c3".into()),
            ..MotionDraft::default()
        };
        assert!(picked.normalized().is_ok());
        let hostile = MotionDraft {
            name: "x".into(),
            clip_id: Some("../../etc".into()),
            ..MotionDraft::default()
        };
        assert_eq!(hostile.normalized(), Err(MixamoError::NotFound));
        let long = MotionDraft {
            query: "x".repeat(MAX_MOTION_QUERY_CHARS + 1),
            ..MotionDraft::default()
        };
        assert_eq!(long.normalized(), Err(MixamoError::QueryTooLong));
    }

    #[test]
    fn statuses_round_trip() {
        for status in MotionStatus::ALL {
            assert_eq!(MotionStatus::from_code(status.code()), Some(status));
        }
        assert!(MotionStatus::Exporting.is_active());
        assert!(!MotionStatus::Done.is_active());
        assert!(engine_takes_motions(GameEngine::Godot));
        assert!(!engine_takes_motions(GameEngine::Phaser));
    }

    #[test]
    fn errors_keep_their_wire_format() {
        let json = serde_json::to_string(&MixamoError::SignedOut).unwrap();
        assert_eq!(json, r#"{"kind":"signed_out"}"#);
    }
}
