use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use noctics_shared::{CLAUDE_MODEL, CLAUDE_TIMEOUT, ClaudeAccount, StudioError};
use serde_json::Value;

const CALL_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(200);
const MAX_OUTPUT_TOKENS: &str = "64000";
const SUBSCRIPTION_AUTH: &str = "claude.ai";
const NO_TOOLS: &str = "";
const WEB_TOOLS: &str = "WebSearch,WebFetch";
const UNATTENDED_PERMISSIONS: &str = "dontAsk";
pub(crate) const SHADOWING_CREDENTIALS: [&str; 2] = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"];
const RATE_LIMITED_STATUS: u64 = 429;
const OVERLOADED_STATUSES: [u64; 2] = [503, 529];
const UNAUTHORIZED_STATUSES: [u64; 2] = [401, 403];
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const EXECUTABLE_NAME: &str = "claude.exe";
#[cfg(not(windows))]
const EXECUTABLE_NAME: &str = "claude";

static PROMPT_FILES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub enum Effort {
    Medium,
    High,
}

impl Effort {
    fn code(self) -> &'static str {
        match self {
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

pub struct Completion {
    pub system: String,
    pub user: String,
    pub effort: Effort,
    pub schema: Option<Value>,
}

struct Output {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

#[derive(Clone)]
pub struct ClaudeCode {
    workdir: PathBuf,
}

impl ClaudeCode {
    pub fn new(workdir: PathBuf) -> Self {
        Self { workdir }
    }

    pub fn account(&self) -> ClaudeAccount {
        let Some(executable) = locate() else {
            return ClaudeAccount::default();
        };
        let version = self
            .run(&executable, &["--version"], None, PROBE_TIMEOUT)
            .ok()
            .and_then(|output| output.stdout.split_whitespace().next().map(str::to_owned));
        let status = self
            .run(&executable, &["auth", "status"], None, PROBE_TIMEOUT)
            .ok()
            .map(|output| parse_status(&output.stdout))
            .unwrap_or_default();
        ClaudeAccount {
            installed: true,
            version,
            ..status
        }
    }

    pub fn complete(&self, completion: &Completion) -> Result<String, StudioError> {
        self.answer(completion, NO_TOOLS)
    }

    pub fn research(&self, completion: &Completion) -> Result<String, StudioError> {
        self.answer(completion, WEB_TOOLS)
    }

    fn answer(&self, completion: &Completion, tools: &str) -> Result<String, StudioError> {
        let executable = locate().ok_or(StudioError::ClaudeCodeMissing)?;
        std::fs::create_dir_all(&self.workdir).map_err(process_failure)?;
        let prompt_file = self.workdir.join(prompt_file_name());
        std::fs::write(&prompt_file, &completion.system).map_err(process_failure)?;
        let prompt_path = prompt_file.to_string_lossy().into_owned();
        let schema = completion.schema.as_ref().map(Value::to_string);
        let mut args = vec![
            "-p",
            "--output-format",
            "json",
            "--tools",
            tools,
            "--safe-mode",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--model",
            CLAUDE_MODEL,
            "--effort",
            completion.effort.code(),
            "--system-prompt-file",
            &prompt_path,
        ];
        if !tools.is_empty() {
            args.extend(["--allowedTools", tools, "--permission-mode", UNATTENDED_PERMISSIONS]);
        }
        if let Some(schema) = &schema {
            args.extend(["--json-schema", schema.as_str()]);
        }
        let outcome = self.run(&executable, &args, Some(&completion.user), CALL_TIMEOUT);
        let _ = std::fs::remove_file(&prompt_file);
        let output = outcome?;
        interpret(
            &output.stdout,
            &output.stderr,
            output.status.success(),
            schema.is_some(),
        )
    }

    pub fn open_login(&self) -> Result<(), StudioError> {
        let executable = locate().ok_or(StudioError::ClaudeCodeMissing)?;
        login_command(&executable)
            .current_dir(&self.workdir)
            .spawn()
            .map(drop)
            .map_err(process_failure)
    }

    fn run(
        &self,
        executable: &Path,
        args: &[&str],
        input: Option<&str>,
        timeout: Duration,
    ) -> Result<Output, StudioError> {
        std::fs::create_dir_all(&self.workdir).map_err(process_failure)?;
        let mut command = Command::new(executable);
        command
            .args(args)
            .current_dir(&self.workdir)
            .env("CLAUDE_CODE_MAX_OUTPUT_TOKENS", MAX_OUTPUT_TOKENS)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for variable in SHADOWING_CREDENTIALS {
            command.env_remove(variable);
        }
        hide_console(&mut command);
        let mut child = command.spawn().map_err(process_failure)?;
        if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
            stdin.write_all(text.as_bytes()).map_err(process_failure)?;
        }
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let status = wait_with_deadline(&mut child, timeout)?;
        Ok(Output {
            status,
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        })
    }
}

pub(crate) fn locate() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default();
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    let user_bin = home.map(|home| PathBuf::from(home).join(".local").join("bin"));
    on_path
        .into_iter()
        .chain(user_bin)
        .map(|dir| dir.join(EXECUTABLE_NAME))
        .find(|candidate| candidate.is_file())
}

fn prompt_file_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let sequence = PROMPT_FILES.fetch_add(1, Ordering::Relaxed);
    format!("system-{}-{nanos}-{sequence}.txt", std::process::id())
}

fn drain<R: Read + Send + 'static>(stream: Option<R>) -> JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stream {
            let _ = stream.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn wait_with_deadline(child: &mut Child, timeout: Duration) -> Result<ExitStatus, StudioError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(process_failure)? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(StudioError::Process(CLAUDE_TIMEOUT.into()));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

#[cfg(windows)]
fn login_command(executable: &Path) -> Command {
    let mut command = Command::new("cmd");
    command
        .args(["/C", "start", "Claude login"])
        .arg(executable)
        .args(["auth", "login"]);
    command
}

#[cfg(not(windows))]
fn login_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command.args(["auth", "login"]);
    command
}

fn parse_status(stdout: &str) -> ClaudeAccount {
    let Ok(status) = serde_json::from_str::<Value>(stdout.trim()) else {
        return ClaudeAccount::default();
    };
    let signed_in = status["loggedIn"].as_bool().unwrap_or(false);
    let subscription = status["authMethod"].as_str() == Some(SUBSCRIPTION_AUTH);
    ClaudeAccount {
        installed: true,
        version: None,
        signed_in: signed_in && subscription,
        email: status["email"].as_str().map(str::to_owned),
        plan: status["subscriptionType"].as_str().map(str::to_owned),
    }
}

fn interpret(
    stdout: &str,
    stderr: &str,
    exited_cleanly: bool,
    structured: bool,
) -> Result<String, StudioError> {
    let Ok(report) = serde_json::from_str::<Value>(stdout.trim()) else {
        let detail = if stderr.trim().is_empty() {
            stdout
        } else {
            stderr
        };
        return Err(classify(detail.trim(), None).unwrap_or_else(|| {
            StudioError::Process(if exited_cleanly {
                "Claude Code returned an unreadable answer".to_owned()
            } else {
                detail.trim().to_owned()
            })
        }));
    };
    let message = report["result"].as_str().unwrap_or_default();
    if report["is_error"].as_bool().unwrap_or(false) {
        let status = report["api_error_status"].as_u64();
        return Err(
            classify(message, status).unwrap_or_else(|| StudioError::Claude(message.to_owned()))
        );
    }
    match report["stop_reason"].as_str() {
        Some("refusal") => return Err(StudioError::Refused),
        Some("max_tokens") => return Err(StudioError::Truncated),
        _ => {}
    }
    let answer = match &report["structured_output"] {
        Value::Null => message.to_owned(),
        value if structured => value.to_string(),
        _ => message.to_owned(),
    };
    if answer.trim().is_empty() {
        return Err(StudioError::Claude("the answer was empty".into()));
    }
    Ok(answer)
}

pub(crate) fn classify(message: &str, status: Option<u64>) -> Option<StudioError> {
    let lower = message.to_lowercase();
    if lower.contains("usage limit") || lower.contains("limit reached") {
        return Some(StudioError::UsageLimit);
    }
    if status.is_some_and(|code| UNAUTHORIZED_STATUSES.contains(&code))
        || lower.contains("not logged in")
        || lower.contains("/login")
        || lower.contains("invalid api key")
    {
        return Some(StudioError::ClaudeSignedOut);
    }
    if status == Some(RATE_LIMITED_STATUS) || lower.contains("rate limit") {
        return Some(StudioError::RateLimited);
    }
    if status.is_some_and(|code| OVERLOADED_STATUSES.contains(&code))
        || lower.contains("overloaded")
    {
        return Some(StudioError::Overloaded);
    }
    None
}

fn process_failure(error: std::io::Error) -> StudioError {
    StudioError::Process(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_subscription_status() {
        let account = parse_status(
            r#"{"loggedIn":true,"authMethod":"claude.ai","email":"owl@studio.ua","subscriptionType":"pro"}"#,
        );
        assert!(account.signed_in);
        assert_eq!(account.email.as_deref(), Some("owl@studio.ua"));
        assert_eq!(account.plan.as_deref(), Some("pro"));

        let api_key = parse_status(r#"{"loggedIn":true,"authMethod":"api_key"}"#);
        assert!(!api_key.signed_in);
        assert!(!parse_status("not json").signed_in);
    }

    #[test]
    fn returns_text_or_structured_output() {
        let text = r#"{"is_error":false,"subtype":"success","stop_reason":"end_turn","result":"<html></html>"}"#;
        assert_eq!(interpret(text, "", true, false).unwrap(), "<html></html>");

        let structured = r#"{"is_error":false,"stop_reason":"tool_use","result":"{\"answer\":\"OK\"}","structured_output":{"answer":"OK"}}"#;
        assert_eq!(
            interpret(structured, "", true, true).unwrap(),
            r#"{"answer":"OK"}"#
        );
    }

    #[test]
    fn maps_failures_to_studio_errors() {
        let limit = r#"{"is_error":true,"result":"Claude AI usage limit reached|1762041600","api_error_status":null}"#;
        assert_eq!(
            interpret(limit, "", false, false),
            Err(StudioError::UsageLimit)
        );

        let logged_out = r#"{"is_error":true,"result":"Not logged in · Please run /login","api_error_status":null}"#;
        assert_eq!(
            interpret(logged_out, "", false, false),
            Err(StudioError::ClaudeSignedOut)
        );

        let overloaded = r#"{"is_error":true,"result":"API Error: 529","api_error_status":529}"#;
        assert_eq!(
            interpret(overloaded, "", false, false),
            Err(StudioError::Overloaded)
        );

        let refusal = r#"{"is_error":false,"stop_reason":"refusal","result":""}"#;
        assert_eq!(
            interpret(refusal, "", true, false),
            Err(StudioError::Refused)
        );

        let other = r#"{"is_error":true,"result":"Something odd","api_error_status":400}"#;
        assert_eq!(
            interpret(other, "", false, false),
            Err(StudioError::Claude("Something odd".into()))
        );

        assert_eq!(
            interpret("", "boom", false, false),
            Err(StudioError::Process("boom".into()))
        );
    }

    #[test]
    fn prompt_files_never_collide() {
        assert_ne!(prompt_file_name(), prompt_file_name());
    }

    #[test]
    #[ignore = "calls the real Claude Code CLI and spends subscription usage"]
    fn live_round_trip_through_the_subscription() {
        let workdir = std::env::temp_dir().join("noctics-claude-live");
        let claude = ClaudeCode::new(workdir.clone());
        let account = claude.account();
        assert!(account.connected(), "{account:?}");

        let answer = claude
            .complete(&Completion {
                system: "Ти тестовий бот студії \"Noctics\". Відповідай одним словом.".into(),
                user: "Назви колір нічного неба.".into(),
                effort: Effort::Medium,
                schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": { "word": { "type": "string" } },
                    "required": ["word"],
                    "additionalProperties": false
                })),
            })
            .unwrap();
        let parsed: Value = serde_json::from_str(&answer).unwrap();
        assert!(!parsed["word"].as_str().unwrap_or_default().is_empty(), "{answer}");
        let leftovers = std::fs::read_dir(&workdir).unwrap().count();
        let _ = std::fs::remove_dir_all(&workdir);
        assert_eq!(leftovers, 0);
    }
}
