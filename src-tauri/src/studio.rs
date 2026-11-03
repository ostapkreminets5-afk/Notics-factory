use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctics_shared::{
    ClaudeAccount, DESIGN_STAGE, GameEngine, INTERFACE_SKILL, Project, Skill, StudioError,
    runs_as_deliverable,
};
use tauri::{AppHandle, Emitter, Manager};

use crate::claude_code::{ClaudeCode, Completion, Effort};
use crate::pinterest::{Pinterest, Refresh};
use crate::prompts::{self, ReferenceBrief, ScreenBrief};
use crate::skills::SkillStore;
use crate::studio_store::{Job, OutputJob, ScreenJob, SkillRef, StudioStore};

pub const CHANGED_EVENT: &str = "studio-changed";
const MAX_WORKERS: usize = 2;
const ACCOUNT_CACHE_TTL: Duration = Duration::from_secs(60);

enum Exit {
    Drained,
    Halted,
}

pub struct Studio {
    pub store: Arc<StudioStore>,
    skills: Arc<SkillStore>,
    claude: ClaudeCode,
    app: AppHandle,
    workers: AtomicUsize,
    account: Mutex<Option<(Instant, ClaudeAccount)>>,
}

impl Studio {
    pub fn new(
        store: Arc<StudioStore>,
        skills: Arc<SkillStore>,
        app: AppHandle,
        workdir: PathBuf,
    ) -> Self {
        Self {
            store,
            skills,
            claude: ClaudeCode::new(workdir),
            app,
            workers: AtomicUsize::new(0),
            account: Mutex::new(None),
        }
    }

    pub fn claude(&self) -> &ClaudeCode {
        &self.claude
    }

    pub fn account(&self, fresh: bool) -> ClaudeAccount {
        if !fresh {
            let cached = self.account.lock().ok().and_then(|slot| {
                slot.as_ref()
                    .filter(|(checked, _)| checked.elapsed() < ACCOUNT_CACHE_TTL)
                    .map(|(_, account)| account.clone())
            });
            if let Some(account) = cached {
                return account;
            }
        }
        let account = self.claude.account();
        if let Ok(mut slot) = self.account.lock() {
            *slot = Some((Instant::now(), account.clone()));
        }
        account
    }

    pub fn require_connection(&self) -> Result<(), StudioError> {
        let account = self.account(false);
        match (account.installed, account.signed_in) {
            (false, _) => Err(StudioError::ClaudeCodeMissing),
            (true, false) => Err(StudioError::ClaudeSignedOut),
            (true, true) => Ok(()),
        }
    }

    pub fn notify(&self) {
        let _ = self.app.emit(CHANGED_EVENT, ());
    }

    pub fn kick(self: &Arc<Self>) {
        loop {
            if !self.store.has_pending().unwrap_or(false) {
                return;
            }
            let running = self.workers.load(Ordering::SeqCst);
            if running >= MAX_WORKERS {
                return;
            }
            if self
                .workers
                .compare_exchange(running, running + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                let worker = Arc::clone(self);
                tauri::async_runtime::spawn(async move { worker.work().await });
            }
        }
    }

    async fn work(self: Arc<Self>) {
        let exit = self.drain().await;
        self.workers.fetch_sub(1, Ordering::SeqCst);
        if matches!(exit, Exit::Drained) {
            self.kick();
        }
    }

    async fn drain(self: &Arc<Self>) -> Exit {
        let gate = Arc::clone(self);
        let connected = tauri::async_runtime::spawn_blocking(move || gate.require_connection())
            .await
            .is_ok_and(|outcome| outcome.is_ok());
        if !connected {
            return Exit::Halted;
        }
        loop {
            let job = match self.store.claim_job() {
                Ok(Some(job)) => job,
                Ok(None) => return Exit::Drained,
                Err(_) => return Exit::Halted,
            };
            self.notify();
            match job {
                Job::Plan {
                    project_id,
                    prompt,
                    engine,
                } => {
                    let outcome = self.plan(&prompt, engine).await;
                    let _ = match outcome {
                        Ok(plan) => self
                            .deliverable_skills()
                            .and_then(|skills| self.store.apply_plan(project_id, &plan, &skills))
                            .inspect(|_| self.collect_pins(project_id)),
                        Err(problem) => self.store.fail_project(project_id, &problem),
                    };
                    self.notify();
                    self.kick();
                }
                Job::Output(output) => {
                    let outcome = self.deliver(&output).await;
                    let _ = match outcome {
                        Ok(content) => self.store.finish_output(output.output_id, &content),
                        Err(problem) => self.store.fail_output(output.output_id, &problem),
                    };
                    self.notify();
                    self.kick();
                }
                Job::Screen(screen) => {
                    let outcome = self.design(&screen).await;
                    let _ = match outcome {
                        Ok(html) => self.store.finish_screen(screen.screen_id, &html),
                        Err(problem) => self.store.fail_screen(screen.screen_id, &problem),
                    };
                    self.notify();
                }
            }
        }
    }

    async fn plan(&self, prompt: &str, engine: GameEngine) -> Result<prompts::Plan, StudioError> {
        let answer = self
            .ask(Completion {
                system: prompts::system_prompt(&[]),
                user: prompts::plan_message(prompt, engine),
                effort: Effort::High,
                schema: Some(prompts::plan_schema()),
            })
            .await?;
        prompts::parse_plan(&answer)
    }

    pub async fn pick_references(&self, project_id: i64) -> Result<Project, StudioError> {
        let context = self.store.reference_context(project_id)?;
        let brief = ReferenceBrief {
            prompt: &context.prompt,
            game_name: &context.game_name,
            pitch: &context.pitch,
            design_document: &context.design_document,
            style: &context.style,
        };
        let answer = self
            .ask(Completion {
                system: prompts::system_prompt(&[]),
                user: prompts::references_message(&brief),
                effort: Effort::Medium,
                schema: Some(prompts::references_schema()),
            })
            .await?;
        let board = prompts::parse_references(&answer)?;
        let updated = self
            .store
            .set_reference_board(project_id, &board.analysis, &board.references)?;
        self.collect_pins(project_id);
        Ok(updated)
    }

    fn collect_pins(&self, project_id: i64) {
        if let Some(pinterest) = self.app.try_state::<Arc<Pinterest>>() {
            let _ = pinterest.load_board(Arc::clone(&self.store), project_id, Refresh::Everything);
        }
    }

    async fn design(&self, job: &ScreenJob) -> Result<String, StudioError> {
        let brief = ScreenBrief {
            game_name: &job.game_name,
            pitch: &job.pitch,
            design_document: &job.design_document,
            style: &job.style,
            sibling_titles: &job.sibling_titles,
            title: &job.title,
            summary: &job.summary,
            reference: job
                .reference
                .as_ref()
                .map(|(title, html)| (title.as_str(), html.as_str())),
            revision: job
                .revision
                .as_ref()
                .map(|(html, feedback)| (html.as_str(), feedback.as_str())),
        };
        let effort = if job.revision.is_some() {
            Effort::Medium
        } else {
            Effort::High
        };
        let interface: Vec<Skill> = self
            .enabled_skills()?
            .into_iter()
            .filter(|skill| skill.name == INTERFACE_SKILL)
            .collect();
        let answer = self
            .ask(Completion {
                system: prompts::system_prompt(&interface),
                user: prompts::screen_message(&brief),
                effort,
                schema: None,
            })
            .await?;
        prompts::extract_html(&answer)
            .ok_or_else(|| StudioError::Claude("the answer had no HTML document".into()))
    }

    async fn ask(&self, completion: Completion) -> Result<String, StudioError> {
        let claude = self.claude.clone();
        tauri::async_runtime::spawn_blocking(move || claude.complete(&completion))
            .await
            .map_err(|error| StudioError::Process(error.to_string()))?
    }

    async fn deliver(&self, job: &OutputJob) -> Result<String, StudioError> {
        let skill = self
            .enabled_skills()?
            .into_iter()
            .find(|skill| skill.name == job.skill_name)
            .ok_or(StudioError::SkillUnavailable)?;
        let effort = if skill.stage <= DESIGN_STAGE {
            Effort::High
        } else {
            Effort::Medium
        };
        let answer = self
            .ask(Completion {
                system: prompts::system_prompt(std::slice::from_ref(&skill)),
                user: prompts::deliverable_message(job),
                effort,
                schema: None,
            })
            .await?;
        Ok(answer.trim().to_owned())
    }

    pub fn deliverable_skills(&self) -> Result<Vec<SkillRef>, StudioError> {
        Ok(self
            .enabled_skills()?
            .into_iter()
            .filter(|skill| runs_as_deliverable(&skill.name))
            .map(|skill| SkillRef {
                name: skill.name,
                title: skill.title,
                stage: skill.stage,
            })
            .collect())
    }

    fn enabled_skills(&self) -> Result<Vec<Skill>, StudioError> {
        Ok(self
            .skills
            .list()
            .map_err(|problem| StudioError::Storage(problem.to_string()))?
            .into_iter()
            .filter(|skill| skill.enabled)
            .collect())
    }
}
