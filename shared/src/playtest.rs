use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaytestStatus {
    Capturing,
    Reviewing,
    Done,
    Failed,
}

impl PlaytestStatus {
    pub const ALL: [Self; 4] = [Self::Capturing, Self::Reviewing, Self::Done, Self::Failed];

    pub fn code(self) -> &'static str {
        match self {
            Self::Capturing => "capturing",
            Self::Reviewing => "reviewing",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.code() == code)
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Capturing | Self::Reviewing)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaytestVerdict {
    Passed,
    Fixed,
    NeedsWork,
}

impl PlaytestVerdict {
    pub fn label(self) -> &'static str {
        match self {
            Self::Passed => "Гра пройшла плейтест",
            Self::Fixed => "Проблеми знайдено й виправлено",
            Self::NeedsWork => "Лишились проблеми",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaytestIssue {
    pub text: String,
    #[serde(default)]
    pub fixed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaytestReport {
    pub verdict: PlaytestVerdict,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub issues: Vec<PlaytestIssue>,
}

impl PlaytestReport {
    pub fn fixed(&self) -> usize {
        self.issues.iter().filter(|issue| issue.fixed).count()
    }

    pub fn open(&self) -> usize {
        self.issues.iter().filter(|issue| !issue.fixed).count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaytestShot {
    pub label: String,
    pub second: u32,
    pub scene: String,
    pub blank: bool,
    pub image: String,
    pub thumb: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playtest {
    pub project_id: i64,
    pub status: PlaytestStatus,
    pub report: Option<PlaytestReport>,
    pub shots: Vec<PlaytestShot>,
    pub scenes: Vec<String>,
    pub errors: Vec<String>,
    pub activity: Vec<String>,
    pub finished_early: bool,
    pub error: Option<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaytestOverview {
    pub blocked: Option<String>,
    pub run: Option<Playtest>,
}

impl PlaytestOverview {
    pub fn is_active(&self) -> bool {
        self.run.as_ref().is_some_and(|run| run.status.is_active())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip() {
        for status in PlaytestStatus::ALL {
            assert_eq!(PlaytestStatus::from_code(status.code()), Some(status));
        }
        assert!(PlaytestStatus::Reviewing.is_active());
        assert!(!PlaytestStatus::Done.is_active());
        assert_eq!(PlaytestStatus::from_code("paused"), None);
    }

    #[test]
    fn reads_the_report_claude_writes() {
        let report: PlaytestReport = serde_json::from_str(
            r#"{"verdict":"fixed","summary":"Меню не запускало гру","issues":[{"text":"Кнопка «Почати» не працювала","fixed":true},{"text":"Дрібний шрифт HUD"}]}"#,
        )
        .unwrap();
        assert_eq!(report.verdict, PlaytestVerdict::Fixed);
        assert_eq!((report.fixed(), report.open()), (1, 1));
        let bare: PlaytestReport = serde_json::from_str(r#"{"verdict":"passed"}"#).unwrap();
        assert!(bare.summary.is_empty() && bare.issues.is_empty());
    }
}
