use std::sync::{Arc, Mutex};

use noctics_shared::{
    AuthError, Credentials, Registration, Session, Skill, SkillDraft, SkillError,
};
use tauri::{State, Window};

use crate::accounts::AccountStore;
use crate::skills::SkillStore;

#[derive(Default)]
pub struct ActiveSession(Mutex<Option<Session>>);

impl ActiveSession {
    fn replace(&self, next: Option<Session>) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = next;
        }
    }

    fn current(&self) -> Option<Session> {
        self.0.lock().ok().and_then(|slot| slot.clone())
    }

    pub fn is_signed_in(&self) -> bool {
        self.current().is_some()
    }

    fn require(&self) -> Result<(), SkillError> {
        self.current().map(drop).ok_or(SkillError::SignedOut)
    }
}

type Accounts<'a> = State<'a, Arc<AccountStore>>;
type Skills<'a> = State<'a, Arc<SkillStore>>;

pub async fn off_main_thread<T, E, F>(task: F, storage: fn(String) -> E) -> Result<T, E>
where
    T: Send + 'static,
    E: Send + 'static,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| storage(error.to_string()))?
}

#[tauri::command]
pub async fn has_accounts(accounts: Accounts<'_>) -> Result<bool, AuthError> {
    let accounts = Arc::clone(&accounts);
    off_main_thread(move || accounts.has_accounts(), AuthError::Storage).await
}

#[tauri::command]
pub async fn register(
    registration: Registration,
    accounts: Accounts<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Session, AuthError> {
    let accounts = Arc::clone(&accounts);
    let created =
        off_main_thread(move || accounts.register(&registration), AuthError::Storage).await?;
    session.replace(Some(created.clone()));
    Ok(created)
}

#[tauri::command]
pub async fn login(
    credentials: Credentials,
    accounts: Accounts<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Session, AuthError> {
    let accounts = Arc::clone(&accounts);
    let signed_in =
        off_main_thread(move || accounts.sign_in(&credentials), AuthError::Storage).await?;
    session.replace(Some(signed_in.clone()));
    Ok(signed_in)
}

#[tauri::command]
pub fn logout(session: State<'_, ActiveSession>) {
    session.replace(None);
}

#[tauri::command]
pub fn current_session(session: State<'_, ActiveSession>) -> Option<Session> {
    session.current()
}

#[tauri::command]
pub async fn list_skills(
    skills: Skills<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Vec<Skill>, SkillError> {
    session.require()?;
    let skills = Arc::clone(&skills);
    off_main_thread(move || skills.list(), SkillError::Storage).await
}

#[tauri::command]
pub async fn create_skill(
    draft: SkillDraft,
    skills: Skills<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Skill, SkillError> {
    session.require()?;
    let skills = Arc::clone(&skills);
    off_main_thread(move || skills.create(&draft), SkillError::Storage).await
}

#[tauri::command]
pub async fn update_skill(
    id: i64,
    draft: SkillDraft,
    skills: Skills<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Skill, SkillError> {
    session.require()?;
    let skills = Arc::clone(&skills);
    off_main_thread(move || skills.update(id, &draft), SkillError::Storage).await
}

#[tauri::command]
pub async fn set_skill_enabled(
    id: i64,
    enabled: bool,
    skills: Skills<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Skill, SkillError> {
    session.require()?;
    let skills = Arc::clone(&skills);
    off_main_thread(move || skills.set_enabled(id, enabled), SkillError::Storage).await
}

#[tauri::command]
pub async fn delete_skill(
    id: i64,
    skills: Skills<'_>,
    session: State<'_, ActiveSession>,
) -> Result<(), SkillError> {
    session.require()?;
    let skills = Arc::clone(&skills);
    off_main_thread(move || skills.delete(id), SkillError::Storage).await
}

#[tauri::command]
pub fn skills_folder(
    skills: Skills<'_>,
    session: State<'_, ActiveSession>,
) -> Result<Option<String>, SkillError> {
    session.require()?;
    Ok(skills
        .export_dir()
        .map(|dir| dir.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn window_minimize(window: Window) -> Result<(), String> {
    window.minimize().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn window_toggle_maximize(window: Window) -> Result<bool, String> {
    let maximized = window.is_maximized().map_err(|error| error.to_string())?;
    let toggled = if maximized {
        window.unmaximize()
    } else {
        window.maximize()
    };
    toggled.map_err(|error| error.to_string())?;
    Ok(!maximized)
}

#[tauri::command]
pub fn window_is_maximized(window: Window) -> Result<bool, String> {
    window.is_maximized().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn window_close(window: Window) -> Result<(), String> {
    window.close().map_err(|error| error.to_string())
}
