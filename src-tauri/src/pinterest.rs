use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use noctics_shared::{
    JournalEvent, PINS_PER_REFERENCE, Pin, PinterestError, PinterestStatus, Reference, StudioError,
};
use serde::Deserialize;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use url::Url;

use crate::studio::{CHANGED_EVENT, Studio};
use crate::studio_store::StudioStore;

const HOME_URL: &str = "https://www.pinterest.com/";
const LOGIN_URL: &str = "https://www.pinterest.com/login/";
const SEARCH_URL: &str = "https://www.pinterest.com/search/pins/";
const SESSION_COOKIE: &str = "_auth";
const SIGNED_IN_VALUE: &str = "1";
const MAIN_WINDOW: &str = "main";
const LOGIN_WINDOW: &str = "pinterest-login";
const LOGIN_TITLE: &str = "Вхід у Pinterest · Noctics";
const SEARCH_WINDOW_PREFIX: &str = "pinterest-search-";
const STARTUP_DELAY: Duration = Duration::from_secs(4);
const SEARCH_TIMEOUT: Duration = Duration::from_secs(25);
const HARVEST_INTERVAL: Duration = Duration::from_millis(700);
const HARVEST_REPLY_TIMEOUT: Duration = Duration::from_secs(5);
const HARVEST_SCRIPT: &str = r#"(() => {
  try {
    if (window.top !== window || !/(^|\.)pinterest\.[a-z.]+$/.test(location.hostname)) {
      return [];
    }
    const linkOf = (image) => {
      const card = image.closest('[data-test-id="pin"], [data-grid-item="true"]');
      return image.closest('a[href*="/pin/"]') || (card && card.querySelector('a[href*="/pin/"]'));
    };
    const pins = [];
    for (const image of document.querySelectorAll('img[src*="i.pinimg.com"]')) {
      if (image.closest('header, [data-test-id="header"]')) {
        continue;
      }
      const link = linkOf(image);
      const found = link && (link.getAttribute('href') || '').match(/\/pin\/(\d+)/);
      const alt = (image.getAttribute('alt') || '').trim();
      pins.push({
        id: found ? found[1] : '',
        image: image.getAttribute('src') || '',
        title: alt.split(/\s+/).length > 1 ? alt : '',
      });
    }
    window.scrollBy(0, Math.round(window.innerHeight * 0.75));
    return pins;
  } catch (error) {
    return [];
  }
})()"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    Everything,
    Missing { previews_too: bool },
}

impl Refresh {
    fn wants(self, reference: &Reference) -> bool {
        match self {
            Self::Everything => true,
            Self::Missing { previews_too } => {
                reference.pins.is_empty()
                    || (previews_too && !reference.pins.iter().any(Pin::is_linked))
            }
        }
    }
}

#[derive(Deserialize)]
struct RawPin {
    id: String,
    image: String,
    #[serde(default)]
    title: String,
}

#[derive(Default)]
struct Boards {
    loading: BTreeSet<i64>,
    empty: BTreeSet<i64>,
}

pub struct Pinterest {
    app: AppHandle,
    boards: Mutex<Boards>,
    searching: Mutex<()>,
    next_window: AtomicU64,
}

impl Pinterest {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            boards: Mutex::new(Boards::default()),
            searching: Mutex::new(()),
            next_window: AtomicU64::new(1),
        }
    }

    pub fn status(&self) -> PinterestStatus {
        let boards = self.boards();
        PinterestStatus {
            signed_in: signed_in(&self.app),
            loading: boards.loading.iter().copied().collect(),
            empty: boards.empty.iter().copied().collect(),
        }
    }

    pub fn open_login(&self) -> Result<(), PinterestError> {
        if let Some(window) = self.app.get_webview_window(LOGIN_WINDOW) {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
            return Ok(());
        }
        let watcher = self.app.clone();
        let window = WebviewWindowBuilder::new(
            &self.app,
            LOGIN_WINDOW,
            WebviewUrl::External(parse(LOGIN_URL)?),
        )
        .title(LOGIN_TITLE)
        .inner_size(520.0, 760.0)
        .center()
        .on_page_load(move |window, payload| {
            let left_login = !payload.url().path().starts_with("/login");
            if matches!(payload.event(), PageLoadEvent::Finished) && left_login {
                let app = watcher.clone();
                thread::spawn(move || {
                    if signed_in(&app) == Some(true) {
                        let _ = window.close();
                    }
                });
            }
        })
        .build()
        .map_err(browser)?;
        let app = self.app.clone();
        window.on_window_event(move |event| {
            if matches!(event, WindowEvent::Destroyed) {
                let app = app.clone();
                thread::spawn(move || {
                    if signed_in(&app) == Some(true) {
                        resume_boards(&app, true);
                    }
                    let _ = app.emit(CHANGED_EVENT, ());
                });
            }
        });
        Ok(())
    }

    pub fn load_board(
        self: &Arc<Self>,
        store: Arc<StudioStore>,
        project_id: i64,
        refresh: Refresh,
    ) -> Result<(), PinterestError> {
        let board = store.reference_board(project_id).map_err(board_error)?;
        if board.is_empty() {
            return Err(PinterestError::NoReferences);
        }
        let had_pins = board.iter().any(|reference| !reference.pins.is_empty());
        let targets: Vec<(usize, String)> = board
            .into_iter()
            .enumerate()
            .filter(|(_, reference)| !reference.topic.is_audio() && refresh.wants(reference))
            .map(|(index, reference)| (index, reference.query))
            .collect();
        if targets.is_empty() || !self.boards().loading.insert(project_id) {
            return Ok(());
        }
        self.boards().empty.remove(&project_id);
        self.notify();
        let pinterest = Arc::clone(self);
        thread::spawn(move || {
            let mut found = 0;
            for (index, query) in &targets {
                let pins = pinterest.search(query).unwrap_or_default();
                if pins.is_empty() {
                    continue;
                }
                found += pins.len();
                match store.set_reference_pins(project_id, *index, query, &pins) {
                    Ok(true) => pinterest.notify(),
                    _ => break,
                }
            }
            let came_back_empty = found == 0 && !had_pins;
            if came_back_empty {
                let _ = store.note(project_id, &JournalEvent::PinsMissing);
            }
            let mut boards = pinterest.boards();
            boards.loading.remove(&project_id);
            if came_back_empty {
                boards.empty.insert(project_id);
            }
            drop(boards);
            pinterest.notify();
        });
        Ok(())
    }

    fn search(&self, query: &str) -> Result<Vec<Pin>, PinterestError> {
        self.search_pins(query, PINS_PER_REFERENCE, SEARCH_TIMEOUT)
    }

    pub fn search_pins(
        &self,
        query: &str,
        limit: usize,
        timeout: Duration,
    ) -> Result<Vec<Pin>, PinterestError> {
        let _turn = self
            .searching
            .lock()
            .map_err(|_| PinterestError::Browser("search lock is poisoned".into()))?;
        let mut link = parse(SEARCH_URL)?;
        link.query_pairs_mut().append_pair("q", query);
        let label = format!(
            "{SEARCH_WINDOW_PREFIX}{}",
            self.next_window.fetch_add(1, Ordering::SeqCst)
        );
        let window = WebviewWindowBuilder::new(&self.app, label, WebviewUrl::External(link))
            .title("Pinterest")
            .inner_size(1280.0, 1600.0)
            .visible(false)
            .focused(false)
            .skip_taskbar(true)
            .on_new_window(|_, _| NewWindowResponse::Deny)
            .build()
            .map_err(browser)?;
        let harvested = harvest(&window, limit, timeout);
        let _ = window.destroy();
        harvested
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

fn harvest(window: &WebviewWindow, limit: usize, timeout: Duration) -> Result<Vec<Pin>, PinterestError> {
    let deadline = Instant::now() + timeout;
    let mut pins: Vec<Pin> = Vec::new();
    while pins.len() < limit && Instant::now() < deadline {
        thread::sleep(HARVEST_INTERVAL);
        let (sender, receiver) = mpsc::channel();
        window
            .eval_with_callback(HARVEST_SCRIPT, move |reply| {
                let _ = sender.send(reply);
            })
            .map_err(browser)?;
        let Ok(reply) = receiver.recv_timeout(HARVEST_REPLY_TIMEOUT) else {
            continue;
        };
        let found: Vec<RawPin> = serde_json::from_str(&reply).unwrap_or_default();
        for raw in found {
            if pins.len() >= limit {
                break;
            }
            let Some(pin) = Pin::checked(&raw.id, &raw.image, &raw.title) else {
                continue;
            };
            if !pins.iter().any(|known| known.same_as(&pin)) {
                pins.push(pin);
            }
        }
    }
    Ok(pins)
}

pub fn resume_after_start(app: AppHandle) {
    thread::spawn(move || {
        thread::sleep(STARTUP_DELAY);
        let previews_too = signed_in(&app) == Some(true);
        resume_boards(&app, previews_too);
    });
}

fn resume_boards(app: &AppHandle, previews_too: bool) {
    let (Some(pinterest), Some(studio)) = (
        app.try_state::<Arc<Pinterest>>(),
        app.try_state::<Arc<Studio>>(),
    ) else {
        return;
    };
    let Ok(projects) = studio.store.list_projects() else {
        return;
    };
    let refresh = Refresh::Missing { previews_too };
    for project in projects.iter().filter(|project| !project.status.is_planning()) {
        let _ = pinterest.load_board(Arc::clone(&studio.store), project.id, refresh);
    }
}

fn signed_in(app: &AppHandle) -> Option<bool> {
    let window = app.get_webview_window(MAIN_WINDOW)?;
    let cookies = window.cookies_for_url(Url::parse(HOME_URL).ok()?).ok()?;
    Some(
        cookies
            .iter()
            .any(|cookie| cookie.name() == SESSION_COOKIE && cookie.value() == SIGNED_IN_VALUE),
    )
}

fn parse(link: &str) -> Result<Url, PinterestError> {
    Url::parse(link).map_err(browser)
}

fn browser(error: impl std::fmt::Display) -> PinterestError {
    PinterestError::Browser(error.to_string())
}

fn board_error(problem: StudioError) -> PinterestError {
    match problem {
        StudioError::NotFound => PinterestError::NotFound,
        StudioError::StillPlanning => PinterestError::NoReferences,
        other => PinterestError::Storage(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use noctics_shared::ReferenceTopic;

    use super::*;

    fn reference(pins: &[(&str, &str)]) -> Reference {
        Reference {
            topic: ReferenceTopic::World,
            title: "Мокрі дахи".into(),
            note: String::new(),
            query: "rainy rooftops".into(),
            palette: Vec::new(),
            pins: pins
                .iter()
                .map(|(id, image)| Pin::checked(id, image, "").unwrap())
                .collect(),
        }
    }

    #[test]
    fn refreshes_only_what_the_board_still_needs() {
        let empty = reference(&[]);
        let preview = reference(&[("", "https://i.pinimg.com/236x/aa/bb.jpg")]);
        let linked = reference(&[("42", "https://i.pinimg.com/236x/cc/dd.jpg")]);
        let quiet = Refresh::Missing { previews_too: false };
        let upgrade = Refresh::Missing { previews_too: true };

        assert!(quiet.wants(&empty) && upgrade.wants(&empty));
        assert!(!quiet.wants(&preview) && upgrade.wants(&preview));
        assert!(!quiet.wants(&linked) && !upgrade.wants(&linked));
        assert!(Refresh::Everything.wants(&linked));
    }
}
