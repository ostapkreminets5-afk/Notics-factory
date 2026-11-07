use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

pub const OPEN_DESIGN_PORT: u16 = 47840;
const SCREEN_WORD: &str = "screen";
const SOURCE_WORDS: [&str; 5] = ["prompt", "pinterest", "artstation", "mix", "mixed"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DesignRunStatus {
    #[default]
    Idle,
    Starting,
    WaitingReferences,
    Running,
    Done,
    Failed,
}

impl DesignRunStatus {
    pub fn is_busy(self) -> bool {
        matches!(self, Self::Starting | Self::WaitingReferences | Self::Running)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DesignSource {
    #[default]
    Prompt,
    Pinterest,
    Artstation,
    Mixed,
}

impl DesignSource {
    pub const ALL: [Self; 4] = [Self::Prompt, Self::Pinterest, Self::Artstation, Self::Mixed];

    pub fn code(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Pinterest => "pinterest",
            Self::Artstation => "artstation",
            Self::Mixed => "mix",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Prompt => "Промпт",
            Self::Pinterest => "Pinterest",
            Self::Artstation => "ArtStation",
            Self::Mixed => "Pinterest + ArtStation",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Mixed => "Pin+Art",
            other => other.label(),
        }
    }

    pub fn from_file(file: &str) -> Self {
        let name = file.rsplit(['/', '\\']).next().unwrap_or(file).to_lowercase();
        let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem);
        let words: Vec<&str> = stem.split(['-', '_', '.', ' ']).collect();
        let has = |word: &str| words.contains(&word);
        let pinterest = has(Self::Pinterest.code());
        let artstation = has(Self::Artstation.code());
        if has(Self::Mixed.code()) || has("mixed") || (pinterest && artstation) {
            Self::Mixed
        } else if pinterest {
            Self::Pinterest
        } else if artstation {
            Self::Artstation
        } else {
            Self::Prompt
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignVariant {
    pub file: String,
    pub title: String,
    pub screen: Option<u32>,
    #[serde(default)]
    pub source: DesignSource,
    pub preview_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OpenDesignBoard {
    pub project_id: i64,
    pub installed: bool,
    pub status: DesignRunStatus,
    pub design_system: Option<String>,
    pub rounds: u32,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub error: Option<String>,
    #[serde(default)]
    pub skipped: Vec<DesignSource>,
    pub variants: Vec<DesignVariant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignVariation {
    pub number: usize,
    pub source: DesignSource,
    pub round: String,
    pub variants: Vec<DesignVariant>,
}

impl DesignVariation {
    pub fn label(&self) -> String {
        format!("V{}", self.number)
    }
}

impl OpenDesignBoard {
    pub fn variations(&self) -> Vec<DesignVariation> {
        let mut groups: BTreeMap<(u32, String, DesignSource), Vec<DesignVariant>> = BTreeMap::new();
        for variant in &self.variants {
            let round = variant_round(&variant.file);
            let order = round.parse::<u32>().unwrap_or(u32::MAX);
            groups
                .entry((order, round, variant.source))
                .or_default()
                .push(variant.clone());
        }
        groups
            .into_iter()
            .enumerate()
            .map(|(index, ((_, round, source), mut variants))| {
                variants.sort_by(|left, right| {
                    left.screen
                        .unwrap_or(u32::MAX)
                        .cmp(&right.screen.unwrap_or(u32::MAX))
                        .then_with(|| left.file.cmp(&right.file))
                });
                DesignVariation {
                    number: index + 1,
                    source,
                    round,
                    variants,
                }
            })
            .collect()
    }

    pub fn for_screen(&self, position: u32) -> Vec<&DesignVariant> {
        self.variants
            .iter()
            .filter(|variant| variant.screen == Some(position))
            .collect()
    }

    pub fn count_from(&self, source: DesignSource) -> usize {
        self.variants.iter().filter(|variant| variant.source == source).count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum OpenDesignError {
    SignedOut,
    NotInstalled,
    NotFound,
    NotPlanned,
    Daemon(String),
    Storage(String),
}

impl fmt::Display for OpenDesignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignedOut => f.write_str("no account is signed in"),
            Self::NotInstalled => f.write_str("Open Design is not installed"),
            Self::NotFound => f.write_str("project or design file does not exist"),
            Self::NotPlanned => f.write_str("the game has no screen plan yet"),
            Self::Daemon(detail) => write!(f, "Open Design daemon failed: {detail}"),
            Self::Storage(detail) => write!(f, "Open Design storage failed: {detail}"),
        }
    }
}

impl std::error::Error for OpenDesignError {}

pub fn variant_screen(file: &str) -> Option<u32> {
    let name = file.rsplit(['/', '\\']).next()?;
    let digits: String = name
        .strip_prefix("screen-")?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

pub fn variant_round(file: &str) -> String {
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file).to_lowercase();
    let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem);
    let mut words: Vec<&str> = stem.split(['-', '_', '.', ' ']).filter(|word| !word.is_empty()).collect();
    let numbered = words.first() == Some(&SCREEN_WORD)
        && words
            .get(1)
            .is_some_and(|word| word.chars().all(|letter| letter.is_ascii_digit()));
    if numbered {
        words.drain(..2);
    }
    words.retain(|word| !SOURCE_WORDS.contains(word));
    words.join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_round_from_variant_files() {
        assert_eq!(variant_round("screen-01-prompt-1.html"), "1");
        assert_eq!(variant_round("designs/screen-12-pinterest-3.html"), "3");
        assert_eq!(variant_round("screen-04-pinterest-artstation-2.html"), "2");
        assert_eq!(variant_round("screen-01-b.html"), "b");
        assert_eq!(variant_round("screen-01-mix.html"), "");
        assert_eq!(variant_round("main-menu.html"), "main-menu");
    }

    #[test]
    fn splits_variants_into_whole_design_variations() {
        let variant = |file: &str| DesignVariant {
            file: file.into(),
            title: String::new(),
            screen: variant_screen(file),
            source: DesignSource::from_file(file),
            preview_url: String::new(),
        };
        let board = OpenDesignBoard {
            variants: vec![
                variant("screen-02-prompt-2.html"),
                variant("screen-02-pinterest-1.html"),
                variant("screen-01-prompt-1.html"),
                variant("screen-02-prompt-1.html"),
                variant("screen-01-pinterest-1.html"),
                variant("screen-01-prompt-2.html"),
            ],
            ..OpenDesignBoard::default()
        };
        let variations = board.variations();
        let summary: Vec<(String, DesignSource, &str, Vec<Option<u32>>)> = variations
            .iter()
            .map(|variation| {
                (
                    variation.label(),
                    variation.source,
                    variation.round.as_str(),
                    variation.variants.iter().map(|variant| variant.screen).collect(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("V1".to_owned(), DesignSource::Prompt, "1", vec![Some(1), Some(2)]),
                ("V2".to_owned(), DesignSource::Pinterest, "1", vec![Some(1), Some(2)]),
                ("V3".to_owned(), DesignSource::Prompt, "2", vec![Some(1), Some(2)]),
            ]
        );
    }

    #[test]
    fn reads_the_screen_number_from_variant_files() {
        assert_eq!(variant_screen("screen-01-a.html"), Some(1));
        assert_eq!(variant_screen("designs/screen-12-c.html"), Some(12));
        assert_eq!(variant_screen("main-menu.html"), None);
        assert_eq!(variant_screen("screen-x.html"), None);
    }

    #[test]
    fn groups_variants_by_screen_and_source() {
        let variant = |file: &str| DesignVariant {
            file: file.into(),
            title: String::new(),
            screen: variant_screen(file),
            source: DesignSource::from_file(file),
            preview_url: String::new(),
        };
        let board = OpenDesignBoard {
            variants: vec![
                variant("screen-01-a.html"),
                variant("screen-02-pinterest-1.html"),
                variant("screen-01-artstation-1.html"),
            ],
            ..OpenDesignBoard::default()
        };
        assert_eq!(board.for_screen(1).len(), 2);
        assert!(board.for_screen(3).is_empty());
        assert_eq!(board.count_from(DesignSource::Prompt), 1);
        assert_eq!(board.count_from(DesignSource::Pinterest), 1);
        assert_eq!(board.count_from(DesignSource::Mixed), 0);
        assert!(DesignRunStatus::Running.is_busy() && DesignRunStatus::WaitingReferences.is_busy());
        assert!(!DesignRunStatus::Done.is_busy());
    }

    #[test]
    fn reads_the_source_from_variant_files() {
        assert_eq!(DesignSource::from_file("screen-01-prompt-1.html"), DesignSource::Prompt);
        assert_eq!(DesignSource::from_file("screen-01-b.html"), DesignSource::Prompt);
        assert_eq!(DesignSource::from_file("designs/screen-03-pinterest-2.html"), DesignSource::Pinterest);
        assert_eq!(DesignSource::from_file("screen-03-ArtStation-1.html"), DesignSource::Artstation);
        assert_eq!(DesignSource::from_file("screen-04-mix-1.html"), DesignSource::Mixed);
        assert_eq!(DesignSource::from_file("screen-04-pinterest-artstation.html"), DesignSource::Mixed);
    }

    #[test]
    fn errors_keep_their_wire_format() {
        let json = serde_json::to_string(&OpenDesignError::NotInstalled).unwrap();
        assert_eq!(json, r#"{"kind":"not_installed"}"#);
    }
}
