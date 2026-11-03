use std::fmt;

use serde::{Deserialize, Serialize};

pub const MAX_ARTIFACT_TITLE_CHARS: usize = 80;
pub const MAX_ARTIFACT_LINK_CHARS: usize = 300;
pub const ARTIFACTS_CHANGED_EVENT: &str = "artifacts-changed";

const ARTIFACT_ORIGIN: &str = "https://claude.ai";
const ARTIFACT_PATHS: [&str; 2] = ["/artifact/", "/public/artifacts/"];
const ACCEPTED_HOSTS: [&str; 2] = ["claude.ai", "www.claude.ai"];
const SCHEMES: [&str; 2] = ["https://", "http://"];
const GENERIC_PAGE_TITLES: [&str; 2] = ["claude", "claude.ai"];
const PAGE_TITLE_SUFFIXES: [&str; 6] = [
    " - Claude",
    " | Claude",
    " · Claude",
    " — Claude",
    " – Claude",
    " - Claude.ai",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactOrigin {
    Manual,
    Session,
}

impl ArtifactOrigin {
    pub const ALL: [Self; 2] = [Self::Manual, Self::Session];

    pub fn code(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Session => "session",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|origin| origin.code() == code)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignArtifact {
    pub id: i64,
    pub title: String,
    pub link: String,
    pub custom_title: bool,
    pub origin: ArtifactOrigin,
    pub added_at: i64,
}

impl DesignArtifact {
    pub fn code(&self) -> &str {
        artifact_code(&self.link)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactDraft {
    pub title: String,
    pub link: String,
}

impl ArtifactDraft {
    pub fn normalized(&self) -> Result<ArtifactDraft, ArtifactError> {
        let link = artifact_link(&self.link).ok_or(ArtifactError::InvalidLink)?;
        let title = single_line(&self.title);
        if title.chars().count() > MAX_ARTIFACT_TITLE_CHARS {
            return Err(ArtifactError::TitleTooLong);
        }
        Ok(ArtifactDraft { title, link })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum ArtifactError {
    SignedOut,
    NotFound,
    InvalidLink,
    Duplicate,
    TitleTooLong,
    Window(String),
    Storage(String),
}

impl fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignedOut => f.write_str("no account is signed in"),
            Self::NotFound => f.write_str("artifact does not exist"),
            Self::InvalidLink => f.write_str("link is not a claude.ai artifact"),
            Self::Duplicate => f.write_str("this artifact is already on the shelf"),
            Self::TitleTooLong => {
                write!(f, "artifact name is longer than {MAX_ARTIFACT_TITLE_CHARS} characters")
            }
            Self::Window(detail) => write!(f, "artifact window failed: {detail}"),
            Self::Storage(detail) => write!(f, "artifact storage failed: {detail}"),
        }
    }
}

impl std::error::Error for ArtifactError {}

pub fn artifact_link(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.chars().count() > MAX_ARTIFACT_LINK_CHARS {
        return None;
    }
    let without_scheme = SCHEMES
        .iter()
        .find_map(|scheme| trimmed.strip_prefix(scheme))
        .unwrap_or(trimmed);
    let address = without_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_end_matches('/');
    let (host, path) = address.split_at(address.find('/')?);
    if !ACCEPTED_HOSTS.contains(&host.to_ascii_lowercase().as_str()) {
        return None;
    }
    let (prefix, code) = ARTIFACT_PATHS
        .iter()
        .find_map(|prefix| path.strip_prefix(prefix).map(|code| (*prefix, code)))?;
    let valid_code = !code.is_empty()
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid_code.then(|| format!("{ARTIFACT_ORIGIN}{prefix}{code}"))
}

pub fn artifact_code(link: &str) -> &str {
    link.rsplit('/').next().unwrap_or_default()
}

pub fn title_from_page(page_title: &str) -> Option<String> {
    let mut title = single_line(page_title);
    for suffix in PAGE_TITLE_SUFFIXES {
        if let Some(stripped) = title.strip_suffix(suffix) {
            title = stripped.trim().to_owned();
        }
    }
    let generic = GENERIC_PAGE_TITLES.contains(&title.to_lowercase().as_str());
    let looks_like_address = SCHEMES.iter().any(|scheme| title.starts_with(scheme))
        || title.starts_with("claude.ai/");
    if title.is_empty() || generic || looks_like_address {
        return None;
    }
    Some(title.chars().take(MAX_ARTIFACT_TITLE_CHARS).collect())
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_artifact_links() {
        assert_eq!(
            artifact_link("  https://claude.ai/artifact/GwYHvofqunFu6ZSHy3aJPY?tab=1 ").as_deref(),
            Some("https://claude.ai/artifact/GwYHvofqunFu6ZSHy3aJPY")
        );
        assert_eq!(
            artifact_link("claude.ai/artifact/6hbmkXmWaD1JYEXSbQVV8z/").as_deref(),
            Some("https://claude.ai/artifact/6hbmkXmWaD1JYEXSbQVV8z")
        );
        assert_eq!(
            artifact_link("https://Claude.ai/public/artifacts/8116871c-07e7-48d6-a271-ad7762549547#x").as_deref(),
            Some("https://claude.ai/public/artifacts/8116871c-07e7-48d6-a271-ad7762549547")
        );
    }

    #[test]
    fn rejects_foreign_or_broken_links() {
        for raw in [
            "",
            "https://claude.ai",
            "https://claude.ai/artifact/",
            "https://claude.ai/chat/123",
            "https://evil.example/artifact/abc",
            "https://claude.ai.evil.example/artifact/abc",
            "https://claude.ai/artifact/a b",
            "https://claude.ai/artifact/abc/../../settings",
            "javascript:alert(1)",
        ] {
            assert_eq!(artifact_link(raw), None, "{raw}");
        }
    }

    #[test]
    fn validates_drafts() {
        let draft = |title: &str, link: &str| ArtifactDraft {
            title: title.into(),
            link: link.into(),
        };
        let ok = draft("  Hopla   портал ", "claude.ai/artifact/UxuweBPZSnhor1MAJw3vte")
            .normalized()
            .unwrap();
        assert_eq!(ok.title, "Hopla портал");
        assert_eq!(ok.link, "https://claude.ai/artifact/UxuweBPZSnhor1MAJw3vte");
        assert_eq!(draft("x", "https://figma.com").normalized(), Err(ArtifactError::InvalidLink));
        assert_eq!(
            draft(&"я".repeat(MAX_ARTIFACT_TITLE_CHARS + 1), "claude.ai/artifact/abc").normalized(),
            Err(ArtifactError::TitleTooLong)
        );
    }

    #[test]
    fn reads_titles_from_pages() {
        assert_eq!(
            title_from_page("ВІДПУСТИ — Арт-дизайн - Claude").as_deref(),
            Some("ВІДПУСТИ — Арт-дизайн")
        );
        assert_eq!(title_from_page("Claude"), None);
        assert_eq!(title_from_page("  "), None);
        assert_eq!(title_from_page("https://claude.ai/artifact/abc"), None);
    }

    #[test]
    fn errors_keep_their_wire_format() {
        let json = serde_json::to_string(&ArtifactError::InvalidLink).unwrap();
        assert_eq!(json, r#"{"kind":"invalid_link"}"#);
    }

    #[test]
    fn origins_round_trip_through_codes() {
        for origin in ArtifactOrigin::ALL {
            assert_eq!(ArtifactOrigin::from_code(origin.code()), Some(origin));
        }
        assert_eq!(ArtifactOrigin::from_code("other"), None);
    }
}
