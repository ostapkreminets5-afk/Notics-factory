use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use noctics_shared::{
    ARTWORKS_PER_REFERENCE, ArtstationError, ArtstationStatus, Artwork, JournalEvent, Reference, StudioError,
};
use serde::Deserialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use crate::studio::{CHANGED_EVENT, Studio};
use crate::studio_store::StudioStore;

const TOKEN_URL: &str = "https://www.artstation.com/api/v2/csrf_protection/token.json";
const SEARCH_URL: &str = "https://www.artstation.com/api/v2/search/projects.json";
const SITE_ORIGIN: &str = "https://www.artstation.com";
const TOKEN_HEADER: &str = "PUBLIC-CSRF-TOKEN";
const JSON_TYPE: &str = "application/json";
const SORTING: &str = "relevance";
const PAGE_SIZE: usize = 50;
const MAX_PAGES: usize = 4;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const SESSION_LIFETIME: Duration = Duration::from_secs(20 * 60);
const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;
const STARTUP_DELAY: Duration = Duration::from_secs(6);
const SEARCH_PAUSE: Duration = Duration::from_millis(400);
const REJECTED_STATUSES: [u16; 3] = [401, 403, 412];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Everything,
    Missing,
}

impl Scope {
    fn wants(self, reference: &Reference) -> bool {
        match self {
            Self::Everything => true,
            Self::Missing => reference.artworks.is_empty(),
        }
    }
}

#[derive(Clone)]
struct Session {
    token: String,
    cookie: String,
    opened: Instant,
}

#[derive(Deserialize)]
struct TokenReply {
    public_csrf_token: String,
}

#[derive(Deserialize)]
struct SearchPage {
    #[serde(default)]
    data: Vec<RawArtwork>,
}

#[derive(Deserialize)]
struct RawArtwork {
    hash_id: Option<String>,
    title: Option<String>,
    smaller_square_cover_url: Option<String>,
    hide_as_adult: Option<bool>,
    is_adult_content: Option<bool>,
    user: Option<RawAuthor>,
}

#[derive(Deserialize)]
struct RawAuthor {
    full_name: Option<String>,
    username: Option<String>,
}

impl RawArtwork {
    fn artwork(&self) -> Option<Artwork> {
        if self.hide_as_adult == Some(true) || self.is_adult_content == Some(true) {
            return None;
        }
        Artwork::checked(
            self.hash_id.as_deref()?,
            self.smaller_square_cover_url.as_deref()?,
            self.title.as_deref().unwrap_or_default(),
            &self.author(),
        )
    }

    fn author(&self) -> String {
        let Some(user) = &self.user else {
            return String::new();
        };
        [&user.full_name, &user.username]
            .into_iter()
            .flatten()
            .map(|name| name.trim())
            .find(|name| !name.is_empty())
            .unwrap_or_default()
            .to_owned()
    }
}

pub struct ArtstationClient {
    agent: ureq::Agent,
    session: Mutex<Option<Session>>,
}

impl ArtstationClient {
    pub fn new() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(REQUEST_TIMEOUT))
                .http_status_as_error(false)
                .build()
                .into(),
            session: Mutex::new(None),
        }
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Artwork>, ArtstationError> {
        let mut artworks: Vec<Artwork> = Vec::new();
        for page in 1..=MAX_PAGES {
            let found = self.search_page(query, page)?;
            let exhausted = found.data.len() < PAGE_SIZE;
            for artwork in found.data.iter().filter_map(RawArtwork::artwork) {
                if artworks.len() >= limit {
                    break;
                }
                if !artworks.iter().any(|known| known.id == artwork.id) {
                    artworks.push(artwork);
                }
            }
            if artworks.len() >= limit || exhausted {
                break;
            }
        }
        Ok(artworks)
    }

    fn search_page(&self, query: &str, page: usize) -> Result<SearchPage, ArtstationError> {
        if let Some(found) = self.post_search(&self.session(false)?, query, page)? {
            return Ok(found);
        }
        self.post_search(&self.session(true)?, query, page)?
            .ok_or_else(|| network("ArtStation rejected the search session"))
    }

    fn post_search(&self, session: &Session, query: &str, page: usize) -> Result<Option<SearchPage>, ArtstationError> {
        let body = json!({
            "query": query,
            "page": page,
            "per_page": PAGE_SIZE,
            "sorting": SORTING,
            "pro_first": "1",
            "filters": [],
            "additional_fields": [],
        });
        let mut response = self
            .agent
            .post(SEARCH_URL)
            .header("Accept", JSON_TYPE)
            .header("Content-Type", JSON_TYPE)
            .header("Origin", SITE_ORIGIN)
            .header(TOKEN_HEADER, session.token.as_str())
            .header("Cookie", session.cookie.as_str())
            .send(body.to_string())
            .map_err(network)?;
        let status = response.status().as_u16();
        if REJECTED_STATUSES.contains(&status) {
            return Ok(None);
        }
        let text = read_text(&mut response)?;
        if status != 200 {
            return Err(network(format!("search answered HTTP {status}")));
        }
        serde_json::from_str(&text).map(Some).map_err(network)
    }

    fn session(&self, renew: bool) -> Result<Session, ArtstationError> {
        let mut slot = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let reusable = slot
            .as_ref()
            .filter(|session| !renew && session.opened.elapsed() < SESSION_LIFETIME);
        if let Some(session) = reusable {
            return Ok(session.clone());
        }
        let fresh = self.open_session()?;
        *slot = Some(fresh.clone());
        Ok(fresh)
    }

    fn open_session(&self) -> Result<Session, ArtstationError> {
        let mut response = self
            .agent
            .post(TOKEN_URL)
            .header("Accept", JSON_TYPE)
            .header("Content-Type", JSON_TYPE)
            .header("Origin", SITE_ORIGIN)
            .send("{}".to_owned())
            .map_err(network)?;
        let status = response.status().as_u16();
        let cookie = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .map(str::trim)
            .filter(|pair| pair.contains('='))
            .collect::<Vec<_>>()
            .join("; ");
        let text = read_text(&mut response)?;
        if status != 200 {
            return Err(network(format!("CSRF token answered HTTP {status}")));
        }
        let reply: TokenReply = serde_json::from_str(&text).map_err(network)?;
        Ok(Session {
            token: reply.public_csrf_token,
            cookie,
            opened: Instant::now(),
        })
    }
}

#[derive(Default)]
struct Boards {
    loading: BTreeSet<i64>,
    empty: BTreeSet<i64>,
}

pub struct ArtStation {
    app: AppHandle,
    client: ArtstationClient,
    boards: Mutex<Boards>,
    board_turn: Mutex<()>,
}

impl ArtStation {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            client: ArtstationClient::new(),
            boards: Mutex::new(Boards::default()),
            board_turn: Mutex::new(()),
        }
    }

    pub fn status(&self) -> ArtstationStatus {
        let boards = self.boards();
        ArtstationStatus {
            loading: boards.loading.iter().copied().collect(),
            empty: boards.empty.iter().copied().collect(),
        }
    }

    pub fn is_loading(&self, project_id: i64) -> bool {
        self.boards().loading.contains(&project_id)
    }

    pub fn search_artworks(&self, query: &str, limit: usize) -> Result<Vec<Artwork>, ArtstationError> {
        self.client.search(query, limit)
    }

    pub fn load_board(
        self: &Arc<Self>,
        store: Arc<StudioStore>,
        project_id: i64,
        scope: Scope,
    ) -> Result<(), ArtstationError> {
        let board = store.reference_board(project_id).map_err(board_error)?;
        if board.is_empty() {
            return Err(ArtstationError::NoReferences);
        }
        let had_artworks = board.iter().any(|reference| !reference.artworks.is_empty());
        let targets: Vec<(usize, String)> = board
            .into_iter()
            .enumerate()
            .filter(|(_, reference)| !reference.topic.is_audio() && scope.wants(reference))
            .map(|(index, reference)| (index, reference.query))
            .collect();
        if targets.is_empty() || !self.boards().loading.insert(project_id) {
            return Ok(());
        }
        self.boards().empty.remove(&project_id);
        self.notify();
        let artstation = Arc::clone(self);
        thread::spawn(move || {
            let turn = artstation
                .board_turn
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut found = 0;
            for (position, (index, query)) in targets.iter().enumerate() {
                if position > 0 {
                    thread::sleep(SEARCH_PAUSE);
                }
                let Ok(artworks) = artstation.search_artworks(query, ARTWORKS_PER_REFERENCE) else {
                    break;
                };
                if artworks.is_empty() {
                    continue;
                }
                found += artworks.len();
                match store.set_reference_artworks(project_id, *index, query, &artworks) {
                    Ok(true) => artstation.notify(),
                    _ => break,
                }
            }
            drop(turn);
            let came_back_empty = found == 0 && !had_artworks;
            if came_back_empty {
                let _ = store.note(project_id, &JournalEvent::ArtworksMissing);
            }
            let mut boards = artstation.boards();
            boards.loading.remove(&project_id);
            if came_back_empty {
                boards.empty.insert(project_id);
            }
            drop(boards);
            artstation.notify();
        });
        Ok(())
    }

    fn notify(&self) {
        let _ = self.app.emit(CHANGED_EVENT, ());
    }

    fn boards(&self) -> MutexGuard<'_, Boards> {
        self.boards
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub fn resume_after_start(app: AppHandle) {
    thread::spawn(move || {
        thread::sleep(STARTUP_DELAY);
        let (Some(artstation), Some(studio)) = (
            app.try_state::<Arc<ArtStation>>(),
            app.try_state::<Arc<Studio>>(),
        ) else {
            return;
        };
        let Ok(projects) = studio.store.list_projects() else {
            return;
        };
        for project in projects.iter().filter(|project| !project.status.is_planning()) {
            let _ = artstation.load_board(Arc::clone(&studio.store), project.id, Scope::Missing);
        }
    });
}

fn read_text(response: &mut ureq::http::Response<ureq::Body>) -> Result<String, ArtstationError> {
    response
        .body_mut()
        .with_config()
        .limit(MAX_JSON_BYTES)
        .read_to_string()
        .map_err(network)
}

fn network(error: impl std::fmt::Display) -> ArtstationError {
    ArtstationError::Network(error.to_string())
}

fn board_error(problem: StudioError) -> ArtstationError {
    match problem {
        StudioError::NotFound => ArtstationError::NotFound,
        StudioError::StillPlanning => ArtstationError::NoReferences,
        other => ArtstationError::Storage(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use noctics_shared::ReferenceTopic;

    use super::*;

    const COVER: &str = "https://cdna.artstation.com/p/assets/images/images/085/683/878/smaller_square/sketch.jpg?1741371096";

    fn reference(artworks: usize) -> Reference {
        Reference {
            topic: ReferenceTopic::World,
            title: "Мокрі дахи".into(),
            note: String::new(),
            query: "rainy rooftops".into(),
            palette: Vec::new(),
            pins: Vec::new(),
            artworks: (0..artworks)
                .map(|index| Artwork::checked(&format!("a{index}"), COVER, "", "").unwrap())
                .collect(),
        }
    }

    #[test]
    fn refreshes_only_what_the_board_still_needs() {
        assert!(Scope::Missing.wants(&reference(0)));
        assert!(!Scope::Missing.wants(&reference(2)));
        assert!(Scope::Everything.wants(&reference(2)));
    }

    #[test]
    fn reads_search_results_and_skips_adult_work() {
        let page: SearchPage = serde_json::from_str(&format!(
            r#"{{"total_count":3,"data":[
                {{"hash_id":"2B1Xae","title":"Castle","smaller_square_cover_url":"{COVER}","hide_as_adult":false,"is_adult_content":false,"user":{{"full_name":" ","username":"dechambo"}}}},
                {{"hash_id":"lD0q0k","title":null,"smaller_square_cover_url":"{COVER}","hide_as_adult":true,"user":null}},
                {{"hash_id":"rlRAbe","title":"No cover","smaller_square_cover_url":null}}
            ]}}"#
        ))
        .unwrap();
        let artworks: Vec<Artwork> = page.data.iter().filter_map(RawArtwork::artwork).collect();
        assert_eq!(artworks.len(), 1);
        assert_eq!(artworks[0].id, "2B1Xae");
        assert_eq!(artworks[0].title, "Castle");
        assert_eq!(artworks[0].author, "dechambo");
    }

    #[test]
    #[ignore = "talks to the real ArtStation"]
    fn searches_the_real_artstation() {
        let client = ArtstationClient::new();
        let artworks = client.search("dark fantasy castle", 60).unwrap();
        assert_eq!(artworks.len(), 60);
        assert!(artworks.iter().all(|artwork| artwork.image.contains("/smaller_square/")));
        let again = client.search("neon city", 5).unwrap();
        assert_eq!(again.len(), 5);
    }
}
