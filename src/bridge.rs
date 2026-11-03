use noctics_shared::{
    AuthError, BuildTarget, ClaudeUsage, Credentials, GameEngine, JournalEntry, PinterestError,
    PinterestStatus, Project, ProjectDetail, Registration, Screen, Session, Skill, SkillOutput,
    SkillDraft, SkillError, StudioError, StudioSettings, ToolDraft, ToolError, ToolSlot, Toolbox,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    async fn tauri_invoke(command: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen, catch)]
    async fn tauri_listen(event: &str, handler: &js_sys::Function) -> Result<JsValue, JsValue>;
}

#[derive(Serialize)]
struct NoArgs {}

#[derive(Serialize)]
struct SignInArgs<'a> {
    credentials: &'a Credentials,
}

#[derive(Serialize)]
struct SignUpArgs<'a> {
    registration: &'a Registration,
}

#[derive(Serialize)]
struct DraftArgs<'a> {
    draft: &'a SkillDraft,
}

#[derive(Serialize)]
struct UpdateArgs<'a> {
    id: i64,
    draft: &'a SkillDraft,
}

#[derive(Serialize)]
struct ToggleArgs {
    id: i64,
    enabled: bool,
}

#[derive(Serialize)]
struct IdArgs {
    id: i64,
}

#[derive(Serialize)]
struct PinArgs<'a> {
    id: &'a str,
}

#[derive(Serialize)]
struct ReferenceArgs {
    id: i64,
    index: usize,
}

#[derive(Serialize)]
struct StartArgs<'a> {
    prompt: &'a str,
    engine: GameEngine,
    targets: &'a [BuildTarget],
}

#[derive(Serialize)]
struct EngineArgs {
    id: i64,
    engine: GameEngine,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NewScreenArgs<'a> {
    project_id: i64,
    title: &'a str,
    summary: &'a str,
}

#[derive(Serialize)]
struct FeedbackArgs<'a> {
    id: i64,
    feedback: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FormArgs<'a> {
    id: i64,
    name: &'a str,
    cover_screen_id: i64,
}

#[derive(Serialize)]
struct RenameArgs<'a> {
    id: i64,
    name: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChooseToolArgs<'a> {
    slot: ToolSlot,
    tool_id: &'a str,
}

#[derive(Serialize)]
struct ToolDraftArgs<'a> {
    draft: &'a ToolDraft,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolIdArgs<'a> {
    tool_id: &'a str,
}

fn tool_error(error: JsValue) -> ToolError {
    command_error(error, ToolError::Storage)
}

pub async fn toolbox() -> Result<Toolbox, ToolError> {
    invoke("toolbox", &NoArgs {}).await.map_err(tool_error)
}

pub async fn choose_tool(slot: ToolSlot, tool_id: &str) -> Result<Toolbox, ToolError> {
    invoke("choose_tool", &ChooseToolArgs { slot, tool_id })
        .await
        .map_err(tool_error)
}

pub async fn add_tool(draft: &ToolDraft) -> Result<Toolbox, ToolError> {
    invoke("add_tool", &ToolDraftArgs { draft })
        .await
        .map_err(tool_error)
}

pub async fn remove_tool(tool_id: &str) -> Result<Toolbox, ToolError> {
    invoke("remove_tool", &ToolIdArgs { tool_id })
        .await
        .map_err(tool_error)
}

pub async fn open_tool(tool_id: &str) -> Result<(), ToolError> {
    invoke("open_tool", &ToolIdArgs { tool_id })
        .await
        .map_err(tool_error)
}

pub struct Subscription {
    unlisten: Option<js_sys::Function>,
    _handler: Closure<dyn FnMut(JsValue)>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(unlisten) = &self.unlisten {
            let _ = unlisten.call0(&JsValue::NULL);
        }
    }
}

pub async fn listen(event: &str, mut on_event: impl FnMut() + 'static) -> Option<Subscription> {
    if !desktop_shell_present() {
        return None;
    }
    let handler = Closure::<dyn FnMut(JsValue)>::new(move |_| on_event());
    let unlisten = tauri_listen(event, handler.as_ref().unchecked_ref())
        .await
        .ok()?
        .dyn_into::<js_sys::Function>()
        .ok();
    Some(Subscription {
        unlisten,
        _handler: handler,
    })
}

fn desktop_shell_present() -> bool {
    web_sys::window()
        .and_then(|window| js_sys::Reflect::get(&window, &JsValue::from_str("__TAURI__")).ok())
        .is_some_and(|tauri| tauri.is_object())
}

pub(crate) async fn invoke<A: Serialize, T: DeserializeOwned>(command: &str, args: &A) -> Result<T, JsValue> {
    if !desktop_shell_present() {
        return Err(JsValue::from_str("Tauri IPC is not available in this window"));
    }
    let payload = serde_wasm_bindgen::to_value(args)?;
    let response = tauri_invoke(command, payload).await?;
    Ok(serde_wasm_bindgen::from_value(response)?)
}

pub(crate) fn command_error<E: DeserializeOwned>(error: JsValue, storage: fn(String) -> E) -> E {
    serde_wasm_bindgen::from_value(error.clone())
        .unwrap_or_else(|_| storage(error.as_string().unwrap_or_else(|| format!("{error:?}"))))
}

fn auth_error(error: JsValue) -> AuthError {
    command_error(error, AuthError::Storage)
}

fn skill_error(error: JsValue) -> SkillError {
    command_error(error, SkillError::Storage)
}

fn studio_error(error: JsValue) -> StudioError {
    command_error(error, StudioError::Process)
}

pub async fn studio_settings() -> Result<StudioSettings, StudioError> {
    invoke("studio_settings", &NoArgs {}).await.map_err(studio_error)
}

pub async fn pick_references(project_id: i64) -> Result<Project, StudioError> {
    invoke("pick_references", &IdArgs { id: project_id })
        .await
        .map_err(studio_error)
}

pub async fn open_reference(project_id: i64, index: usize) -> Result<(), StudioError> {
    invoke(
        "open_reference",
        &ReferenceArgs {
            id: project_id,
            index,
        },
    )
    .await
    .map_err(studio_error)
}

fn pinterest_error(error: JsValue) -> PinterestError {
    command_error(error, PinterestError::Browser)
}

pub async fn pinterest_status() -> Result<PinterestStatus, PinterestError> {
    invoke("pinterest_status", &NoArgs {})
        .await
        .map_err(pinterest_error)
}

pub async fn pinterest_login() -> Result<(), PinterestError> {
    invoke("pinterest_login", &NoArgs {})
        .await
        .map_err(pinterest_error)
}

pub async fn load_reference_pins(project_id: i64) -> Result<(), PinterestError> {
    invoke("load_reference_pins", &IdArgs { id: project_id })
        .await
        .map_err(pinterest_error)
}

pub async fn open_pin(pin_id: &str) -> Result<(), PinterestError> {
    invoke("open_pin", &PinArgs { id: pin_id })
        .await
        .map_err(pinterest_error)
}

pub async fn claude_usage() -> Result<Option<ClaudeUsage>, StudioError> {
    invoke("claude_usage", &NoArgs {}).await.map_err(studio_error)
}

pub async fn claude_login() -> Result<(), StudioError> {
    invoke("claude_login", &NoArgs {}).await.map_err(studio_error)
}

pub async fn list_projects() -> Result<Vec<Project>, StudioError> {
    invoke("list_projects", &NoArgs {}).await.map_err(studio_error)
}

pub async fn project_detail(id: i64) -> Result<ProjectDetail, StudioError> {
    invoke("project_detail", &IdArgs { id })
        .await
        .map_err(studio_error)
}

pub async fn project_journal(id: i64) -> Result<Vec<JournalEntry>, StudioError> {
    invoke("project_journal", &IdArgs { id })
        .await
        .map_err(studio_error)
}

pub async fn start_project(
    prompt: &str,
    engine: GameEngine,
    targets: &[BuildTarget],
) -> Result<Project, StudioError> {
    invoke("start_project", &StartArgs { prompt, engine, targets })
        .await
        .map_err(studio_error)
}

pub async fn set_project_engine(id: i64, engine: GameEngine) -> Result<Project, StudioError> {
    invoke("set_project_engine", &EngineArgs { id, engine })
        .await
        .map_err(studio_error)
}

pub async fn add_screen(project_id: i64, title: &str, summary: &str) -> Result<Screen, StudioError> {
    invoke(
        "add_screen",
        &NewScreenArgs {
            project_id,
            title,
            summary,
        },
    )
    .await
    .map_err(studio_error)
}

pub async fn refine_screen(id: i64, feedback: &str) -> Result<Screen, StudioError> {
    invoke("refine_screen", &FeedbackArgs { id, feedback })
        .await
        .map_err(studio_error)
}

pub async fn regenerate_screen(id: i64) -> Result<Screen, StudioError> {
    invoke("regenerate_screen", &IdArgs { id })
        .await
        .map_err(studio_error)
}

pub async fn approve_screen(id: i64) -> Result<Screen, StudioError> {
    invoke("approve_screen", &IdArgs { id })
        .await
        .map_err(studio_error)
}

pub async fn form_project(id: i64, name: &str, cover_screen_id: i64) -> Result<Project, StudioError> {
    invoke(
        "form_project",
        &FormArgs {
            id,
            name,
            cover_screen_id,
        },
    )
    .await
    .map_err(studio_error)
}

pub async fn rename_project(id: i64, name: &str) -> Result<Project, StudioError> {
    invoke("rename_project", &RenameArgs { id, name })
        .await
        .map_err(studio_error)
}

pub async fn delete_project(id: i64) -> Result<(), StudioError> {
    invoke("delete_project", &IdArgs { id })
        .await
        .map_err(studio_error)
}

pub async fn regenerate_output(id: i64) -> Result<SkillOutput, StudioError> {
    invoke("regenerate_output", &IdArgs { id })
        .await
        .map_err(studio_error)
}

pub async fn sync_project(id: i64) -> Result<ProjectDetail, StudioError> {
    invoke("sync_project", &IdArgs { id })
        .await
        .map_err(studio_error)
}

pub async fn has_accounts() -> Result<bool, AuthError> {
    invoke("has_accounts", &NoArgs {}).await.map_err(auth_error)
}

pub async fn current_session() -> Result<Option<Session>, AuthError> {
    invoke("current_session", &NoArgs {}).await.map_err(auth_error)
}

pub async fn sign_in(credentials: &Credentials) -> Result<Session, AuthError> {
    invoke("login", &SignInArgs { credentials })
        .await
        .map_err(auth_error)
}

pub async fn sign_up(registration: &Registration) -> Result<Session, AuthError> {
    invoke("register", &SignUpArgs { registration })
        .await
        .map_err(auth_error)
}

pub async fn sign_out() -> Result<(), AuthError> {
    invoke("logout", &NoArgs {}).await.map_err(auth_error)
}

pub async fn list_skills() -> Result<Vec<Skill>, SkillError> {
    invoke("list_skills", &NoArgs {}).await.map_err(skill_error)
}

pub async fn create_skill(draft: &SkillDraft) -> Result<Skill, SkillError> {
    invoke("create_skill", &DraftArgs { draft })
        .await
        .map_err(skill_error)
}

pub async fn update_skill(id: i64, draft: &SkillDraft) -> Result<Skill, SkillError> {
    invoke("update_skill", &UpdateArgs { id, draft })
        .await
        .map_err(skill_error)
}

pub async fn set_skill_enabled(id: i64, enabled: bool) -> Result<Skill, SkillError> {
    invoke("set_skill_enabled", &ToggleArgs { id, enabled })
        .await
        .map_err(skill_error)
}

pub async fn delete_skill(id: i64) -> Result<(), SkillError> {
    invoke("delete_skill", &IdArgs { id })
        .await
        .map_err(skill_error)
}

pub async fn skills_folder() -> Result<Option<String>, SkillError> {
    invoke("skills_folder", &NoArgs {})
        .await
        .map_err(skill_error)
}

pub async fn minimize_window() {
    let _ = invoke::<_, ()>("window_minimize", &NoArgs {}).await;
}

pub async fn toggle_maximize_window() -> Option<bool> {
    invoke("window_toggle_maximize", &NoArgs {}).await.ok()
}

pub async fn window_is_maximized() -> Option<bool> {
    invoke("window_is_maximized", &NoArgs {}).await.ok()
}

pub async fn close_window() {
    let _ = invoke::<_, ()>("window_close", &NoArgs {}).await;
}
