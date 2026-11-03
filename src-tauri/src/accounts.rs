use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use noctics_shared::{AuthError, Credentials, Registration, Session, normalize_email};
use rusqlite::{Connection, ErrorCode, OptionalExtension, params};

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS accounts (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        email TEXT NOT NULL UNIQUE COLLATE NOCASE,
        password_hash TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        last_sign_in_at INTEGER
    );
";

struct StoredAccount {
    id: i64,
    name: String,
    email: String,
    password_hash: String,
}

pub struct AccountStore {
    connection: Mutex<Connection>,
}

impl AccountStore {
    pub fn open(path: &Path) -> Result<Self, AuthError> {
        Self::prepare(Connection::open(path).map_err(storage)?)
    }

    fn prepare(connection: Connection) -> Result<Self, AuthError> {
        connection.execute_batch(SCHEMA).map_err(storage)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn has_accounts(&self) -> Result<bool, AuthError> {
        self.lock()?
            .query_row("SELECT EXISTS(SELECT 1 FROM accounts)", [], |row| {
                row.get(0)
            })
            .map_err(storage)
    }

    pub fn register(&self, registration: &Registration) -> Result<Session, AuthError> {
        registration.validate()?;
        let name = registration.name.trim().to_owned();
        let email = normalize_email(&registration.email);
        if self.find(&email)?.is_some() {
            return Err(AuthError::EmailTaken);
        }
        let password_hash = hash_secret(&registration.password)?;
        self.lock()?
            .execute(
                "INSERT INTO accounts (name, email, password_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![name, email, password_hash, unix_now()],
            )
            .map_err(|error| match error {
                rusqlite::Error::SqliteFailure(failure, _)
                    if failure.code == ErrorCode::ConstraintViolation =>
                {
                    AuthError::EmailTaken
                }
                other => storage(other),
            })?;
        Ok(Session { name, email })
    }

    pub fn sign_in(&self, credentials: &Credentials) -> Result<Session, AuthError> {
        let account = self.find(&normalize_email(&credentials.email))?;
        let stored_hash = match &account {
            Some(account) => account.password_hash.as_str(),
            None => decoy_hash(),
        };
        let password_matches = secret_matches(&credentials.password, stored_hash);
        let account = account
            .filter(|_| password_matches)
            .ok_or(AuthError::InvalidCredentials)?;
        self.lock()?
            .execute(
                "UPDATE accounts SET last_sign_in_at = ?1 WHERE id = ?2",
                params![unix_now(), account.id],
            )
            .map_err(storage)?;
        Ok(Session {
            name: account.name,
            email: account.email,
        })
    }

    fn find(&self, email: &str) -> Result<Option<StoredAccount>, AuthError> {
        self.lock()?
            .query_row(
                "SELECT id, name, email, password_hash FROM accounts WHERE email = ?1",
                params![email],
                |row| {
                    Ok(StoredAccount {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        email: row.get(2)?,
                        password_hash: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(storage)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, AuthError> {
        self.connection
            .lock()
            .map_err(|_| AuthError::Storage("account store lock is poisoned".into()))
    }
}

fn hash_secret(password: &str) -> Result<String, AuthError> {
    let hash: PasswordHash = Argon2::default()
        .hash_password(password.as_bytes())
        .map_err(|error| AuthError::Storage(format!("password hashing failed: {error}")))?;
    Ok(hash.to_string())
}

fn secret_matches(password: &str, stored_hash: &str) -> bool {
    Argon2::default()
        .verify_password(password.as_bytes(), stored_hash)
        .is_ok()
}

fn decoy_hash() -> &'static str {
    static DECOY: OnceLock<String> = OnceLock::new();
    DECOY.get_or_init(|| hash_secret("noctics-decoy-secret").unwrap_or_default())
}

pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

fn storage(error: rusqlite::Error) -> AuthError {
    AuthError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "місяць-над-містом";

    fn in_memory() -> AccountStore {
        AccountStore::prepare(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn registration(email: &str) -> Registration {
        Registration {
            name: "  Нічна Сова ".into(),
            email: email.into(),
            password: PASSWORD.into(),
        }
    }

    fn credentials(email: &str, password: &str) -> Credentials {
        Credentials {
            email: email.into(),
            password: password.into(),
        }
    }

    #[test]
    fn registers_then_signs_in_with_any_email_casing() {
        let store = in_memory();
        assert!(!store.has_accounts().unwrap());

        let created = store.register(&registration("Owl@Studio.UA")).unwrap();
        assert_eq!(
            created,
            Session {
                name: "Нічна Сова".into(),
                email: "owl@studio.ua".into(),
            }
        );
        assert!(store.has_accounts().unwrap());

        let signed_in = store
            .sign_in(&credentials("  OWL@studio.ua ", PASSWORD))
            .unwrap();
        assert_eq!(signed_in, created);
    }

    #[test]
    fn wrong_password_and_unknown_email_fail_the_same_way() {
        let store = in_memory();
        store.register(&registration("owl@studio.ua")).unwrap();

        assert_eq!(
            store.sign_in(&credentials("owl@studio.ua", "сонце-над-містом")),
            Err(AuthError::InvalidCredentials)
        );
        assert_eq!(
            store.sign_in(&credentials("fox@studio.ua", PASSWORD)),
            Err(AuthError::InvalidCredentials)
        );
    }

    #[test]
    fn refuses_a_second_account_for_the_same_email() {
        let store = in_memory();
        store.register(&registration("owl@studio.ua")).unwrap();

        assert_eq!(
            store.register(&registration("OWL@Studio.ua")),
            Err(AuthError::EmailTaken)
        );
    }

    #[test]
    fn keeps_only_an_argon2id_hash_of_the_password() {
        let store = in_memory();
        store.register(&registration("owl@studio.ua")).unwrap();

        let stored = store.find("owl@studio.ua").unwrap().unwrap();
        assert!(stored.password_hash.starts_with("$argon2id$"));
        assert!(!stored.password_hash.contains(PASSWORD));
    }

    #[test]
    fn rejects_invalid_registrations_before_writing() {
        let store = in_memory();
        let mut short = registration("owl@studio.ua");
        short.password = "коротко".into();

        assert_eq!(store.register(&short), Err(AuthError::PasswordTooShort));
        assert!(!store.has_accounts().unwrap());
    }

    #[test]
    fn accounts_survive_reopening_the_database() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("noctics-accounts-{nanos}.sqlite3"));

        AccountStore::open(&path)
            .unwrap()
            .register(&registration("owl@studio.ua"))
            .unwrap();
        let reopened = AccountStore::open(&path).unwrap();
        let signed_in = reopened.sign_in(&credentials("owl@studio.ua", PASSWORD));
        drop(reopened);
        std::fs::remove_file(&path).unwrap();

        assert_eq!(signed_in.unwrap().email, "owl@studio.ua");
    }
}
