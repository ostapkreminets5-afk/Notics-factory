use noctics_shared::{MotionClip, MotionOptions};
use serde_json::{Value, json};

pub const EXPORT_FORMAT: &str = "fbx7";
pub const FILE_EXTENSION: &str = "fbx";
const MOTION_TYPE: &str = "Motion";
const CHARACTER_TYPE: &str = "Character";
const DEFAULT_CHARACTER: &str = "y bot";
const KEYFRAME_REDUCTION: &str = "0";
const MAX_TEXT_CHARS: usize = 160;
const MAX_URL_CHARS: usize = 8192;
const MAX_SCHEME_CHARS: usize = 10;
const MIN_PREFIX_CHARS: usize = 3;
const EXACT_NAME_SCORE: i32 = 100;
const NAME_WORD_SCORE: i32 = 12;
const NAME_PREFIX_SCORE: i32 = 8;
const DESCRIPTION_WORD_SCORE: i32 = 5;
const DESCRIPTION_PREFIX_SCORE: i32 = 3;
const FULL_COVERAGE_SCORE: i32 = 20;
const EXTRA_WORD_PENALTY: i32 = 1;
const STOP_WORDS: [&str; 10] = ["a", "an", "the", "and", "with", "of", "to", "in", "on", "for"];

#[derive(Debug, Clone, PartialEq)]
pub struct Details {
    pub gms_hash: Value,
    pub supports_in_place: bool,
    pub clip: MotionClip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Character {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Monitor {
    Working,
    Ready(String),
    Unlinked(String),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMotion {
    pub name: &'static str,
    pub query: &'static str,
    pub in_place: bool,
}

struct Need {
    cues: &'static [&'static str],
    motions: &'static [PlannedMotion],
}

const fn moving(name: &'static str, query: &'static str) -> PlannedMotion {
    PlannedMotion {
        name,
        query,
        in_place: true,
    }
}

const fn acting(name: &'static str, query: &'static str) -> PlannedMotion {
    PlannedMotion {
        name,
        query,
        in_place: false,
    }
}

const BASE_SET: [PlannedMotion; 6] = [
    acting("idle", "idle"),
    moving("walk", "walking"),
    moving("run", "running"),
    acting("jump", "jump"),
    acting("hit", "hit reaction"),
    acting("death", "dying"),
];

const NEEDS: [Need; 9] = [
    Need {
        cues: &["sword", "blade", "katana", "knight", "меч", "клинок", "лицар", "фехт"],
        motions: &[acting("attack-sword", "sword slash"), acting("block", "sword block")],
    },
    Need {
        cues: &["gun", "rifle", "shoot", "pistol", "шутер", "стріл", "пістолет", "рушниц"],
        motions: &[acting("aim", "rifle aiming idle"), acting("shoot", "shooting"), moving("aim-walk", "rifle walk")],
    },
    Need {
        cues: &["magic", "spell", "mage", "wizard", "магі", "заклин", "чаклун", "чарів"],
        motions: &[acting("cast", "spell cast"), acting("cast-area", "magic area attack")],
    },
    Need {
        cues: &["punch", "fight", "brawl", "boxing", "бій", "бійк", "кулак", "удар"],
        motions: &[acting("punch", "punching"), acting("kick", "kick")],
    },
    Need {
        cues: &["stealth", "sneak", "thief", "стелс", "злод", "крад", "непомітн"],
        motions: &[moving("crouch-walk", "crouched walking"), acting("crouch-idle", "crouch idle")],
    },
    Need {
        cues: &["climb", "parkour", "ladder", "лазі", "паркур", "драбин", "видира"],
        motions: &[acting("climb", "climbing"), acting("vault", "vault")],
    },
    Need {
        cues: &["swim", "diving", "плава", "пірна"],
        motions: &[moving("swim", "swimming")],
    },
    Need {
        cues: &["zombie", "horror", "monster", "зомбі", "хорор", "монстр", "жах"],
        motions: &[moving("zombie-walk", "zombie walk"), acting("zombie-attack", "zombie attack")],
    },
    Need {
        cues: &["dance", "dancing", "танц"],
        motions: &[acting("dance", "dancing")],
    },
];

pub fn plan_for(text: &str) -> Vec<PlannedMotion> {
    let said = words(text);
    let mentions = |cue: &&str| said.iter().any(|word| word.starts_with(*cue));
    let mut planned = BASE_SET.to_vec();
    for need in NEEDS.iter().filter(|need| need.cues.iter().any(mentions)) {
        planned.extend(need.motions.iter().cloned());
    }
    planned
}

pub fn parse_products(body: &Value) -> Vec<MotionClip> {
    body["results"]
        .as_array()
        .map(|results| {
            results
                .iter()
                .filter(|item| item["type"].as_str() == Some(MOTION_TYPE))
                .filter_map(clip_from)
                .collect()
        })
        .unwrap_or_default()
}

pub fn rank(query: &str, clips: Vec<MotionClip>) -> Vec<MotionClip> {
    let wanted = words(query);
    let mut scored: Vec<(i32, MotionClip)> = clips
        .into_iter()
        .map(|clip| (score(&wanted, query, &clip), clip))
        .collect();
    scored.sort_by(|left, right| right.0.cmp(&left.0));
    scored.into_iter().map(|(_, clip)| clip).collect()
}

pub fn read_details(body: &Value) -> Option<Details> {
    let gms_hash = body["details"]["gms_hash"].clone();
    if !gms_hash.is_object() {
        return None;
    }
    let supports_in_place = body["details"]["supports_inplace"].as_bool().unwrap_or(false);
    let id = body["id"].as_str().map(str::trim).filter(|id| is_id(id)).unwrap_or_default();
    let clip = MotionClip {
        supports_in_place,
        ..clip_fields(body, id)
    };
    Some(Details {
        gms_hash,
        supports_in_place,
        clip,
    })
}

pub fn merge_clip(known: Option<&MotionClip>, fetched: &MotionClip, id: &str) -> MotionClip {
    let pick = |mine: &str, theirs: &str| if mine.is_empty() { theirs.to_owned() } else { mine.to_owned() };
    let base = known.cloned().unwrap_or_default();
    MotionClip {
        id: id.to_owned(),
        name: pick(&base.name, &fetched.name),
        description: pick(&base.description, &fetched.description),
        thumbnail: pick(&base.thumbnail, &fetched.thumbnail),
        animated: pick(&base.animated, &fetched.animated),
        supports_in_place: fetched.supports_in_place,
    }
}

pub fn export_body(character: &Character, clip: &MotionClip, details: &Details, options: &MotionOptions) -> Option<Value> {
    let mut hash = details.gms_hash.as_object()?.clone();
    let params = hash
        .get("params")
        .and_then(Value::as_array)
        .map(|params| params.iter().map(|param| param_value(&param[1])).collect::<Vec<_>>().join(","))
        .unwrap_or_default();
    hash.insert("params".into(), Value::String(params));
    hash.insert("mirror".into(), json!(options.mirror));
    hash.insert("trim".into(), json!([options.trim_start, options.trim_end]));
    if details.supports_in_place {
        hash.insert("inplace".into(), json!(options.in_place));
    }
    let product_name = if clip.name.is_empty() { clip.title() } else { clip.name.as_str() };
    Some(json!({
        "character_id": character.id,
        "gms_hash": [Value::Object(hash)],
        "preferences": {
            "format": EXPORT_FORMAT,
            "skin": if options.skin { "true" } else { "false" },
            "fps": options.fps.to_string(),
            "reducekf": KEYFRAME_REDUCTION,
        },
        "product_name": product_name,
        "type": MOTION_TYPE,
    }))
}

pub fn read_monitor(body: &Value) -> Monitor {
    match body["status"].as_str() {
        Some("completed") => match body["job_result"].as_str() {
            Some(link) => clean_url(link).map_or_else(|| Monitor::Unlinked(rejected_link(link)), Monitor::Ready),
            None => Monitor::Unlinked("Mixamo не дав посилання на файл".to_owned()),
        },
        Some("failed") => Monitor::Failed(
            body["message"]
                .as_str()
                .map(clip_text)
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| "Mixamo не зміг запекти анімацію".to_owned()),
        ),
        _ => Monitor::Working,
    }
}

pub fn read_primary(body: &Value) -> Option<Character> {
    let id = body["primary_character_id"].as_str().map(str::trim).filter(|id| is_id(id))?;
    let name = body["primary_character_name"].as_str().map(clip_text).unwrap_or_default();
    Some(Character {
        id: id.to_owned(),
        name,
    })
}

pub fn pick_character(body: &Value) -> Option<Character> {
    let characters: Vec<Character> = body["results"]
        .as_array()?
        .iter()
        .filter(|item| item["type"].as_str().is_none_or(|kind| kind == CHARACTER_TYPE))
        .filter_map(|item| {
            let id = item["id"].as_str().map(str::trim).filter(|id| is_id(id))?;
            Some(Character {
                id: id.to_owned(),
                name: item["name"].as_str().map(clip_text).unwrap_or_default(),
            })
        })
        .collect();
    characters
        .iter()
        .find(|character| character.name.to_lowercase() == DEFAULT_CHARACTER)
        .or_else(|| characters.first())
        .cloned()
}

pub fn is_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn rejected_link(link: &str) -> String {
    let link = link.trim();
    let scheme = link
        .split_once("://")
        .map(|(scheme, _)| scheme)
        .filter(|scheme| scheme.len() <= MAX_SCHEME_CHARS && scheme.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .unwrap_or("без протоколу");
    format!(
        "Mixamo дав посилання на файл, яке Noctics не прийняв ({scheme}, {} символів)",
        link.chars().count()
    )
}

pub fn clean_url(url: &str) -> Option<String> {
    let url = url.trim();
    let safe = url.starts_with("https://")
        && url.len() <= MAX_URL_CHARS
        && url
            .chars()
            .all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\'' | '\\' | '<' | '>' | '`'));
    safe.then(|| url.to_owned())
}

fn clip_from(item: &Value) -> Option<MotionClip> {
    let id = item["id"].as_str().map(str::trim).filter(|id| is_id(id))?;
    Some(clip_fields(item, id))
}

fn clip_fields(item: &Value, id: &str) -> MotionClip {
    let text = |key: &str| item[key].as_str().map(clip_text).unwrap_or_default();
    let link = |key: &str| item[key].as_str().and_then(clean_url).unwrap_or_default();
    MotionClip {
        id: id.to_owned(),
        name: text("name"),
        description: text("description"),
        thumbnail: link("thumbnail"),
        animated: link("thumbnail_animated"),
        supports_in_place: item["details"]["supports_inplace"].as_bool().unwrap_or(false),
    }
}

fn clip_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_TEXT_CHARS)
        .collect()
}

fn param_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty() && !STOP_WORDS.contains(word))
        .map(str::to_owned)
        .collect()
}

fn related(left: &str, right: &str) -> bool {
    left.chars().count() >= MIN_PREFIX_CHARS
        && right.chars().count() >= MIN_PREFIX_CHARS
        && (left.starts_with(right) || right.starts_with(left))
}

fn score(wanted: &[String], query: &str, clip: &MotionClip) -> i32 {
    if wanted.is_empty() {
        return 0;
    }
    let name = words(&clip.name);
    let description = words(&clip.description);
    let mut total = 0;
    let mut covered = 0;
    for word in wanted {
        let gained = if name.contains(word) {
            NAME_WORD_SCORE
        } else if name.iter().any(|candidate| related(candidate, word)) {
            NAME_PREFIX_SCORE
        } else if description.contains(word) {
            DESCRIPTION_WORD_SCORE
        } else if description.iter().any(|candidate| related(candidate, word)) {
            DESCRIPTION_PREFIX_SCORE
        } else {
            0
        };
        covered += usize::from(gained > 0);
        total += gained;
    }
    if words(query) == name || words(query) == description {
        total += EXACT_NAME_SCORE;
    }
    if covered == wanted.len() {
        total += FULL_COVERAGE_SCORE;
    }
    let extra = name.iter().filter(|candidate| !wanted.contains(candidate)).count();
    total - i32::try_from(extra).unwrap_or(i32::MAX) * EXTRA_WORD_PENALTY
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product(id: &str, kind: &str, name: &str, description: &str) -> Value {
        json!({
            "id": id,
            "type": kind,
            "name": name,
            "description": description,
            "thumbnail": format!("https://cdn.mixamo.com/{id}.png"),
            "thumbnail_animated": format!("https://cdn.mixamo.com/{id}.gif"),
        })
    }

    #[test]
    fn parses_only_single_motions() {
        let body = json!({
            "results": [
                product("a-1", "Motion", "Running", "Fast Run"),
                product("p-2", "MotionPack", "Locomotion Pack", ""),
                product("../x", "Motion", "Bad", ""),
                { "id": "b-3", "type": "Motion", "name": "Idle", "thumbnail": "javascript:alert(1)" },
            ],
            "pagination": { "num_pages": 3 }
        });
        let clips = parse_products(&body);
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].animated, "https://cdn.mixamo.com/a-1.gif");
        assert_eq!(clips[0].preview(), "https://cdn.mixamo.com/a-1.gif");
        assert_eq!(clips[1].thumbnail, "");
        assert!(parse_products(&json!({})).is_empty());
    }

    #[test]
    fn ranks_the_closest_animation_first() {
        let clip = |id: &str, name: &str, description: &str| MotionClip {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            ..MotionClip::default()
        };
        let ranked = rank(
            "sword slash",
            vec![
                clip("1", "Great Sword Idle", "Holding A Sword"),
                clip("2", "Stable Sword Outward Slash", "Slashing With Sword"),
                clip("3", "Sword And Shield Slash", "Slash"),
                clip("4", "Running", "Run"),
            ],
        );
        assert_eq!(ranked[0].id, "3");
        assert_eq!(ranked.last().unwrap().id, "4");
        let running = rank("run", vec![clip("w", "Walking", "Walk"), clip("r", "Running", "Running Forward")]);
        assert_eq!(running[0].id, "r");
        let exact = rank("idle", vec![clip("x", "Breathing Idle", ""), clip("y", "Idle", "")]);
        assert_eq!(exact[0].id, "y");
    }

    #[test]
    fn plans_animations_from_the_design_document() {
        let names = |text: &str| plan_for(text).into_iter().map(|motion| motion.name).collect::<Vec<_>>();
        let plain = names("Головоломка про світло");
        assert_eq!(plain, ["idle", "walk", "run", "jump", "hit", "death"]);
        let knight = names("Лицар з мечем бореться з зомбі");
        assert!(knight.contains(&"attack-sword") && knight.contains(&"zombie-walk"));
        assert!(!knight.contains(&"shoot"));
        assert!(plan_for("stealth").iter().any(|motion| motion.name == "crouch-walk" && motion.in_place));
    }

    #[test]
    fn builds_the_export_request_like_the_mixamo_site() {
        let details = read_details(&json!({
            "name": "Running",
            "description": "Fast Run",
            "details": {
                "supports_inplace": true,
                "gms_hash": {
                    "model-id": 105_220,
                    "mirror": false,
                    "trim": [0, 100],
                    "overdrive": 0,
                    "params": [["Speed", 0.5], ["Height", "1"]],
                    "arm-space": 0,
                    "inplace": false
                }
            }
        }))
        .unwrap();
        let character = Character {
            id: "c-1".into(),
            name: "Y Bot".into(),
        };
        let options = MotionOptions {
            in_place: true,
            mirror: true,
            trim_start: 10,
            trim_end: 90,
            ..MotionOptions::default()
        };
        let body = export_body(&character, &details.clip, &details, &options).unwrap();
        let hash = &body["gms_hash"][0];
        assert_eq!(hash["params"], "0.5,1");
        assert_eq!(hash["inplace"], true);
        assert_eq!(hash["mirror"], true);
        assert_eq!(hash["trim"], json!([10, 90]));
        assert_eq!(hash["model-id"], 105_220);
        assert_eq!(body["preferences"]["format"], EXPORT_FORMAT);
        assert_eq!(body["preferences"]["skin"], "false");
        assert_eq!(body["preferences"]["fps"], "30");
        assert_eq!(body["product_name"], "Running");
        assert_eq!(body["character_id"], "c-1");
        assert!(read_details(&json!({ "details": { "gms_hash": [] } })).is_none());
    }

    #[test]
    fn reads_export_progress() {
        assert_eq!(read_monitor(&json!({ "status": "processing" })), Monitor::Working);
        assert_eq!(
            read_monitor(&json!({ "status": "completed", "job_result": "https://s3.amazonaws.com/x.fbx?sig=1" })),
            Monitor::Ready("https://s3.amazonaws.com/x.fbx?sig=1".into())
        );
        let presigned = format!(
            "https://s3.amazonaws.com/mixamo-production/x.fbx?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Security-Token={}&X-Amz-Signature=9f2c",
            "IQoJb3JpZ2luX2VjE".repeat(110)
        );
        assert!(presigned.len() > 1800);
        assert_eq!(
            read_monitor(&json!({ "status": "completed", "job_result": presigned.clone() })),
            Monitor::Ready(presigned)
        );
        assert_eq!(
            read_monitor(&json!({ "status": "completed", "job_result": "ftp://x" })),
            Monitor::Unlinked("Mixamo дав посилання на файл, яке Noctics не прийняв (ftp, 7 символів)".into())
        );
        assert_eq!(
            read_monitor(&json!({ "status": "completed", "job_result": format!("https://x/{}", "a".repeat(9000)) })),
            Monitor::Unlinked("Mixamo дав посилання на файл, яке Noctics не прийняв (https, 9010 символів)".into())
        );
        assert_eq!(
            read_monitor(&json!({ "status": "completed", "job_result": "s3/x.fbx" })),
            Monitor::Unlinked("Mixamo дав посилання на файл, яке Noctics не прийняв (без протоколу, 8 символів)".into())
        );
        assert_eq!(
            read_monitor(&json!({ "status": "completed" })),
            Monitor::Unlinked("Mixamo не дав посилання на файл".into())
        );
        assert_eq!(
            read_monitor(&json!({ "status": "failed", "message": "  bad  rig " })),
            Monitor::Failed("bad rig".into())
        );
    }

    #[test]
    fn finds_the_character_to_animate() {
        assert_eq!(
            read_primary(&json!({ "primary_character_id": "abc-1", "primary_character_name": "Ninja" })),
            Some(Character {
                id: "abc-1".into(),
                name: "Ninja".into()
            })
        );
        assert_eq!(read_primary(&json!({ "primary_character_id": "" })), None);
        let list = json!({ "results": [
            { "id": "x-1", "type": "Character", "name": "Amy" },
            { "id": "y-2", "type": "Character", "name": "Y Bot" }
        ] });
        assert_eq!(pick_character(&list).unwrap().id, "y-2");
        let other = json!({ "results": [{ "id": "x-1", "type": "Character", "name": "Amy" }] });
        assert_eq!(pick_character(&other).unwrap().name, "Amy");
    }
}
