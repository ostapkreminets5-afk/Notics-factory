use std::fmt;

use serde::{Deserialize, Serialize};

pub const FIRST_STAGE: u8 = 1;
pub const LAST_STAGE: u8 = 6;
pub const MAX_SKILL_NAME_CHARS: usize = 64;
pub const MAX_SKILL_TITLE_CHARS: usize = 64;
pub const MAX_SKILL_SUMMARY_CHARS: usize = 1024;
pub const MAX_SKILL_INSTRUCTIONS_CHARS: usize = 20_000;
pub const SKILL_FILE_NAME: &str = "SKILL.md";

const RESERVED_NAME_WORDS: [&str; 2] = ["anthropic", "claude"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillCategory {
    GameDesign,
    Art,
    Code,
}

impl SkillCategory {
    pub const ALL: [Self; 3] = [Self::GameDesign, Self::Art, Self::Code];

    pub fn code(self) -> &'static str {
        match self {
            Self::GameDesign => "game_design",
            Self::Art => "art",
            Self::Code => "code",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|category| category.code() == code)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDraft {
    pub name: String,
    pub stage: u8,
    pub title: String,
    pub summary: String,
    pub category: SkillCategory,
    pub instructions: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    pub id: i64,
    pub name: String,
    pub stage: u8,
    pub title: String,
    pub summary: String,
    pub category: SkillCategory,
    pub instructions: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum SkillError {
    SignedOut,
    NotFound,
    NameInvalid,
    NameTaken,
    StageOutOfRange,
    TitleMissing,
    TitleTooLong,
    SummaryMissing,
    SummaryTooLong,
    InstructionsTooLong,
    Storage(String),
}

impl fmt::Display for SkillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignedOut => f.write_str("no account is signed in"),
            Self::NotFound => f.write_str("skill does not exist"),
            Self::NameInvalid => f.write_str(
                "skill name must be lowercase latin letters, digits and single hyphens",
            ),
            Self::NameTaken => f.write_str("another skill already uses this name"),
            Self::StageOutOfRange => {
                write!(f, "stage must be between {FIRST_STAGE} and {LAST_STAGE}")
            }
            Self::TitleMissing => f.write_str("skill title is empty"),
            Self::TitleTooLong => {
                write!(f, "skill title is longer than {MAX_SKILL_TITLE_CHARS} characters")
            }
            Self::SummaryMissing => f.write_str("skill description is empty"),
            Self::SummaryTooLong => write!(
                f,
                "skill description is longer than {MAX_SKILL_SUMMARY_CHARS} characters"
            ),
            Self::InstructionsTooLong => write!(
                f,
                "skill instructions are longer than {MAX_SKILL_INSTRUCTIONS_CHARS} characters"
            ),
            Self::Storage(detail) => write!(f, "skill storage failed: {detail}"),
        }
    }
}

impl std::error::Error for SkillError {}

pub fn is_valid_skill_name(name: &str) -> bool {
    let allowed = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    let well_formed = !name.starts_with('-') && !name.ends_with('-') && !name.contains("--");
    let reserved = RESERVED_NAME_WORDS.iter().any(|word| name.contains(word));
    !name.is_empty() && name.len() <= MAX_SKILL_NAME_CHARS && allowed && well_formed && !reserved
}

pub fn suggest_skill_name(title: &str) -> String {
    let mut name = String::new();
    for c in title.to_lowercase().chars() {
        match transliterate(c) {
            Some(latin) => name.push_str(latin),
            None if c.is_ascii_alphanumeric() => name.push(c),
            None if !name.ends_with('-') && !name.is_empty() => name.push('-'),
            None => {}
        }
    }
    let mut name: String = name.chars().take(MAX_SKILL_NAME_CHARS).collect();
    while name.ends_with('-') {
        name.pop();
    }
    name
}

fn transliterate(c: char) -> Option<&'static str> {
    let latin = match c {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "h",
        'ґ' => "g",
        'д' => "d",
        'е' => "e",
        'є' => "ie",
        'ж' => "zh",
        'з' => "z",
        'и' => "y",
        'і' => "i",
        'ї' => "i",
        'й' => "i",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "kh",
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "shch",
        'ю' => "iu",
        'я' => "ia",
        'ь' | '’' | '\'' | 'ʼ' => "",
        _ => return None,
    };
    Some(latin)
}

impl SkillDraft {
    pub fn normalized(&self) -> Self {
        Self {
            name: self.name.trim().to_lowercase(),
            stage: self.stage,
            title: self.title.trim().to_owned(),
            summary: single_line(&self.summary),
            category: self.category,
            instructions: self.instructions.trim().to_owned(),
            enabled: self.enabled,
        }
    }

    pub fn validate(&self) -> Result<(), SkillError> {
        let draft = self.normalized();
        if !is_valid_skill_name(&draft.name) {
            return Err(SkillError::NameInvalid);
        }
        if !(FIRST_STAGE..=LAST_STAGE).contains(&draft.stage) {
            return Err(SkillError::StageOutOfRange);
        }
        match draft.title.chars().count() {
            0 => return Err(SkillError::TitleMissing),
            count if count > MAX_SKILL_TITLE_CHARS => return Err(SkillError::TitleTooLong),
            _ => {}
        }
        match draft.summary.chars().count() {
            0 => return Err(SkillError::SummaryMissing),
            count if count > MAX_SKILL_SUMMARY_CHARS => return Err(SkillError::SummaryTooLong),
            _ => {}
        }
        if draft.instructions.chars().count() > MAX_SKILL_INSTRUCTIONS_CHARS {
            return Err(SkillError::InstructionsTooLong);
        }
        Ok(())
    }
}

impl Skill {
    pub fn draft(&self) -> SkillDraft {
        SkillDraft {
            name: self.name.clone(),
            stage: self.stage,
            title: self.title.clone(),
            summary: self.summary.clone(),
            category: self.category,
            instructions: self.instructions.clone(),
            enabled: self.enabled,
        }
    }

    pub fn stage_code(&self) -> String {
        format!("{:02}", self.stage)
    }

    pub fn to_skill_markdown(&self) -> String {
        let instructions = if self.instructions.trim().is_empty() {
            self.summary.clone()
        } else {
            self.instructions.trim().to_owned()
        };
        format!(
            "---\nname: {name}\ndescription: {description}\nmetadata:\n  title: {title}\n  stage: \"{stage}\"\n  category: {category}\n---\n\n# {heading}\n\n{instructions}\n",
            name = self.name,
            description = yaml_quoted(&self.summary),
            title = yaml_quoted(&self.title),
            stage = self.stage_code(),
            category = self.category.code(),
            heading = self.title.trim(),
        )
    }
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn yaml_quoted(text: &str) -> String {
    let escaped = single_line(text).replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> SkillDraft {
        SkillDraft {
            name: "  Lore-And-Naming ".into(),
            stage: 1,
            title: "  Лор і наймінг ".into(),
            summary: "Світ, імена,\n  назви рівнів.".into(),
            category: SkillCategory::GameDesign,
            instructions: "\n- Пиши коротко.\n".into(),
            enabled: true,
        }
    }

    #[test]
    fn normalizes_before_storing() {
        let normalized = draft().normalized();
        assert_eq!(normalized.name, "lore-and-naming");
        assert_eq!(normalized.title, "Лор і наймінг");
        assert_eq!(normalized.summary, "Світ, імена, назви рівнів.");
        assert_eq!(normalized.instructions, "- Пиши коротко.");
        assert_eq!(draft().validate(), Ok(()));
    }

    #[test]
    fn accepts_only_agent_skill_names() {
        for name in ["idea-to-gdd", "model-spec", "stage6"] {
            assert!(is_valid_skill_name(name), "{name}");
        }
        for name in [
            "",
            "-lead",
            "trail-",
            "double--dash",
            "Upper",
            "з-кирилицею",
            "with space",
            "claude-helper",
            "my-anthropic",
        ] {
            assert!(!is_valid_skill_name(name), "{name}");
        }
        assert!(!is_valid_skill_name(&"a".repeat(MAX_SKILL_NAME_CHARS + 1)));
    }

    #[test]
    fn rejects_incomplete_drafts() {
        let mut stage = draft();
        stage.stage = LAST_STAGE + 1;
        assert_eq!(stage.validate(), Err(SkillError::StageOutOfRange));

        let mut title = draft();
        title.title = "   ".into();
        assert_eq!(title.validate(), Err(SkillError::TitleMissing));

        let mut summary = draft();
        summary.summary = "\n".into();
        assert_eq!(summary.validate(), Err(SkillError::SummaryMissing));

        let mut name = draft();
        name.name = "лор".into();
        assert_eq!(name.validate(), Err(SkillError::NameInvalid));
    }

    #[test]
    fn suggests_latin_names_from_ukrainian_titles() {
        assert_eq!(suggest_skill_name("Лор і наймінг"), "lor-i-naiminh");
        assert_eq!(suggest_skill_name("Ідея → дизайн-документ"), "ideia-dyzain-dokument");
        assert_eq!(suggest_skill_name("Скрипти гри 2D"), "skrypty-hry-2d");
        assert_eq!(suggest_skill_name("  ---  "), "");
        assert!(is_valid_skill_name(&suggest_skill_name("Стиль матеріалів")));
    }

    #[test]
    fn renders_a_skill_file_claude_can_load() {
        let skill = Skill {
            id: 7,
            name: "lore-and-naming".into(),
            stage: 1,
            title: "Лор і наймінг".into(),
            summary: "Імена \"героїв\" і рівнів".into(),
            category: SkillCategory::GameDesign,
            instructions: "- Пиши коротко.".into(),
            enabled: true,
        };
        assert_eq!(
            skill.to_skill_markdown(),
            "---\nname: lore-and-naming\ndescription: \"Імена \\\"героїв\\\" і рівнів\"\nmetadata:\n  title: \"Лор і наймінг\"\n  stage: \"01\"\n  category: game_design\n---\n\n# Лор і наймінг\n\n- Пиши коротко.\n"
        );
    }

    #[test]
    fn errors_keep_their_wire_format() {
        let json = serde_json::to_string(&SkillError::NameTaken).unwrap();
        assert_eq!(json, r#"{"kind":"name_taken"}"#);
        let category = serde_json::to_string(&SkillCategory::GameDesign).unwrap();
        assert_eq!(category, r#""game_design""#);
        assert_eq!(SkillCategory::from_code("art"), Some(SkillCategory::Art));
    }
}
