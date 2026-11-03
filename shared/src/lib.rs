mod artifacts;
mod builds;
mod comfy;
mod journal;
mod pinterest;
mod skills;
mod studio;
mod toolbox;

use std::fmt;

use serde::{Deserialize, Serialize};

pub use artifacts::*;
pub use builds::*;
pub use comfy::*;
pub use journal::*;
pub use pinterest::*;
pub use skills::*;
pub use studio::*;
pub use toolbox::*;

pub const MIN_PASSWORD_CHARS: usize = 8;
pub const MAX_PASSWORD_CHARS: usize = 256;
pub const MAX_NAME_CHARS: usize = 48;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registration {
    pub name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum AuthError {
    InvalidCredentials,
    EmailTaken,
    InvalidEmail,
    PasswordTooShort,
    PasswordTooLong,
    NameMissing,
    NameTooLong,
    Storage(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCredentials => f.write_str("invalid email or password"),
            Self::EmailTaken => f.write_str("an account with this email already exists"),
            Self::InvalidEmail => f.write_str("email address is malformed"),
            Self::PasswordTooShort => {
                write!(f, "password is shorter than {MIN_PASSWORD_CHARS} characters")
            }
            Self::PasswordTooLong => {
                write!(f, "password is longer than {MAX_PASSWORD_CHARS} characters")
            }
            Self::NameMissing => f.write_str("display name is empty"),
            Self::NameTooLong => write!(f, "display name is longer than {MAX_NAME_CHARS} characters"),
            Self::Storage(detail) => write!(f, "account storage failed: {detail}"),
        }
    }
}

impl std::error::Error for AuthError {}

pub fn normalize_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

pub fn validate_email(email: &str) -> Result<(), AuthError> {
    let (local, domain) = email.split_once('@').ok_or(AuthError::InvalidEmail)?;
    let domain_is_valid =
        domain.split('.').count() >= 2 && domain.split('.').all(|label| !label.is_empty());
    let has_forbidden = email.chars().any(char::is_whitespace) || domain.contains('@');
    if local.is_empty() || !domain_is_valid || has_forbidden {
        return Err(AuthError::InvalidEmail);
    }
    Ok(())
}

pub fn validate_password(password: &str) -> Result<(), AuthError> {
    match password.chars().count() {
        count if count < MIN_PASSWORD_CHARS => Err(AuthError::PasswordTooShort),
        count if count > MAX_PASSWORD_CHARS => Err(AuthError::PasswordTooLong),
        _ => Ok(()),
    }
}

pub fn validate_name(name: &str) -> Result<(), AuthError> {
    match name.trim().chars().count() {
        0 => Err(AuthError::NameMissing),
        count if count > MAX_NAME_CHARS => Err(AuthError::NameTooLong),
        _ => Ok(()),
    }
}

impl Registration {
    pub fn validate(&self) -> Result<(), AuthError> {
        validate_name(&self.name)?;
        validate_email(&normalize_email(&self.email))?;
        validate_password(&self.password)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PasswordStrength {
    Weak,
    Fair,
    Good,
    Strong,
}

impl PasswordStrength {
    pub fn assess(password: &str) -> Self {
        let length = password.chars().count();
        if length < MIN_PASSWORD_CHARS {
            return Self::Weak;
        }
        let contains = |test: fn(char) -> bool| password.chars().any(test);
        let alphabet: u32 = [
            (contains(|c| c.is_ascii_lowercase()), 26),
            (contains(|c| c.is_ascii_uppercase()), 26),
            (contains(|c| c.is_ascii_digit()), 10),
            (contains(|c| !c.is_alphanumeric()), 33),
            (contains(|c| c.is_alphabetic() && !c.is_ascii()), 66),
        ]
        .into_iter()
        .filter_map(|(present, size)| present.then_some(size))
        .sum();
        let entropy_bits = length as f64 * f64::from(alphabet).log2();
        match entropy_bits {
            bits if bits < 40.0 => Self::Weak,
            bits if bits < 60.0 => Self::Fair,
            bits if bits < 80.0 => Self::Good,
            _ => Self::Strong,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_email_case_and_whitespace() {
        assert_eq!(normalize_email("  Nichna@Studio.UA "), "nichna@studio.ua");
    }

    #[test]
    fn accepts_plain_addresses() {
        for email in ["a@b.co", "night.owl+games@studio.com.ua"] {
            assert_eq!(validate_email(email), Ok(()), "{email}");
        }
    }

    #[test]
    fn rejects_malformed_addresses() {
        for email in [
            "",
            "plain",
            "@studio.ua",
            "owl@",
            "owl@studio",
            "owl@studio.",
            "owl@.ua",
            "o wl@studio.ua",
            "owl@@studio.ua",
        ] {
            assert_eq!(validate_email(email), Err(AuthError::InvalidEmail), "{email}");
        }
    }

    #[test]
    fn measures_password_in_characters_not_bytes() {
        assert_eq!(validate_password("ніч1234"), Err(AuthError::PasswordTooShort));
        assert_eq!(validate_password("нічнасова"), Ok(()));
        assert_eq!(
            validate_password(&"x".repeat(MAX_PASSWORD_CHARS + 1)),
            Err(AuthError::PasswordTooLong)
        );
    }

    #[test]
    fn requires_a_display_name() {
        assert_eq!(validate_name("   "), Err(AuthError::NameMissing));
        assert_eq!(
            validate_name(&"я".repeat(MAX_NAME_CHARS + 1)),
            Err(AuthError::NameTooLong)
        );
        assert_eq!(validate_name(" Остап "), Ok(()));
    }

    #[test]
    fn grades_passwords_by_length_and_alphabet() {
        assert_eq!(PasswordStrength::assess("Ab1!"), PasswordStrength::Weak);
        assert_eq!(PasswordStrength::assess("password"), PasswordStrength::Weak);
        assert_eq!(PasswordStrength::assess("qwertyuiop"), PasswordStrength::Fair);
        assert_eq!(PasswordStrength::assess("Password12!"), PasswordStrength::Good);
        assert_eq!(PasswordStrength::assess("Password12!xyz"), PasswordStrength::Strong);
        assert_eq!(
            PasswordStrength::assess("місяць-над-містом"),
            PasswordStrength::Strong
        );
    }

    #[test]
    fn errors_keep_their_wire_format() {
        let json = serde_json::to_string(&AuthError::InvalidCredentials).unwrap();
        assert_eq!(json, r#"{"kind":"invalid_credentials"}"#);
        let storage = AuthError::Storage("disk full".into());
        let round_trip: AuthError =
            serde_json::from_str(&serde_json::to_string(&storage).unwrap()).unwrap();
        assert_eq!(round_trip, storage);
    }
}
