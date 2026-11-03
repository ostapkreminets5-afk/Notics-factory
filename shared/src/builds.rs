use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{GameEngine, StudioError};

pub const MAX_BUILD_REQUEST_CHARS: usize = 2000;
pub const MAX_SOURCE_PATH_CHARS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildFamily {
    Desktop,
    Web,
    Mobile,
}

impl BuildFamily {
    pub const ALL: [Self; 3] = [Self::Desktop, Self::Web, Self::Mobile];

    pub fn label(self) -> &'static str {
        match self {
            Self::Desktop => "Десктоп",
            Self::Web => "Веб",
            Self::Mobile => "Мобільні",
        }
    }

    pub fn targets(self) -> impl Iterator<Item = BuildTarget> {
        BuildTarget::ALL
            .into_iter()
            .filter(move |target| target.family() == self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildTarget {
    Windows,
    Macos,
    Linux,
    Web,
    Android,
    Ios,
}

impl BuildTarget {
    pub const ALL: [Self; 6] = [
        Self::Windows,
        Self::Macos,
        Self::Linux,
        Self::Web,
        Self::Android,
        Self::Ios,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Web => "web",
            Self::Android => "android",
            Self::Ios => "ios",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|target| target.code() == code)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::Macos => "macOS",
            Self::Linux => "Linux",
            Self::Web => "Web",
            Self::Android => "Android",
            Self::Ios => "iOS",
        }
    }

    pub fn package(self) -> &'static str {
        match self {
            Self::Windows => ".exe",
            Self::Macos => ".app",
            Self::Linux => "x86_64",
            Self::Web => "HTML5",
            Self::Android => ".apk",
            Self::Ios => ".ipa",
        }
    }

    pub fn family(self) -> BuildFamily {
        match self {
            Self::Windows | Self::Macos | Self::Linux => BuildFamily::Desktop,
            Self::Web => BuildFamily::Web,
            Self::Android | Self::Ios => BuildFamily::Mobile,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl BuildStatus {
    pub const ALL: [Self; 5] = [
        Self::Queued,
        Self::Running,
        Self::Done,
        Self::Failed,
        Self::Cancelled,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.code() == code)
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum Readiness {
    Ready(String),
    Missing(String),
    NeedsHost(String),
    Unsupported,
}

impl Readiness {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetReport {
    pub target: BuildTarget,
    pub readiness: Readiness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceOrigin {
    Generated,
    Linked,
}

impl SourceOrigin {
    pub const ALL: [Self; 2] = [Self::Generated, Self::Linked];

    pub fn code(self) -> &'static str {
        match self {
            Self::Generated => "generated",
            Self::Linked => "linked",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|origin| origin.code() == code)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    Generating,
    Ready,
    Failed,
}

impl SourceStatus {
    pub const ALL: [Self; 3] = [Self::Generating, Self::Ready, Self::Failed];

    pub fn code(self) -> &'static str {
        match self {
            Self::Generating => "generating",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.code() == code)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameSource {
    pub path: String,
    pub origin: SourceOrigin,
    pub status: SourceStatus,
    pub engine: GameEngine,
    pub valid: bool,
    pub activity: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    pub id: i64,
    pub project_id: i64,
    pub target: BuildTarget,
    pub status: BuildStatus,
    pub artifact: Option<String>,
    pub size_bytes: Option<u64>,
    pub log_tail: String,
    pub error: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildOverview {
    pub project_id: i64,
    pub engine: GameEngine,
    pub source: Option<GameSource>,
    pub targets: Vec<TargetReport>,
    pub builds: Vec<Build>,
    pub output_dir: String,
    #[serde(default)]
    pub planned: Vec<BuildTarget>,
}

impl BuildOverview {
    pub fn readiness(&self, target: BuildTarget) -> Readiness {
        self.targets
            .iter()
            .find(|report| report.target == target)
            .map_or(Readiness::Unsupported, |report| report.readiness.clone())
    }

    pub fn can_build(&self) -> bool {
        self.source
            .as_ref()
            .is_some_and(|source| source.valid && source.status != SourceStatus::Generating)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum BuildError {
    SignedOut,
    NotFound,
    NoSource,
    SourceBusy,
    SourceInvalid,
    PathInvalid,
    RequestTooLong,
    NothingSelected,
    TargetUnavailable(BuildTarget),
    Claude(StudioError),
    Process(String),
    Storage(String),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignedOut => f.write_str("no account is signed in"),
            Self::NotFound => f.write_str("project or build does not exist"),
            Self::NoSource => f.write_str("the project has no game source yet"),
            Self::SourceBusy => f.write_str("the game source is being generated"),
            Self::SourceInvalid => f.write_str("the folder is not a project of the chosen engine"),
            Self::PathInvalid => f.write_str("the folder does not exist"),
            Self::RequestTooLong => {
                write!(f, "request is longer than {MAX_BUILD_REQUEST_CHARS} characters")
            }
            Self::NothingSelected => f.write_str("no build target was selected"),
            Self::TargetUnavailable(target) => {
                write!(f, "{} cannot be built on this machine", target.label())
            }
            Self::Claude(problem) => write!(f, "{problem}"),
            Self::Process(detail) => write!(f, "build process failed: {detail}"),
            Self::Storage(detail) => write!(f, "build storage failed: {detail}"),
        }
    }
}

impl std::error::Error for BuildError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GamesFolder {
    pub path: String,
    pub is_default: bool,
}

pub fn project_marker(engine: GameEngine) -> &'static str {
    match engine {
        GameEngine::Godot => "project.godot",
        GameEngine::Unity => "ProjectSettings/ProjectVersion.txt",
        GameEngine::Unreal => ".uproject",
        GameEngine::ThreeJs | GameEngine::BabylonJs | GameEngine::Phaser => "package.json",
        GameEngine::Bevy | GameEngine::ThreeD | GameEngine::Penumbra => "Cargo.toml",
        GameEngine::Defold => "game.project",
    }
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["Б", "КБ", "МБ", "ГБ"];
    const STEP: f64 = 1024.0;
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= STEP && unit + 1 < UNITS.len() {
        value /= STEP;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit]).replace('.', ",")
    }
}

pub fn format_duration(seconds: i64) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    let seconds = seconds.max(0);
    match seconds {
        s if s < MINUTE => format!("{s} с"),
        s if s < HOUR => format!("{} хв {} с", s / MINUTE, s % MINUTE),
        s => format!("{} год {} хв", s / HOUR, s % HOUR / MINUTE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for target in BuildTarget::ALL {
            assert_eq!(BuildTarget::from_code(target.code()), Some(target));
            let wire = serde_json::to_string(&target).unwrap();
            assert_eq!(wire, format!("\"{}\"", target.code()));
        }
        for status in BuildStatus::ALL {
            assert_eq!(BuildStatus::from_code(status.code()), Some(status));
        }
        for origin in SourceOrigin::ALL {
            assert_eq!(SourceOrigin::from_code(origin.code()), Some(origin));
        }
        for status in SourceStatus::ALL {
            assert_eq!(SourceStatus::from_code(status.code()), Some(status));
        }
        assert_eq!(BuildTarget::from_code("playstation"), None);
    }

    #[test]
    fn every_target_belongs_to_one_family() {
        let grouped: usize = BuildFamily::ALL
            .iter()
            .map(|family| family.targets().count())
            .sum();
        assert_eq!(grouped, BuildTarget::ALL.len());
        assert_eq!(
            BuildFamily::Mobile.targets().collect::<Vec<_>>(),
            [BuildTarget::Android, BuildTarget::Ios]
        );
    }

    #[test]
    fn errors_keep_their_wire_format() {
        let json = serde_json::to_string(&BuildError::TargetUnavailable(BuildTarget::Ios)).unwrap();
        assert_eq!(json, r#"{"kind":"target_unavailable","detail":"ios"}"#);
        let nested = BuildError::Claude(StudioError::UsageLimit);
        let round_trip: BuildError =
            serde_json::from_str(&serde_json::to_string(&nested).unwrap()).unwrap();
        assert_eq!(round_trip, nested);
        let ready = serde_json::to_string(&Readiness::Ready("Godot 4.3".into())).unwrap();
        assert_eq!(ready, r#"{"kind":"ready","detail":"Godot 4.3"}"#);
    }

    #[test]
    fn formats_sizes_and_durations() {
        assert_eq!(format_size(512), "512 Б");
        assert_eq!(format_size(1536), "1,5 КБ");
        assert_eq!(format_size(48 * 1024 * 1024), "48,0 МБ");
        assert_eq!(format_duration(42), "42 с");
        assert_eq!(format_duration(72), "1 хв 12 с");
        assert_eq!(format_duration(3720), "1 год 2 хв");
    }
}
