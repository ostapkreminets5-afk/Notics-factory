use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Pin;

pub const CLAUDE_MODEL: &str = "claude-opus-5-5";
pub const MAX_PROMPT_CHARS: usize = 4000;
pub const MIN_PROMPT_CHARS: usize = 12;
pub const MAX_PROJECT_NAME_CHARS: usize = 48;
pub const MAX_SCREEN_TITLE_CHARS: usize = 48;
pub const MAX_SCREEN_SUMMARY_CHARS: usize = 240;
pub const MAX_FEEDBACK_CHARS: usize = 2000;
pub const PALETTE_SIZE: usize = 5;
pub const MIN_PLANNED_SCREENS: usize = 3;
pub const MAX_PLANNED_SCREENS: usize = 8;
pub const MAX_CORNER_RADIUS: u8 = 32;
pub const SCREEN_WIDTH: u32 = 1280;
pub const SCREEN_HEIGHT: u32 = 720;
pub const INTERFACE_SKILL: &str = "game-ui-screens";
pub const ART_SKILL: &str = "real-art-not-vector";
pub const SCREEN_SKILLS: [&str; 2] = [INTERFACE_SKILL, ART_SKILL];
pub const DESIGN_STAGE: u8 = 1;
pub const INTERFACE_STAGE: u8 = 2;
pub const MIN_REFERENCES: usize = 8;
pub const MAX_REFERENCES: usize = 18;
pub const REFERENCE_PALETTE_SIZE: usize = 3;
pub const MAX_ANALYSIS_KEYWORDS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Queued,
    Planning,
    Designing,
    Ready,
    Failed,
}

impl ProjectStatus {
    pub const ALL: [Self; 5] = [
        Self::Queued,
        Self::Planning,
        Self::Designing,
        Self::Ready,
        Self::Failed,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Planning => "planning",
            Self::Designing => "designing",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.code() == code)
    }

    pub fn is_planning(self) -> bool {
        matches!(self, Self::Queued | Self::Planning)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenStatus {
    Queued,
    Generating,
    Draft,
    Approved,
    Failed,
}

impl ScreenStatus {
    pub const ALL: [Self; 5] = [
        Self::Queued,
        Self::Generating,
        Self::Draft,
        Self::Approved,
        Self::Failed,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Generating => "generating",
            Self::Draft => "draft",
            Self::Approved => "approved",
            Self::Failed => "failed",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.code() == code)
    }

    pub fn is_busy(self) -> bool {
        matches!(self, Self::Queued | Self::Generating)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStatus {
    Queued,
    Generating,
    Done,
    Failed,
}

impl OutputStatus {
    pub const ALL: [Self; 4] = [Self::Queued, Self::Generating, Self::Done, Self::Failed];

    pub fn code(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Generating => "generating",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.code() == code)
    }

    pub fn is_busy(self) -> bool {
        matches!(self, Self::Queued | Self::Generating)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GameEngine {
    #[default]
    Godot,
    Unity,
    Unreal,
    ThreeJs,
    BabylonJs,
    Phaser,
    Bevy,
    ThreeD,
    Penumbra,
    Defold,
}

impl GameEngine {
    pub const ALL: [Self; 10] = [
        Self::Godot,
        Self::Unity,
        Self::Unreal,
        Self::ThreeJs,
        Self::BabylonJs,
        Self::Phaser,
        Self::Bevy,
        Self::ThreeD,
        Self::Penumbra,
        Self::Defold,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Godot => "godot",
            Self::Unity => "unity",
            Self::Unreal => "unreal",
            Self::ThreeJs => "three_js",
            Self::BabylonJs => "babylon_js",
            Self::Phaser => "phaser",
            Self::Bevy => "bevy",
            Self::ThreeD => "three_d",
            Self::Penumbra => "penumbra",
            Self::Defold => "defold",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|engine| engine.code() == code)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Godot => "Godot",
            Self::Unity => "Unity",
            Self::Unreal => "Unreal Engine",
            Self::ThreeJs => "Three.js",
            Self::BabylonJs => "Babylon.js",
            Self::Phaser => "Phaser",
            Self::Bevy => "Bevy",
            Self::ThreeD => "three-d",
            Self::Penumbra => "Penumbra",
            Self::Defold => "Defold",
        }
    }

    pub fn stack(self) -> &'static str {
        match self {
            Self::Godot => "GDScript / C#",
            Self::Unity => "C#",
            Self::Unreal => "C++ / Blueprints",
            Self::ThreeJs => "JavaScript · WebGL",
            Self::BabylonJs => "TypeScript · WebGL",
            Self::Phaser => "JavaScript · 2D",
            Self::Bevy => "Rust · ECS",
            Self::ThreeD => "Rust · OpenGL",
            Self::Penumbra => "Rust · wgpu",
            Self::Defold => "Lua",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StyleGuide {
    pub palette: Vec<String>,
    pub fonts: String,
    pub corner_radius: u8,
    pub mood: String,
}

impl StyleGuide {
    pub fn summary(&self) -> String {
        format!(
            "Шрифти: {} · Кути {} px · {}",
            self.fonts, self.corner_radius, self.mood
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceTopic {
    World,
    Characters,
    Light,
    Vfx,
    Interface,
    Panels,
    Buttons,
    Icons,
    Materials,
    Props,
    Animation,
    Sounds,
    Music,
}

impl ReferenceTopic {
    pub const ALL: [Self; 13] = [
        Self::World,
        Self::Characters,
        Self::Light,
        Self::Vfx,
        Self::Interface,
        Self::Panels,
        Self::Buttons,
        Self::Icons,
        Self::Materials,
        Self::Props,
        Self::Animation,
        Self::Sounds,
        Self::Music,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::World => "world",
            Self::Characters => "characters",
            Self::Light => "light",
            Self::Vfx => "vfx",
            Self::Interface => "interface",
            Self::Panels => "panels",
            Self::Buttons => "buttons",
            Self::Icons => "icons",
            Self::Materials => "materials",
            Self::Props => "props",
            Self::Animation => "animation",
            Self::Sounds => "sounds",
            Self::Music => "music",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|topic| topic.code() == code)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::World => "Світ",
            Self::Characters => "Персонажі",
            Self::Light => "Світло й настрій",
            Self::Vfx => "VFX і ефекти",
            Self::Interface => "Інтерфейс і HUD",
            Self::Panels => "Панелі й рамки",
            Self::Buttons => "Кнопки",
            Self::Icons => "Іконки",
            Self::Materials => "Матеріали",
            Self::Props => "Предмети",
            Self::Animation => "Анімація",
            Self::Sounds => "Звуки й SFX",
            Self::Music => "Музика",
        }
    }

    pub fn is_audio(self) -> bool {
        matches!(self, Self::Sounds | Self::Music)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub topic: ReferenceTopic,
    pub title: String,
    pub note: String,
    pub query: String,
    pub palette: Vec<String>,
    #[serde(default)]
    pub pins: Vec<Pin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchNeed {
    pub topic: ReferenceTopic,
    pub what: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PromptAnalysis {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub needs: Vec<SearchNeed>,
}

impl PromptAnalysis {
    pub fn is_empty(&self) -> bool {
        self.summary.is_empty() && self.keywords.is_empty() && self.needs.is_empty()
    }

    pub fn need_for(&self, topic: ReferenceTopic) -> Option<&SearchNeed> {
        self.needs.iter().find(|need| need.topic == topic)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub prompt: String,
    pub pitch: String,
    pub engine: GameEngine,
    pub status: ProjectStatus,
    pub style: StyleGuide,
    pub references: Vec<Reference>,
    #[serde(default)]
    pub analysis: PromptAnalysis,
    pub cover_screen_id: Option<i64>,
    pub cover_html: Option<String>,
    pub screen_count: u32,
    pub approved_count: u32,
    pub pending_count: u32,
    pub error: Option<StudioError>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Screen {
    pub id: i64,
    pub project_id: i64,
    pub position: u32,
    pub title: String,
    pub summary: String,
    pub status: ScreenStatus,
    pub html: Option<String>,
    pub revision: u32,
    pub error: Option<StudioError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillOutput {
    pub id: i64,
    pub project_id: i64,
    pub skill_name: String,
    pub title: String,
    pub stage: u8,
    pub status: OutputStatus,
    pub content: Option<String>,
    pub error: Option<StudioError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDetail {
    pub project: Project,
    pub design_document: String,
    pub screens: Vec<Screen>,
    pub outputs: Vec<SkillOutput>,
}

pub fn runs_as_deliverable(skill_name: &str) -> bool {
    !SCREEN_SKILLS.contains(&skill_name)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ClaudeAccount {
    pub installed: bool,
    pub version: Option<String>,
    pub signed_in: bool,
    pub email: Option<String>,
    pub plan: Option<String>,
}

impl ClaudeAccount {
    pub fn connected(&self) -> bool {
        self.installed && self.signed_in
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageWindow {
    pub percent: u32,
    pub resets: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ClaudeUsage {
    pub session: Option<UsageWindow>,
    pub weekly: Option<UsageWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudioSettings {
    pub account: ClaudeAccount,
    pub model: String,
    pub skills_folder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum StudioError {
    SignedOut,
    ClaudeCodeMissing,
    ClaudeSignedOut,
    UsageLimit,
    NotFound,
    PromptTooShort,
    PromptTooLong,
    NameMissing,
    NameTooLong,
    SummaryTooLong,
    FeedbackMissing,
    FeedbackTooLong,
    NotApproved,
    StillPlanning,
    ScreenBusy,
    SkillUnavailable,
    Refused,
    Truncated,
    RateLimited,
    Overloaded,
    Process(String),
    Claude(String),
    Storage(String),
}

impl fmt::Display for StudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignedOut => f.write_str("no account is signed in"),
            Self::ClaudeCodeMissing => f.write_str("Claude Code CLI is not installed"),
            Self::ClaudeSignedOut => f.write_str("Claude Code is not signed in to a Claude subscription"),
            Self::UsageLimit => f.write_str("the Claude subscription usage limit is reached"),
            Self::NotFound => f.write_str("project or screen does not exist"),
            Self::PromptTooShort => {
                write!(f, "game prompt is shorter than {MIN_PROMPT_CHARS} characters")
            }
            Self::PromptTooLong => {
                write!(f, "game prompt is longer than {MAX_PROMPT_CHARS} characters")
            }
            Self::NameMissing => f.write_str("name is empty"),
            Self::NameTooLong => f.write_str("name is too long"),
            Self::SummaryTooLong => f.write_str("screen description is too long"),
            Self::FeedbackMissing => f.write_str("feedback is empty"),
            Self::FeedbackTooLong => {
                write!(f, "feedback is longer than {MAX_FEEDBACK_CHARS} characters")
            }
            Self::NotApproved => f.write_str("the cover screen is not approved"),
            Self::StillPlanning => f.write_str("the project is still being planned"),
            Self::ScreenBusy => f.write_str("the screen is being generated"),
            Self::SkillUnavailable => f.write_str("the skill was disabled or deleted"),
            Self::Refused => f.write_str("Claude declined the request"),
            Self::Truncated => f.write_str("Claude's answer was cut off"),
            Self::RateLimited => f.write_str("Claude rate limit reached"),
            Self::Overloaded => f.write_str("Claude is overloaded"),
            Self::Process(detail) => write!(f, "Claude Code process failed: {detail}"),
            Self::Claude(detail) => write!(f, "Claude error: {detail}"),
            Self::Storage(detail) => write!(f, "studio storage failed: {detail}"),
        }
    }
}

impl std::error::Error for StudioError {}

pub fn validate_game_prompt(prompt: &str) -> Result<String, StudioError> {
    let prompt = prompt.trim();
    match prompt.chars().count() {
        count if count < MIN_PROMPT_CHARS => Err(StudioError::PromptTooShort),
        count if count > MAX_PROMPT_CHARS => Err(StudioError::PromptTooLong),
        _ => Ok(prompt.to_owned()),
    }
}

pub fn validate_label(text: &str, limit: usize) -> Result<String, StudioError> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match text.chars().count() {
        0 => Err(StudioError::NameMissing),
        count if count > limit => Err(StudioError::NameTooLong),
        _ => Ok(text),
    }
}

pub fn validate_screen_summary(summary: &str) -> Result<String, StudioError> {
    let summary = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    if summary.chars().count() > MAX_SCREEN_SUMMARY_CHARS {
        return Err(StudioError::SummaryTooLong);
    }
    Ok(summary)
}

pub fn validate_feedback(feedback: &str) -> Result<String, StudioError> {
    let feedback = feedback.trim();
    match feedback.chars().count() {
        0 => Err(StudioError::FeedbackMissing),
        count if count > MAX_FEEDBACK_CHARS => Err(StudioError::FeedbackTooLong),
        _ => Ok(feedback.to_owned()),
    }
}

pub fn is_hex_color(value: &str) -> bool {
    let digits = value.strip_prefix('#').unwrap_or_default();
    matches!(digits.len(), 3 | 6) && digits.chars().all(|c| c.is_ascii_hexdigit())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip_through_codes() {
        for status in ProjectStatus::ALL {
            assert_eq!(ProjectStatus::from_code(status.code()), Some(status));
        }
        for status in ScreenStatus::ALL {
            assert_eq!(ScreenStatus::from_code(status.code()), Some(status));
        }
        for status in OutputStatus::ALL {
            assert_eq!(OutputStatus::from_code(status.code()), Some(status));
        }
        assert!(!runs_as_deliverable(INTERFACE_SKILL));
        assert!(!runs_as_deliverable(ART_SKILL));
        assert!(runs_as_deliverable("game-scripts"));
        assert_eq!(ScreenStatus::from_code("unknown"), None);
        for topic in ReferenceTopic::ALL {
            assert_eq!(ReferenceTopic::from_code(topic.code()), Some(topic));
        }
        let audio: Vec<ReferenceTopic> = ReferenceTopic::ALL
            .into_iter()
            .filter(|topic| topic.is_audio())
            .collect();
        assert_eq!(audio, [ReferenceTopic::Sounds, ReferenceTopic::Music]);
        for engine in GameEngine::ALL {
            assert_eq!(GameEngine::from_code(engine.code()), Some(engine));
            let wire = serde_json::to_string(&engine).unwrap();
            assert_eq!(wire, format!("\"{}\"", engine.code()));
        }
        assert_eq!(GameEngine::from_code("cryengine"), None);
    }

    #[test]
    fn validates_prompts_and_labels() {
        assert_eq!(validate_game_prompt("коротко"), Err(StudioError::PromptTooShort));
        assert_eq!(
            validate_game_prompt("  Нічний злодій у неоновому місті  ").unwrap(),
            "Нічний злодій у неоновому місті"
        );
        assert_eq!(
            validate_label("  Головне \n меню ", MAX_SCREEN_TITLE_CHARS).unwrap(),
            "Головне меню"
        );
        assert_eq!(validate_label("   ", 10), Err(StudioError::NameMissing));
        assert_eq!(validate_label("довга назва", 4), Err(StudioError::NameTooLong));
        assert_eq!(validate_feedback(" \n"), Err(StudioError::FeedbackMissing));
    }

    #[test]
    fn recognizes_hex_colors() {
        assert!(is_hex_color("#3b7bff"));
        assert!(is_hex_color("#FFF"));
        assert!(!is_hex_color("3b7bff"));
        assert!(!is_hex_color("#3b7bfz"));
        assert!(!is_hex_color("#3b7b"));
    }

    #[test]
    fn connects_only_when_installed_and_signed_in() {
        let mut account = ClaudeAccount {
            installed: true,
            ..ClaudeAccount::default()
        };
        assert!(!account.connected());
        account.signed_in = true;
        assert!(account.connected());
    }

    #[test]
    fn errors_keep_their_wire_format() {
        let json = serde_json::to_string(&StudioError::ClaudeSignedOut).unwrap();
        assert_eq!(json, r#"{"kind":"claude_signed_out"}"#);
        let status = serde_json::to_string(&ScreenStatus::Approved).unwrap();
        assert_eq!(status, r#""approved""#);
    }
}
