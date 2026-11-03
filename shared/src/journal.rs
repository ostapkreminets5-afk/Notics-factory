use serde::{Deserialize, Serialize};

use crate::{BuildTarget, FIRST_STAGE, GameEngine, INTERFACE_STAGE, LAST_STAGE, StudioError};

pub const JOURNAL_LIMIT: u32 = 500;
const REFERENCE_STAGE: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalSource {
    Core,
    Skills,
    References,
    Interface,
}

impl JournalSource {
    pub const ALL: [Self; 4] = [Self::Core, Self::Skills, Self::References, Self::Interface];

    pub fn code(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Skills => "skills",
            Self::References => "references",
            Self::Interface => "interface",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Core => "Ядро",
            Self::Skills => "Skills",
            Self::References => "Pinterest",
            Self::Interface => "Claude Design",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JournalTone {
    Info,
    Working,
    Done,
    Failed,
}

impl JournalTone {
    pub fn code(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Working => "working",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum JournalEvent {
    ProjectStarted { engine: GameEngine },
    PlanReady { name: String, screens: u32, skills: u32 },
    PlanFailed { error: StudioError },
    SkillsSynced { count: u32 },
    SkillStarted { title: String, stage: u8 },
    SkillDone { title: String, stage: u8 },
    SkillFailed { title: String, stage: u8, error: StudioError },
    SkillRequeued { title: String, stage: u8 },
    ScreenAdded { title: String },
    ScreenStarted { title: String, revision: bool },
    ScreenDone { title: String, version: u32 },
    ScreenFailed { title: String, error: StudioError },
    ScreenRefined { title: String },
    ScreenRequeued { title: String },
    ScreenApproved { title: String },
    BoardsPicked { count: u32 },
    PinsImported { board: String, count: u32 },
    PinsMissing,
    ProjectFormed { name: String },
    ProjectRenamed { name: String },
    EngineChanged { engine: GameEngine },
    RetryQueued { count: u32 },
    CodeStarted { fresh: bool },
    CodeReady { files: u32 },
    CodeFailed { reason: String },
    BuildStarted { target: BuildTarget, toolchain: String },
    BuildDone { target: BuildTarget, size_bytes: u64 },
    BuildFailed { target: BuildTarget, reason: String },
    BuildCancelled { target: BuildTarget },
    BuildSkipped { target: BuildTarget, reason: String },
}

impl JournalEvent {
    pub fn source(&self) -> JournalSource {
        match self {
            Self::ProjectStarted { .. }
            | Self::PlanReady { .. }
            | Self::PlanFailed { .. }
            | Self::ProjectFormed { .. }
            | Self::ProjectRenamed { .. }
            | Self::EngineChanged { .. }
            | Self::RetryQueued { .. }
            | Self::CodeStarted { .. }
            | Self::CodeReady { .. }
            | Self::CodeFailed { .. }
            | Self::BuildStarted { .. }
            | Self::BuildDone { .. }
            | Self::BuildFailed { .. }
            | Self::BuildCancelled { .. }
            | Self::BuildSkipped { .. } => JournalSource::Core,
            Self::SkillsSynced { .. }
            | Self::SkillStarted { .. }
            | Self::SkillDone { .. }
            | Self::SkillFailed { .. }
            | Self::SkillRequeued { .. } => JournalSource::Skills,
            Self::ScreenAdded { .. }
            | Self::ScreenStarted { .. }
            | Self::ScreenDone { .. }
            | Self::ScreenFailed { .. }
            | Self::ScreenRefined { .. }
            | Self::ScreenRequeued { .. }
            | Self::ScreenApproved { .. } => JournalSource::Interface,
            Self::BoardsPicked { .. } | Self::PinsImported { .. } | Self::PinsMissing => {
                JournalSource::References
            }
        }
    }

    pub fn tone(&self) -> JournalTone {
        match self {
            Self::SkillStarted { .. }
            | Self::ScreenStarted { .. }
            | Self::CodeStarted { .. }
            | Self::BuildStarted { .. } => JournalTone::Working,
            Self::PlanReady { .. }
            | Self::SkillDone { .. }
            | Self::ScreenDone { .. }
            | Self::ScreenApproved { .. }
            | Self::BoardsPicked { .. }
            | Self::PinsImported { .. }
            | Self::ProjectFormed { .. }
            | Self::CodeReady { .. }
            | Self::BuildDone { .. } => JournalTone::Done,
            Self::PlanFailed { .. }
            | Self::SkillFailed { .. }
            | Self::ScreenFailed { .. }
            | Self::PinsMissing
            | Self::CodeFailed { .. }
            | Self::BuildFailed { .. } => JournalTone::Failed,
            Self::ProjectStarted { .. }
            | Self::SkillsSynced { .. }
            | Self::SkillRequeued { .. }
            | Self::ScreenAdded { .. }
            | Self::ScreenRefined { .. }
            | Self::ScreenRequeued { .. }
            | Self::ProjectRenamed { .. }
            | Self::EngineChanged { .. }
            | Self::RetryQueued { .. }
            | Self::BuildCancelled { .. }
            | Self::BuildSkipped { .. } => JournalTone::Info,
        }
    }

    pub fn stage(&self) -> Option<u8> {
        match self {
            Self::ProjectStarted { .. } | Self::PlanReady { .. } | Self::PlanFailed { .. } => {
                Some(FIRST_STAGE)
            }
            Self::SkillStarted { stage, .. }
            | Self::SkillDone { stage, .. }
            | Self::SkillFailed { stage, .. }
            | Self::SkillRequeued { stage, .. } => Some(*stage),
            Self::ScreenAdded { .. }
            | Self::ScreenStarted { .. }
            | Self::ScreenDone { .. }
            | Self::ScreenFailed { .. }
            | Self::ScreenRefined { .. }
            | Self::ScreenRequeued { .. }
            | Self::ScreenApproved { .. } => Some(INTERFACE_STAGE),
            Self::BoardsPicked { .. } | Self::PinsImported { .. } | Self::PinsMissing => {
                Some(REFERENCE_STAGE)
            }
            Self::CodeStarted { .. }
            | Self::CodeReady { .. }
            | Self::CodeFailed { .. }
            | Self::BuildStarted { .. }
            | Self::BuildDone { .. }
            | Self::BuildFailed { .. }
            | Self::BuildCancelled { .. }
            | Self::BuildSkipped { .. } => Some(LAST_STAGE),
            Self::SkillsSynced { .. }
            | Self::ProjectFormed { .. }
            | Self::ProjectRenamed { .. }
            | Self::EngineChanged { .. }
            | Self::RetryQueued { .. } => None,
        }
    }

    pub fn error(&self) -> Option<&StudioError> {
        match self {
            Self::PlanFailed { error }
            | Self::SkillFailed { error, .. }
            | Self::ScreenFailed { error, .. } => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: i64,
    pub project_id: i64,
    pub event: JournalEvent,
    pub created_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_keep_their_wire_format() {
        let event = JournalEvent::SkillDone {
            title: "Ідея → дизайн-документ".into(),
            stage: 1,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"skill_done","detail":{"title":"Ідея → дизайн-документ","stage":1}}"#
        );
        assert_eq!(serde_json::from_str::<JournalEvent>(&json).unwrap(), event);
        let missing = serde_json::to_string(&JournalEvent::PinsMissing).unwrap();
        assert_eq!(missing, r#"{"kind":"pins_missing"}"#);
        let failed = JournalEvent::ScreenFailed {
            title: "HUD".into(),
            error: StudioError::UsageLimit,
        };
        let round_trip: JournalEvent =
            serde_json::from_str(&serde_json::to_string(&failed).unwrap()).unwrap();
        assert_eq!(round_trip.error(), Some(&StudioError::UsageLimit));
    }

    #[test]
    fn events_belong_to_the_system_that_produced_them() {
        let started = JournalEvent::ProjectStarted {
            engine: GameEngine::Godot,
        };
        assert_eq!(started.source(), JournalSource::Core);
        assert_eq!(started.stage(), Some(FIRST_STAGE));

        let skill = JournalEvent::SkillStarted {
            title: "Скрипти гри".into(),
            stage: 6,
        };
        assert_eq!(
            (skill.source(), skill.tone(), skill.stage()),
            (JournalSource::Skills, JournalTone::Working, Some(6))
        );

        let screen = JournalEvent::ScreenApproved { title: "HUD".into() };
        assert_eq!(
            (screen.source(), screen.tone(), screen.stage()),
            (JournalSource::Interface, JournalTone::Done, Some(INTERFACE_STAGE))
        );

        let build = JournalEvent::BuildFailed {
            target: BuildTarget::Macos,
            reason: "Parse Error".into(),
        };
        assert_eq!(
            (build.source(), build.tone(), build.stage()),
            (JournalSource::Core, JournalTone::Failed, Some(LAST_STAGE))
        );

        assert_eq!(JournalEvent::PinsMissing.source(), JournalSource::References);
        assert_eq!(JournalEvent::PinsMissing.tone(), JournalTone::Failed);
        assert_eq!(
            JournalEvent::ProjectRenamed { name: "Тіні".into() }.stage(),
            None
        );
    }
}
