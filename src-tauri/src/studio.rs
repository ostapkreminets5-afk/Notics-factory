use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctics_shared::{
    ART_SKILL, ClaudeAccount, DESIGN_STAGE, GameEngine, Project, SCREEN_SKILLS, Skill, SkillCategory,
    StudioError, guides_the_agent, runs_as_deliverable,
};
use tauri::{AppHandle, Emitter, Manager};

use crate::asset_hunt::Hunter;
use crate::builder::Builder;
use crate::claude_code::{ClaudeCode, Completion, Effort};
use crate::artstation::{ArtStation, Scope};
use crate::open_design;
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
                    let outcome = self.plan(project_id, &prompt, engine).await;
                    let _ = match outcome {
                        Ok(plan) => self
                            .deliverable_skills()
                            .and_then(|skills| self.store.apply_plan(project_id, &plan, &skills))
                            .inspect(|_| {
                                self.collect_references(project_id);
                                self.hunt_assets(project_id);
                                open_design::start_for_plan(&self.app, project_id);
                            }),
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
                    self.advance(output.project_id);
                    self.kick();
                }
                Job::Screen(screen) => {
                    let outcome = self.design(&screen).await;
                    let _ = match outcome {
                        Ok(html) => self.store.finish_screen(screen.screen_id, &html),
                        Err(problem) => self.store.fail_screen(screen.screen_id, &problem),
                    };
                    self.notify();
                    self.advance(screen.project_id);
                }
            }
        }
    }

    pub fn advance(&self, project_id: i64) {
        if !self.store.ready_for_code(project_id).unwrap_or(false) {
            return;
        }
        if let Some(builder) = self.app.try_state::<Arc<Builder>>() {
            let builder = Arc::clone(&builder);
            std::thread::spawn(move || builder.advance(project_id));
        }
    }

    async fn plan(&self, project_id: i64, prompt: &str, engine: GameEngine) -> Result<prompts::Plan, StudioError> {
        let earlier: Vec<Project> = self
            .store
            .list_projects()
            .unwrap_or_default()
            .into_iter()
            .filter(|project| project.id != project_id)
            .collect();
        let answer = self
            .ask(Completion {
                system: prompts::system_prompt(&[]),
                user: prompts::plan_message(prompt, engine, &earlier),
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
        self.collect_references(project_id);
        Ok(updated)
    }

    pub async fn restyle(&self, project_id: i64) -> Result<Project, StudioError> {
        let context = self.store.reference_context(project_id)?;
        let earlier: Vec<Project> = self
            .store
            .list_projects()
            .unwrap_or_default()
            .into_iter()
            .filter(|project| project.id != project_id)
            .collect();
        let brief = prompts::StyleBrief {
            prompt: &context.prompt,
            game_name: &context.game_name,
            pitch: &context.pitch,
            design_document: &context.design_document,
            style: &context.style,
            board: &context.references,
        };
        let answer = self
            .ask(Completion {
                system: prompts::system_prompt(&[]),
                user: prompts::restyle_message(&brief, &earlier),
                effort: Effort::High,
                schema: Some(prompts::style_schema()),
            })
            .await?;
        let style = prompts::parse_style(&answer)?;
        self.store.restyle(project_id, &style)
    }

    fn hunt_assets(&self, project_id: i64) {
        if let Some(hunter) = self.app.try_state::<Arc<Hunter>>() {
            hunter.follow_plan(project_id);
        }
    }

    fn collect_references(&self, project_id: i64) {
        if let Some(pinterest) = self.app.try_state::<Arc<Pinterest>>() {
            let _ = pinterest.load_board(Arc::clone(&self.store), project_id, Refresh::Everything);
        }
        if let Some(artstation) = self.app.try_state::<Arc<ArtStation>>() {
            let _ = artstation.load_board(Arc::clone(&self.store), project_id, Scope::Everything);
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
            board: &job.board,
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
            .filter(|skill| SCREEN_SKILLS.contains(&skill.name.as_str()))
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
        let enabled = self.enabled_skills()?;
        let skill = enabled
            .iter()
            .find(|skill| skill.name == job.skill_name)
            .cloned()
            .ok_or(StudioError::SkillUnavailable)?;
        let effort = if skill.stage <= DESIGN_STAGE {
            Effort::High
        } else {
            Effort::Medium
        };
        let guides: Vec<Skill> = std::iter::once(skill.clone())
            .chain(
                enabled
                    .into_iter()
                    .filter(|art| art.name == ART_SKILL && makes_pictures(&skill)),
            )
            .collect();
        let answer = self
            .ask(Completion {
                system: prompts::system_prompt(&guides),
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
            .filter(|skill| runs_as_deliverable(&skill.name) && !guides_the_agent(skill.stage))
            .map(|skill| SkillRef {
                name: skill.name,
                title: skill.title,
                stage: skill.stage,
            })
            .collect())
    }

    pub fn agent_skills(&self) -> Vec<Skill> {
        self.enabled_skills()
            .unwrap_or_default()
            .into_iter()
            .filter(|skill| runs_as_deliverable(&skill.name) && guides_the_agent(skill.stage))
            .collect()
    }

    pub fn enabled_skill(&self, name: &str) -> Option<Skill> {
        self.enabled_skills()
            .ok()?
            .into_iter()
            .find(|skill| skill.name == name)
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

fn makes_pictures(skill: &Skill) -> bool {
    skill.name != ART_SKILL && skill.category != SkillCategory::GameDesign
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str, category: SkillCategory) -> Skill {
        Skill {
            id: 0,
            name: name.into(),
            stage: 2,
            title: String::new(),
            summary: String::new(),
            category,
            instructions: String::new(),
            enabled: true,
        }
    }

    #[test]
    fn art_rules_follow_skills_that_make_pictures_or_code() {
        assert!(makes_pictures(&skill("game-scripts", SkillCategory::Code)));
        assert!(makes_pictures(&skill("concept-prompts", SkillCategory::Art)));
        assert!(!makes_pictures(&skill("lore-and-naming", SkillCategory::GameDesign)));
        assert!(!makes_pictures(&skill(ART_SKILL, SkillCategory::Art)));
    }
}
