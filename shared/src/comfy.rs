use std::fmt;

use serde::{Deserialize, Serialize};

pub const COMFY_CHANGED_EVENT: &str = "comfy-changed";
pub const COMFY_API_PORT: u16 = 47822;
pub const MIN_ASSET_BRIEF_CHARS: usize = 3;
pub const MAX_ASSET_BRIEF_CHARS: usize = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Medium {
    Image,
    Mesh,
    Audio,
}

impl Medium {
    pub const ALL: [Self; 3] = [Self::Image, Self::Mesh, Self::Audio];

    pub fn label(self) -> &'static str {
        match self {
            Self::Image => "Зображення",
            Self::Mesh => "3D",
            Self::Audio => "Звук і музика",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Concept,
    Sprite,
    Texture,
    Effect,
    Interface,
    Model,
    Music,
    Sound,
}

impl AssetKind {
    pub const ALL: [Self; 8] = [
        Self::Concept,
        Self::Sprite,
        Self::Texture,
        Self::Effect,
        Self::Interface,
        Self::Model,
        Self::Music,
        Self::Sound,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Concept => "concept",
            Self::Sprite => "sprite",
            Self::Texture => "texture",
            Self::Effect => "effect",
            Self::Interface => "interface",
            Self::Model => "model",
            Self::Music => "music",
            Self::Sound => "sound",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let code = code.trim().to_ascii_lowercase();
        let alias = match code.as_str() {
            "vfx" | "fx" => Some(Self::Effect),
            "ui" | "icon" => Some(Self::Interface),
            "3d" | "mesh" => Some(Self::Model),
            "sfx" => Some(Self::Sound),
            "art" | "image" => Some(Self::Concept),
            _ => None,
        };
        alias.or_else(|| Self::ALL.into_iter().find(|kind| kind.code() == code))
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Concept => "Концепт-арт",
            Self::Sprite => "Спрайт",
            Self::Texture => "Текстура",
            Self::Effect => "VFX-ефект",
            Self::Interface => "UI-елемент",
            Self::Model => "3D-модель",
            Self::Music => "Музика",
            Self::Sound => "Звук",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Concept => "Персонажі, локації, предмети",
            Self::Sprite => "Прозоре тло — одразу в гру",
            Self::Texture => "Безшовна, тайлиться",
            Self::Effect => "На чорному тлі для additive",
            Self::Interface => "Кнопки, рамки, іконки",
            Self::Model => "GLB з текстурою",
            Self::Music => "Тема, фон, луп",
            Self::Sound => "Короткий SFX",
        }
    }

    pub fn medium(self) -> Medium {
        match self {
            Self::Model => Medium::Mesh,
            Self::Music | Self::Sound => Medium::Audio,
            _ => Medium::Image,
        }
    }

    pub fn transparent(self) -> bool {
        matches!(self, Self::Sprite | Self::Interface)
    }

    pub fn tiles(self) -> bool {
        self == Self::Texture
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RosterSource {
    Server,
    Disk,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComfyModel {
    pub id: String,
    pub name: String,
    pub note: String,
    pub precision: Option<String>,
    pub medium: Medium,
    pub ready: bool,
    pub missing: Vec<String>,
    pub enabled: bool,
}

impl ComfyModel {
    pub fn usable(&self) -> bool {
        self.ready && self.enabled
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComfyStatus {
    pub online: bool,
    pub url: String,
    pub version: Option<String>,
    pub gpu: Option<String>,
    pub vram_mb: Option<u64>,
    pub source: RosterSource,
    pub launchable: bool,
    pub launching: bool,
    pub media_base: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Writing,
    Queued,
    Running,
    Done,
    Failed,
}

impl TaskStatus {
    pub fn code(self) -> &'static str {
        match self {
            Self::Writing => "writing",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        [
            Self::Writing,
            Self::Queued,
            Self::Running,
            Self::Done,
            Self::Failed,
        ]
        .into_iter()
        .find(|status| status.code() == code)
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Writing | Self::Queued | Self::Running)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Done,
    Failed,
    Skipped,
}

impl RunStatus {
    pub fn code(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        [
            Self::Queued,
            Self::Running,
            Self::Done,
            Self::Failed,
            Self::Skipped,
        ]
        .into_iter()
        .find(|status| status.code() == code)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOrigin {
    Studio,
    Api,
}

impl TaskOrigin {
    pub fn code(self) -> &'static str {
        match self {
            Self::Studio => "studio",
            Self::Api => "api",
        }
    }

    pub fn from_code(code: &str) -> Self {
        if code == Self::Api.code() {
            Self::Api
        } else {
            Self::Studio
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFile {
    pub name: String,
    pub medium: Medium,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRun {
    pub id: i64,
    pub model_id: String,
    pub model_name: String,
    pub reference: bool,
    pub status: RunStatus,
    pub files: Vec<RunFile>,
    pub error: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

impl ModelRun {
    pub fn seconds(&self) -> Option<i64> {
        Some(self.finished_at? - self.started_at?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetTask {
    pub id: i64,
    pub kind: AssetKind,
    pub brief: String,
    pub title: String,
    pub prompt: String,
    pub origin: TaskOrigin,
    pub project_name: Option<String>,
    pub status: TaskStatus,
    pub note: Option<String>,
    pub created_at: i64,
    pub runs: Vec<ModelRun>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComfyOverview {
    pub status: ComfyStatus,
    pub models: Vec<ComfyModel>,
    pub tasks: Vec<AssetTask>,
}

impl ComfyOverview {
    pub fn busy(&self) -> bool {
        self.tasks.iter().any(|task| task.status.is_active())
    }

    pub fn usable_count(&self, medium: Medium) -> usize {
        self.models
            .iter()
            .filter(|model| model.medium == medium && model.usable())
            .count()
    }

    pub fn runs_for(&self, kind: AssetKind) -> usize {
        let candidates = self.usable_count(kind.medium());
        if kind == AssetKind::Model && self.usable_count(Medium::Image) == 0 {
            0
        } else {
            candidates
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetDraft {
    pub brief: String,
    pub kinds: Vec<AssetKind>,
    pub project_id: Option<i64>,
}

impl AssetDraft {
    pub fn normalized(&self) -> Result<AssetDraft, ComfyError> {
        let brief = self.brief.split_whitespace().collect::<Vec<_>>().join(" ");
        match brief.chars().count() {
            count if count < MIN_ASSET_BRIEF_CHARS => return Err(ComfyError::BriefMissing),
            count if count > MAX_ASSET_BRIEF_CHARS => return Err(ComfyError::BriefTooLong),
            _ => {}
        }
        let kinds: Vec<AssetKind> = AssetKind::ALL
            .into_iter()
            .filter(|kind| self.kinds.contains(kind))
            .collect();
        if kinds.is_empty() {
            return Err(ComfyError::NoKinds);
        }
        Ok(AssetDraft {
            brief,
            kinds,
            project_id: self.project_id,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum ComfyError {
    SignedOut,
    BriefMissing,
    BriefTooLong,
    NoKinds,
    NoModels,
    NotFound,
    NotInstalled,
    Launch(String),
    Storage(String),
}

impl fmt::Display for ComfyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignedOut => f.write_str("no account is signed in"),
            Self::BriefMissing => {
                write!(f, "task brief is shorter than {MIN_ASSET_BRIEF_CHARS} characters")
            }
            Self::BriefTooLong => {
                write!(f, "task brief is longer than {MAX_ASSET_BRIEF_CHARS} characters")
            }
            Self::NoKinds => f.write_str("no asset kind was chosen"),
            Self::NoModels => f.write_str("no installed ComfyUI model can make these assets"),
            Self::NotFound => f.write_str("task or run does not exist"),
            Self::NotInstalled => f.write_str("no local ComfyUI install was found"),
            Self::Launch(detail) => write!(f, "ComfyUI did not start: {detail}"),
            Self::Storage(detail) => write!(f, "gallery storage failed: {detail}"),
        }
    }
}

impl std::error::Error for ComfyError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip_and_accept_aliases() {
        for kind in AssetKind::ALL {
            assert_eq!(AssetKind::from_code(kind.code()), Some(kind));
        }
        assert_eq!(AssetKind::from_code(" VFX "), Some(AssetKind::Effect));
        assert_eq!(AssetKind::from_code("3d"), Some(AssetKind::Model));
        assert_eq!(AssetKind::from_code("sfx"), Some(AssetKind::Sound));
        assert_eq!(AssetKind::from_code("video"), None);
    }

    #[test]
    fn drafts_are_trimmed_and_ordered() {
        let draft = AssetDraft {
            brief: "  вогняний   меч  ".into(),
            kinds: vec![AssetKind::Sound, AssetKind::Sprite, AssetKind::Sound],
            project_id: None,
        };
        let normalized = draft.normalized().unwrap();
        assert_eq!(normalized.brief, "вогняний меч");
        assert_eq!(normalized.kinds, vec![AssetKind::Sprite, AssetKind::Sound]);

        let empty = AssetDraft {
            brief: "меч".into(),
            kinds: vec![],
            project_id: None,
        };
        assert_eq!(empty.normalized(), Err(ComfyError::NoKinds));
        let short = AssetDraft {
            brief: " ж ".into(),
            kinds: vec![AssetKind::Concept],
            project_id: None,
        };
        assert_eq!(short.normalized(), Err(ComfyError::BriefMissing));
    }

    #[test]
    fn statuses_keep_their_codes() {
        assert_eq!(TaskStatus::from_code("running"), Some(TaskStatus::Running));
        assert_eq!(RunStatus::from_code("skipped"), Some(RunStatus::Skipped));
        assert_eq!(TaskOrigin::from_code("api"), TaskOrigin::Api);
        let json = serde_json::to_string(&ComfyError::NoModels).unwrap();
        assert_eq!(json, r#"{"kind":"no_models"}"#);
    }
}
