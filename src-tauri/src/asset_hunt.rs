use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use noctics_shared::{
    AssetCost, AssetFind, AssetHunt, FindKind, JournalEvent, MAX_ASSET_FINDS, MAX_HUNT_REQUEST_CHARS,
    MAX_KEPT_FINDS, ProjectDetail, RawFind, StudioError,
};
use serde_json::{Value, json};

use crate::asset_hunt_store::{Claimed, HuntStore};
use crate::asset_previews;
use crate::claude_code::{Completion, Effort};
use crate::prompts;
use crate::studio::Studio;

const MAX_DESIGN_CHARS: usize = 6000;
const NOTHING_FOUND: &str = "пошук не дав жодного робочого посилання на ассет";

const SYSTEM_PROMPT: &str = "Ти — розвідник ассетів Noctics Factory. Студія робить гру за промптом гравця, \
а ти шукаєш в інтернеті готові ассети, які пасують саме цій грі, щоб агент коду міг їх використати.\n\n\
Як працювати:\n\
- Шукай через WebSearch, а сторінки, у яких сумніваєшся, відкривай через WebFetch. Повертай лише сторінки, \
які справді існують і які ти бачив у результатах пошуку або відкрив. Не вигадуй адрес.\n\
- Перш за все дивись itch.io (itch.io/game-assets, особливо itch.io/game-assets/free і теги стилю), потім OpenGameArt, Kenney, \
Quaternius, Poly Pizza, Poly Haven, ambientCG, Freesound, Pixabay, Google Fonts. Платні паки — itch.io, GameDev Market, CraftPix, \
Unity Asset Store, Fab — лише коли вони набагато влучніші за безкоштовні.\n\
- Посилання веде на сторінку конкретного пака чи ассета, а не на пошук, тег, колекцію чи головну сайту.\n\
- Ассет має пасувати грі: та сама перспектива (вид збоку, зверху, ізометрія, 3D), роздільна здатність піксель-арту, \
палітра, настрій і жанр. Під 3D-рушій шукай GLB/FBX-моделі й PBR-текстури, під 2D — спрайт-листи, тайлсети, фони.\n\
- Покрий справжні потреби гри: персонажі й вороги, оточення й фони, тайли, предмети, інтерфейс, іконки, VFX, звуки, музика, шрифти — \
лише те, що потрібно цій грі. Кілька сильних варіантів на важливу потребу краще за один слабкий на кожну.\n\
- Віддавай перевагу безкоштовним ассетам з ліцензією, що дозволяє використання в комерційних іграх: CC0, CC-BY або ліцензія автора.\n\n\
Поля кожної знахідки:\n\
- title — назва пака як на сторінці; author — автор або студія;\n\
- kind — одна з категорій схеми;\n\
- license — коротко, як на сторінці: «CC0», «CC-BY 4.0», «власна: комерційно можна, без перепродажу»; якщо ліцензії не видно — «не вказано»;\n\
- cost — free (безкоштовне завантаження), pay_what_you_want (ціну обирає покупець, можна 0), paid (лише за гроші); price — ціна для paid, інакше порожньо;\n\
- fit — одне речення мовою промпту гравця: чому пасує саме цій грі й куди піде.\n\
summary — одне-два речення мовою промпту гравця: що знайдено і чого бракує, що краще згенерувати в ComfyUI.";

pub struct Hunter {
    store: Arc<HuntStore>,
    studio: Arc<Studio>,
    working: AtomicBool,
}

struct Found {
    summary: String,
    finds: Vec<AssetFind>,
}

impl Hunter {
    pub fn new(store: Arc<HuntStore>, studio: Arc<Studio>) -> Self {
        Self {
            store,
            studio,
            working: AtomicBool::new(false),
        }
    }

    pub fn hunt(&self, project_id: i64) -> Result<Option<AssetHunt>, StudioError> {
        self.store.hunt(project_id)
    }

    pub fn start(self: &Arc<Self>, project_id: i64, request: &str) -> Result<AssetHunt, StudioError> {
        self.studio.require_connection()?;
        let project = self.studio.store.detail(project_id)?.project;
        if project.status.is_planning() {
            return Err(StudioError::StillPlanning);
        }
        let request: String = request.trim().chars().take(MAX_HUNT_REQUEST_CHARS).collect();
        let hunt = self.store.queue(project_id, &request)?;
        self.studio.notify();
        self.kick();
        Ok(hunt)
    }

    pub fn follow_plan(self: &Arc<Self>, project_id: i64) {
        if self.store.queue_if_new(project_id).unwrap_or(false) {
            self.studio.notify();
            self.kick();
        }
    }

    pub fn forget(&self, project_id: i64) -> Result<(), StudioError> {
        self.store.forget(project_id)
    }

    pub fn find(&self, project_id: i64, url: &str) -> Result<AssetFind, StudioError> {
        self.store
            .hunt(project_id)?
            .and_then(|hunt| hunt.finds.into_iter().find(|find| find.url == url))
            .ok_or(StudioError::NotFound)
    }

    pub fn brief(&self, project_id: i64) -> Option<String> {
        self.store
            .hunt(project_id)
            .ok()
            .flatten()
            .and_then(|hunt| brief_markdown(&hunt))
    }

    pub fn kick(self: &Arc<Self>) {
        if !self.store.has_queued().unwrap_or(false) {
            return;
        }
        if self
            .working
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let hunter = Arc::clone(self);
        std::thread::spawn(move || {
            let drained = hunter.drain();
            hunter.working.store(false, Ordering::SeqCst);
            if drained {
                hunter.kick();
            }
        });
    }

    fn drain(&self) -> bool {
        loop {
            if self.studio.require_connection().is_err() {
                return false;
            }
            let claimed = match self.store.claim() {
                Ok(Some(claimed)) => claimed,
                Ok(None) => return true,
                Err(_) => return false,
            };
            self.studio.notify();
            match self.search(&claimed) {
                Ok(found) => {
                    let _ = self.store.finish(claimed.project_id, &found.summary, &found.finds);
                    let _ = self.studio.store.note(
                        claimed.project_id,
                        &JournalEvent::AssetsFound {
                            count: u32::try_from(found.finds.len()).unwrap_or(u32::MAX),
                        },
                    );
                }
                Err(StudioError::NotFound) => {
                    let _ = self.store.forget(claimed.project_id);
                }
                Err(problem) => {
                    let halts = halts_queue(&problem);
                    let _ = self.store.fail(claimed.project_id, &problem);
                    let _ = self
                        .studio
                        .store
                        .note(claimed.project_id, &JournalEvent::AssetHuntFailed { error: problem });
                    if halts {
                        self.studio.notify();
                        return false;
                    }
                }
            }
            self.studio.notify();
        }
    }

    fn search(&self, claimed: &Claimed) -> Result<Found, StudioError> {
        let detail = self.studio.store.detail(claimed.project_id)?;
        let previous = self
            .store
            .hunt(claimed.project_id)?
            .map(|hunt| hunt.finds)
            .unwrap_or_default();
        let keep_previous = !claimed.request.is_empty();
        let avoid: Vec<&AssetFind> = if keep_previous { previous.iter().collect() } else { Vec::new() };
        let answer = self.studio.claude().research(&Completion {
            system: SYSTEM_PROMPT.to_owned(),
            user: hunt_message(&detail, &claimed.request, &avoid),
            effort: Effort::Medium,
            schema: Some(hunt_schema()),
        })?;
        let mut found = parse_hunt(&answer)?;
        asset_previews::attach(&mut found.finds);
        if keep_previous {
            found.finds = merged(found.finds, previous);
        }
        Ok(found)
    }
}

fn halts_queue(problem: &StudioError) -> bool {
    matches!(
        problem,
        StudioError::ClaudeCodeMissing | StudioError::ClaudeSignedOut | StudioError::UsageLimit
    )
}

fn hunt_message(detail: &ProjectDetail, request: &str, avoid: &[&AssetFind]) -> String {
    let project = &detail.project;
    let style = &project.style;
    let screens = detail
        .screens
        .iter()
        .map(|screen| format!("- {} — {}", screen.title, screen.summary))
        .collect::<Vec<_>>()
        .join("\n");
    let design: String = detail.design_document.chars().take(MAX_DESIGN_CHARS).collect();
    let mut message = format!(
        "Знайди готові ассети для гри «{}».\n\nПітч: {}\n\nПромпт гравця:\n{}\n\n{}\n\n\
Стиль: палітра {}; шрифти {}; настрій {}.\n\nЕкрани:\n{}\n\nДизайн-документ:\n{}\n",
        project.name,
        project.pitch,
        project.prompt,
        prompts::engine_brief(project.engine),
        style.palette.join(", "),
        style.fonts,
        style.mood,
        screens,
        design,
    );
    if !project.references.is_empty() {
        message.push_str(&format!(
            "\nДошка референсів — тримай цей стиль:\n{}\n",
            prompts::describe_references(&project.references)
        ));
    }
    if !request.is_empty() {
        message.push_str(&format!("\nГравець просить окремо знайти: {request}\nЗосередься саме на цьому.\n"));
    }
    if !avoid.is_empty() {
        let known = avoid
            .iter()
            .map(|find| format!("- {}", find.url))
            .collect::<Vec<_>>()
            .join("\n");
        message.push_str(&format!("\nЦі сторінки вже знайдено — не повторюй їх:\n{known}\n"));
    }
    message.push_str(&format!("\nПоверни від 8 до {MAX_ASSET_FINDS} знахідок без дублікатів."));
    message
}

fn hunt_schema() -> Value {
    let kinds: Vec<&str> = FindKind::ALL.into_iter().map(FindKind::code).collect();
    let costs: Vec<&str> = AssetCost::ALL.into_iter().map(AssetCost::code).collect();
    json!({
        "type": "object",
        "properties": {
            "summary": { "type": "string" },
            "finds": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "url": { "type": "string" },
                        "kind": { "type": "string", "enum": kinds },
                        "author": { "type": "string" },
                        "license": { "type": "string" },
                        "cost": { "type": "string", "enum": costs },
                        "price": { "type": "string" },
                        "fit": { "type": "string" }
                    },
                    "required": ["title", "url", "kind", "author", "license", "cost", "price", "fit"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["summary", "finds"],
        "additionalProperties": false
    })
}

fn parse_hunt(answer: &str) -> Result<Found, StudioError> {
    let report: Value = serde_json::from_str(answer.trim())
        .map_err(|_| StudioError::Claude("відповідь розвідника ассетів не схожа на JSON".into()))?;
    let text = |item: &Value, key: &str| item[key].as_str().unwrap_or_default().to_owned();
    let mut seen = HashSet::new();
    let finds: Vec<AssetFind> = report["finds"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|item| {
            let fields = [
                "title", "url", "kind", "author", "license", "cost", "price", "fit",
            ]
            .map(|key| text(item, key));
            AssetFind::checked(&RawFind {
                title: &fields[0],
                url: &fields[1],
                kind: &fields[2],
                author: &fields[3],
                license: &fields[4],
                cost: &fields[5],
                price: &fields[6],
                fit: &fields[7],
            })
        })
        .filter(|find| seen.insert(link_key(&find.url)))
        .take(MAX_ASSET_FINDS)
        .collect();
    if finds.is_empty() {
        return Err(StudioError::Claude(NOTHING_FOUND.into()));
    }
    Ok(Found {
        summary: report["summary"].as_str().unwrap_or_default().trim().to_owned(),
        finds,
    })
}

fn merged(fresh: Vec<AssetFind>, previous: Vec<AssetFind>) -> Vec<AssetFind> {
    let mut seen = HashSet::new();
    fresh
        .into_iter()
        .chain(previous)
        .filter(|find| seen.insert(link_key(&find.url)))
        .take(MAX_KEPT_FINDS)
        .collect()
}

fn link_key(url: &str) -> String {
    url.trim_end_matches('/').to_lowercase()
}

pub fn brief_markdown(hunt: &AssetHunt) -> Option<String> {
    if hunt.finds.is_empty() {
        return None;
    }
    let mut text = String::from("# Знайдені ассети\n\n");
    if !hunt.summary.is_empty() {
        text.push_str(&format!("{}\n\n", hunt.summary));
    }
    for kind in hunt.kinds() {
        text.push_str(&format!("## {}\n\n", kind.label()));
        for find in hunt.finds.iter().filter(|find| find.kind == kind) {
            let author = if find.author.is_empty() {
                String::new()
            } else {
                format!(", автор {}", find.author)
            };
            text.push_str(&format!(
                "- [{}]({}) — {}{author}. Ліцензія: {}. {}. {}\n",
                find.title,
                find.url,
                find.site,
                if find.license.is_empty() { "не вказано" } else { &find.license },
                find.cost_label(),
                find.fit
            ));
        }
        text.push('\n');
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use noctics_shared::{GameEngine, HuntStatus, Project, ProjectStatus, PromptAnalysis, Screen, ScreenStatus, StyleGuide};

    use super::*;

    const ANSWER: &str = r#"{
        "summary": "Знайшов піксельних ніндзя й тайли лісу",
        "finds": [
            {"title": "Ninja Adventure", "url": "https://pixel-boy.itch.io/ninja-adventure-asset-pack", "kind": "characters",
             "author": "Pixel-Boy", "license": "CC0", "cost": "free", "price": "", "fit": "Той самий вид зверху"},
            {"title": "Дубль", "url": "https://pixel-boy.itch.io/ninja-adventure-asset-pack/", "kind": "characters",
             "author": "", "license": "", "cost": "free", "price": "", "fit": ""},
            {"title": "Forest Tiles", "url": "https://www.gamedevmarket.net/asset/forest-tiles", "kind": "tiles",
             "author": "Cainos", "license": "власна", "cost": "paid", "price": "$9.99", "fit": "Ліс для другого рівня"},
            {"title": "Фейк", "url": "javascript:alert(1)", "kind": "music", "author": "", "license": "", "cost": "free", "price": "", "fit": ""},
            {"title": "Космос", "url": "https://kenney.nl/assets/space", "kind": "spaceships", "author": "", "license": "", "cost": "free", "price": "", "fit": ""}
        ]
    }"#;

    #[test]
    fn keeps_only_real_unique_finds() {
        let found = parse_hunt(ANSWER).unwrap();
        assert_eq!(found.summary, "Знайшов піксельних ніндзя й тайли лісу");
        let titles: Vec<&str> = found.finds.iter().map(|find| find.title.as_str()).collect();
        assert_eq!(titles, ["Ninja Adventure", "Forest Tiles"]);
        assert_eq!(found.finds[1].price, "$9.99");
        assert_eq!(found.finds[1].site, "GameDev Market");
    }

    #[test]
    fn an_empty_hunt_is_an_error() {
        assert_eq!(
            parse_hunt(r#"{"summary": "", "finds": []}"#).err(),
            Some(StudioError::Claude(NOTHING_FOUND.into()))
        );
        assert!(parse_hunt("not json").is_err());
    }

    #[test]
    fn merges_fresh_finds_before_old_ones() {
        let first = parse_hunt(ANSWER).unwrap().finds;
        let fresh = vec![first[1].clone()];
        let kept = merged(fresh, first);
        let titles: Vec<&str> = kept.iter().map(|find| find.title.as_str()).collect();
        assert_eq!(titles, ["Forest Tiles", "Ninja Adventure"]);
    }

    #[test]
    fn writes_a_brief_grouped_by_kind() {
        let hunt = AssetHunt {
            project_id: 1,
            status: HuntStatus::Done,
            request: String::new(),
            summary: "Підсумок".into(),
            finds: parse_hunt(ANSWER).unwrap().finds,
            error: None,
            started_at: None,
            finished_at: None,
        };
        let brief = brief_markdown(&hunt).unwrap();
        assert!(brief.starts_with("# Знайдені ассети\n\nПідсумок\n\n## Персонажі\n\n"));
        assert!(brief.contains(
            "- [Ninja Adventure](https://pixel-boy.itch.io/ninja-adventure-asset-pack) — itch.io, автор Pixel-Boy. Ліцензія: CC0. Безкоштовно. Той самий вид зверху"
        ));
        assert!(brief.contains("## Тайли"));
        assert!(brief.contains("Платно · $9.99"));
        let empty = AssetHunt { finds: Vec::new(), ..hunt };
        assert!(brief_markdown(&empty).is_none());
    }

    #[test]
    fn schema_lists_every_kind_and_cost() {
        let schema = hunt_schema();
        assert_eq!(schema["properties"]["finds"]["items"]["properties"]["kind"]["enum"][0], "characters");
        assert_eq!(schema["properties"]["finds"]["items"]["properties"]["cost"]["enum"][2], "paid");
    }

    fn ninja_game() -> ProjectDetail {
        ProjectDetail {
            project: Project {
                id: 1,
                name: "Тінь Сакури".into(),
                prompt: "Піксельний 2D-платформер про ніндзя, що бігає по стінах бамбукового лісу й тушевих гір".into(),
                pitch: "Ніндзя рятує храм від вогняного демона".into(),
                engine: GameEngine::Godot,
                status: ProjectStatus::Ready,
                style: StyleGuide {
                    palette: vec!["#1b1b24".into(), "#e9e1cf".into(), "#c0392b".into()],
                    mood: "туш, туман, тиша перед ударом".into(),
                    ..StyleGuide::default()
                },
                references: Vec::new(),
                analysis: PromptAnalysis::default(),
                cover_screen_id: None,
                cover_html: None,
                screen_count: 1,
                approved_count: 1,
                pending_count: 0,
                error: None,
                created_at: 0,
            },
            design_document: "Вид збоку, піксель-арт 32×32, біг по стінах, сюрікени, бос-демон".into(),
            screens: vec![Screen {
                id: 4,
                project_id: 1,
                position: 0,
                title: "HUD".into(),
                summary: "Життя й сюрікени".into(),
                status: ScreenStatus::Approved,
                html: None,
                revision: 1,
                error: None,
            }],
            outputs: Vec::new(),
        }
    }

    #[test]
    fn describes_the_game_and_skips_known_pages() {
        let known = parse_hunt(ANSWER).unwrap().finds;
        let message = hunt_message(&ninja_game(), "тайли бамбука", &known.iter().collect::<Vec<_>>());
        assert!(message.starts_with("Знайди готові ассети для гри «Тінь Сакури»."));
        assert!(message.contains("Стиль: палітра #1b1b24, #e9e1cf, #c0392b"));
        assert!(message.contains("Гравець просить окремо знайти: тайли бамбука"));
        assert!(message.contains("- https://pixel-boy.itch.io/ninja-adventure-asset-pack\n"));
        let plain = hunt_message(&ninja_game(), "", &[]);
        assert!(!plain.contains("окремо"));
        assert!(!plain.contains("вже знайдено"));
    }

    #[test]
    #[ignore = "searches the web through the real Claude Code CLI and spends subscription usage"]
    fn live_hunt_finds_real_pages_with_previews() {
        let workdir = std::env::temp_dir().join("noctics-hunt-live");
        let claude = crate::claude_code::ClaudeCode::new(workdir.clone());
        let answer = claude
            .research(&Completion {
                system: SYSTEM_PROMPT.to_owned(),
                user: hunt_message(&ninja_game(), "", &[]),
                effort: Effort::Medium,
                schema: Some(hunt_schema()),
            })
            .unwrap();
        let _ = std::fs::remove_dir_all(&workdir);
        let mut found = parse_hunt(&answer).unwrap();
        asset_previews::attach(&mut found.finds);
        println!("{}", found.summary);
        for find in &found.finds {
            println!(
                "{} | {} | {} | {} | {} | {}",
                find.kind.label(),
                find.site,
                find.cost_label(),
                find.title,
                find.url,
                find.image.as_deref().unwrap_or("-")
            );
        }
        assert!(found.finds.len() >= 3, "{answer}");
        assert!(found.finds.iter().any(|find| find.site == "itch.io"));
        assert!(found.finds.iter().any(|find| find.image.is_some()));
    }
}
