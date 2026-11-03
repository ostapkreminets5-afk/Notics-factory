use std::fmt;

use serde::{Deserialize, Serialize};

pub const MAX_TOOL_NAME_CHARS: usize = 32;
pub const MAX_TOOL_NOTE_CHARS: usize = 48;
pub const MAX_TOOL_URL_CHARS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSlot {
    Core,
    Images,
    Interface,
    References,
    Models,
    Build,
}

impl ToolSlot {
    pub const ALL: [Self; 6] = [
        Self::Core,
        Self::Images,
        Self::Interface,
        Self::References,
        Self::Models,
        Self::Build,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Images => "images",
            Self::Interface => "interface",
            Self::References => "references",
            Self::Models => "models",
            Self::Build => "build",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|slot| slot.code() == code)
    }

    pub fn purpose(self) -> &'static str {
        match self {
            Self::Core => "Центр усіх систем",
            Self::Images => "Генерація зображень",
            Self::Interface => "UI та екрани гри",
            Self::References => "Референси й настрій",
            Self::Models => "Бібліотека моделей",
            Self::Build => "Білди гри",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    pub id: String,
    pub slot: ToolSlot,
    pub name: String,
    pub note: String,
    pub url: Option<String>,
    pub custom: bool,
    pub integrated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveTool {
    pub slot: ToolSlot,
    pub tool_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Toolbox {
    pub tools: Vec<Tool>,
    pub active: Vec<ActiveTool>,
}

impl Toolbox {
    pub fn tools_for(&self, slot: ToolSlot) -> impl Iterator<Item = &Tool> {
        self.tools.iter().filter(move |tool| tool.slot == slot)
    }

    pub fn active_tool(&self, slot: ToolSlot) -> Option<&Tool> {
        let chosen = self
            .active
            .iter()
            .find(|active| active.slot == slot)
            .map(|active| active.tool_id.as_str());
        chosen
            .and_then(|id| self.tools_for(slot).find(|tool| tool.id == id))
            .or_else(|| self.tools_for(slot).next())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDraft {
    pub slot: ToolSlot,
    pub name: String,
    pub note: String,
    pub url: String,
}

impl ToolDraft {
    pub fn normalized(&self) -> Result<ToolDraft, ToolError> {
        let name = single_line(&self.name);
        match name.chars().count() {
            0 => return Err(ToolError::NameMissing),
            count if count > MAX_TOOL_NAME_CHARS => return Err(ToolError::NameTooLong),
            _ => {}
        }
        let note = single_line(&self.note);
        if note.chars().count() > MAX_TOOL_NOTE_CHARS {
            return Err(ToolError::NoteTooLong);
        }
        let url = self.url.trim().to_owned();
        if !url.is_empty() && !is_web_url(&url) {
            return Err(ToolError::InvalidUrl);
        }
        Ok(ToolDraft {
            slot: self.slot,
            name,
            note,
            url,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum ToolError {
    SignedOut,
    NotFound,
    BuiltIn,
    NameMissing,
    NameTooLong,
    NoteTooLong,
    InvalidUrl,
    NoUrl,
    Storage(String),
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignedOut => f.write_str("no account is signed in"),
            Self::NotFound => f.write_str("tool does not exist in this slot"),
            Self::BuiltIn => f.write_str("built-in tools cannot be removed"),
            Self::NameMissing => f.write_str("tool name is empty"),
            Self::NameTooLong => {
                write!(f, "tool name is longer than {MAX_TOOL_NAME_CHARS} characters")
            }
            Self::NoteTooLong => {
                write!(f, "tool note is longer than {MAX_TOOL_NOTE_CHARS} characters")
            }
            Self::InvalidUrl => f.write_str("tool link must be an http or https address"),
            Self::NoUrl => f.write_str("tool has no link to open"),
            Self::Storage(detail) => write!(f, "toolbox storage failed: {detail}"),
        }
    }
}

impl std::error::Error for ToolError {}

pub fn is_web_url(url: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or_default();
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !host.is_empty()
        && url.chars().count() <= MAX_TOOL_URL_CHARS
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(id: &str, slot: ToolSlot) -> Tool {
        Tool {
            id: id.into(),
            slot,
            name: id.into(),
            note: String::new(),
            url: None,
            custom: false,
            integrated: false,
        }
    }

    #[test]
    fn falls_back_to_the_first_tool_of_a_slot() {
        let mut toolbox = Toolbox {
            tools: vec![
                tool("comfyui", ToolSlot::Images),
                tool("midjourney", ToolSlot::Images),
                tool("figma", ToolSlot::Interface),
            ],
            active: vec![],
        };
        assert_eq!(toolbox.active_tool(ToolSlot::Images).unwrap().id, "comfyui");
        toolbox.active.push(ActiveTool {
            slot: ToolSlot::Images,
            tool_id: "midjourney".into(),
        });
        assert_eq!(toolbox.active_tool(ToolSlot::Images).unwrap().id, "midjourney");
        toolbox.active.push(ActiveTool {
            slot: ToolSlot::Interface,
            tool_id: "midjourney".into(),
        });
        assert_eq!(toolbox.active_tool(ToolSlot::Interface).unwrap().id, "figma");
        assert!(toolbox.active_tool(ToolSlot::Build).is_none());
    }

    #[test]
    fn validates_custom_tools() {
        let draft = |name: &str, url: &str| ToolDraft {
            slot: ToolSlot::Models,
            name: name.into(),
            note: "  Генерація   3D ".into(),
            url: url.into(),
        };
        let ok = draft("  Meshy  AI ", " https://www.meshy.ai ").normalized().unwrap();
        assert_eq!((ok.name.as_str(), ok.note.as_str(), ok.url.as_str()), ("Meshy AI", "Генерація 3D", "https://www.meshy.ai"));
        assert_eq!(draft("Blender", "").normalized().unwrap().url, "");
        assert_eq!(draft(" ", "").normalized(), Err(ToolError::NameMissing));
        assert_eq!(draft("Tool", "ftp://files").normalized(), Err(ToolError::InvalidUrl));
        assert_eq!(draft("Tool", "javascript:alert(1)").normalized(), Err(ToolError::InvalidUrl));
        assert_eq!(draft("Tool", "https://").normalized(), Err(ToolError::InvalidUrl));
        assert_eq!(
            draft(&"x".repeat(MAX_TOOL_NAME_CHARS + 1), "").normalized(),
            Err(ToolError::NameTooLong)
        );
    }

    #[test]
    fn slots_round_trip_through_codes() {
        for slot in ToolSlot::ALL {
            assert_eq!(ToolSlot::from_code(slot.code()), Some(slot));
        }
        let json = serde_json::to_string(&ToolError::BuiltIn).unwrap();
        assert_eq!(json, r#"{"kind":"built_in"}"#);
    }
}
