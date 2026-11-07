use noctics_shared::{
    GameEngine, MAX_ANALYSIS_KEYWORDS, MAX_PLANNED_SCREENS, MAX_PROJECT_NAME_CHARS, MAX_REFERENCES,
    MAX_SCREEN_SUMMARY_CHARS, MAX_SCREEN_TITLE_CHARS, MIN_REFERENCES, PALETTE_SIZE, Project, PromptAnalysis,
    REFERENCE_PALETTE_SIZE, Reference, ReferenceTopic, SCREEN_HEIGHT, SCREEN_WIDTH, SearchNeed, Skill,
    StudioError, StyleGuide, is_hex_color,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::studio_store::OutputJob;

const DEFAULT_PALETTE: [&str; PALETTE_SIZE] =
    ["#0b0e1a", "#3b7bff", "#a78bfa", "#3dd6d0", "#f2b444"];
const DEFAULT_FONTS: &str = "Unbounded / Manrope";
const MAX_FONTS_CHARS: usize = 60;
const MAX_MOOD_CHARS: usize = 40;
const MAX_PITCH_CHARS: usize = 200;
const MAX_CORNER_RADIUS: i64 = 32;
const MAX_TAKEN_STYLES: usize = 8;
const MAX_NOTE_CHARS: usize = 90;
const MAX_LOOK_CHARS: usize = 700;
const PALETTE_ROLES: [&str; PALETTE_SIZE] = ["фон", "поверхня панелей", "колір світу", "текст", "акцент головної дії"];
const MAX_REFERENCE_TITLE_CHARS: usize = 60;
const MAX_REFERENCE_NOTE_CHARS: usize = 160;
const MAX_REFERENCE_QUERY_CHARS: usize = 80;
const MAX_ANALYSIS_SUMMARY_CHARS: usize = 420;
const MAX_KEYWORD_CHARS: usize = 32;
const MAX_NEED_CHARS: usize = 160;
const TITLES_PER_SOURCE: usize = 3;
const THEME_WORDS: usize = 3;
const GENERIC_QUERY_WORDS: [&str; 12] = [
    "game", "games", "ui", "hud", "vfx", "concept", "art", "asset", "assets", "sprite", "3d", "2d",
];

pub struct Plan {
    pub name: String,
    pub pitch: String,
    pub design_document: String,
    pub style: StyleGuide,
    pub screens: Vec<PlannedScreen>,
    pub analysis: PromptAnalysis,
    pub references: Vec<Reference>,
}

pub struct ReferenceBoard {
    pub analysis: PromptAnalysis,
    pub references: Vec<Reference>,
}

pub struct ReferenceBrief<'a> {
    pub prompt: &'a str,
    pub game_name: &'a str,
    pub pitch: &'a str,
    pub design_document: &'a str,
    pub style: &'a StyleGuide,
}

pub struct PlannedScreen {
    pub title: String,
    pub summary: String,
}

pub struct ScreenBrief<'a> {
    pub game_name: &'a str,
    pub pitch: &'a str,
    pub design_document: &'a str,
    pub style: &'a StyleGuide,
    pub sibling_titles: &'a [String],
    pub title: &'a str,
    pub summary: &'a str,
    pub board: &'a [Reference],
    pub reference: Option<(&'a str, &'a str)>,
    pub revision: Option<(&'a str, &'a str)>,
}

#[derive(Deserialize)]
struct RawPlan {
    name: String,
    pitch: String,
    design_document: String,
    style: RawStyle,
    screens: Vec<RawScreen>,
    #[serde(default)]
    analysis: RawAnalysis,
    #[serde(default)]
    references: Vec<RawReference>,
}

#[derive(Deserialize)]
struct RawReferenceBoard {
    #[serde(default)]
    analysis: RawAnalysis,
    references: Vec<RawReference>,
}

#[derive(Deserialize, Default)]
struct RawAnalysis {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    needs: Vec<RawNeed>,
}

#[derive(Deserialize)]
struct RawNeed {
    topic: String,
    what: String,
}

#[derive(Deserialize)]
struct RawReference {
    topic: String,
    title: String,
    note: String,
    query: String,
    #[serde(default)]
    palette: Vec<String>,
}

#[derive(Deserialize)]
struct RawStyle {
    #[serde(default)]
    look: String,
    palette: Vec<RawColor>,
    fonts: String,
    corner_radius: i64,
    mood: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawColor {
    Described {
        hex: String,
        #[serde(default)]
        role: String,
        #[serde(default)]
        source: String,
    },
    Bare(String),
}

impl RawColor {
    fn hex(&self) -> String {
        match self {
            Self::Described { hex, .. } | Self::Bare(hex) => hex.trim().to_lowercase(),
        }
    }

    fn note(&self) -> String {
        let Self::Described { role, source, .. } = self else {
            return String::new();
        };
        let (role, source) = (clip(role, MAX_NOTE_CHARS), clip(source, MAX_NOTE_CHARS));
        let note = match (role.is_empty(), source.is_empty()) {
            (false, false) => format!("{role} — {source}"),
            (false, true) => role,
            _ => source,
        };
        clip(&note, MAX_NOTE_CHARS)
    }
}

#[derive(Deserialize)]
struct RawStyleAnswer {
    style: RawStyle,
}

pub struct StyleBrief<'a> {
    pub prompt: &'a str,
    pub game_name: &'a str,
    pub pitch: &'a str,
    pub design_document: &'a str,
    pub style: &'a StyleGuide,
    pub board: &'a [Reference],
}

#[derive(Deserialize)]
struct RawScreen {
    title: String,
    summary: String,
}

pub fn system_prompt(skills: &[Skill]) -> String {
    let catalog: String = skills
        .iter()
        .filter(|skill| skill.enabled)
        .map(|skill| {
            format!(
                "<skill name=\"{}\" stage=\"{}\">\n{}</skill>\n",
                skill.name,
                skill.stage_code(),
                skill.to_skill_markdown()
            )
        })
        .collect();
    format!(
        "Ти — AI-ядро Noctics Factory, студії, що перетворює ідею гравця на гру. \
Конвеєр має шість етапів: 01 Промпт, 02 Концепт, 03 Модель, 04 Матеріал, 05 Анімація, 06 Збірка.\n\n\
Нижче — навички студії. Кожна описує, як працювати на своєму етапі. \
Застосовуй ті, що стосуються поточного завдання, і дотримуйся їхніх правил.\n\n\
<skills>\n{catalog}</skills>\n\n\
Відповідай мовою промпту гравця."
    )
}

pub fn engine_brief(engine: GameEngine) -> String {
    format!(
        "Рушій гри: {} ({}). Код, структура проєкту, сцени, формати ассетів, шейдери й назви файлів — \
лише під цей рушій і його мову, без порад для інших рушіїв.",
        engine.label(),
        engine.stack()
    )
}

pub fn plan_message(prompt: &str, engine: GameEngine, earlier: &[Project]) -> String {
    format!(
        "Промпт гравця:\n<prompt>\n{prompt}\n</prompt>\n\n\
{}\n\n\
Сплануй гру для конвеєра:\n\
- name: робоча назва гри, 1–3 слова, до {MAX_PROJECT_NAME_CHARS} символів.\n\
- pitch: одне речення для картки проєкту.\n\
- design_document: короткий бриф гри в Markdown, 5–8 речень — жанр, світ, герой, основний цикл. Повний дизайн-документ, лор та інше окремо зроблять навички студії.\n\
- style: візуальна мова інтерфейсу гри — look, palette, fonts, corner_radius, mood за правилами стилю нижче.\n\
- screens: 4–6 ключових екранів інтерфейсу саме цієї гри (головне меню, HUD, вибір рівня, пауза, інвентар, фінальні титри — що їй потрібно). \
title — 1–3 слова; summary — що на екрані, до 80 символів.\n\
{}\n\n\
{}{}",
        engine_brief(engine),
        reference_rules(),
        style_rules(),
        taken_styles(earlier)
    )
}

fn style_rules() -> String {
    let roles = PALETTE_ROLES
        .iter()
        .map(|role| format!("«{role}»"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Правила стилю. Стиль виводь зі світу саме цієї гри, а не з загальних звичок. \
Спершу look — 2–4 речення про те, як цей світ і його інтерфейс виглядають у кадрі: час доби й світло, погода, \
головні матеріали й поверхні (з чого зроблені панелі, рамки й кнопки — папір, лакове дерево, ковані залізо, скло, тканина…), \
орнаменти й мотиви світу, кольори героя, ворогів, загрози й надії. \
Потім palette — рівно {PALETTE_SIZE} кольорів, кожен узятий з look: hex у форматі #rrggbb, role і source. Ролі по порядку: {roles}. \
source — конкретна річ цього світу, звідки колір («кленове листя на рисових терасах», «світло паперових ліхтарів»), \
а не загальне слово на кшталт «темний» чи «нейтральний». Насиченість і світлість — такі, як у самому світі: \
похмурий світ може бути приглушеним, святковий — соковитим. Текст має добре читатися на фоні й поверхні, акцент — виділятися на обох. \
fonts — два шрифти з Google Fonts у форматі «Display / Text», що пасують епосі, культурі й матеріалам світу; \
corner_radius — радіус кутів у px від 0 до {MAX_CORNER_RADIUS}, як у форм цього світу; mood — 2–4 слова про настрій і фактуру. \
Не бери гаму чи оформлення за замовчуванням — темний фон з неоновим акцентом, кремовий фон із засічками, \
сіро-бежеву гаму з одним червоним акцентом, моноширинні підписи, — якщо їх не дає сам світ гри."
    )
}

fn taken_styles(earlier: &[Project]) -> String {
    let taken: Vec<String> = earlier
        .iter()
        .filter(|project| !project.style.palette.is_empty())
        .take(MAX_TAKEN_STYLES)
        .map(|project| {
            format!(
                "- «{}»: {}; шрифти {}; настрій {}",
                project.name,
                project.style.palette.join(", "),
                project.style.fonts,
                project.style.mood
            )
        })
        .collect();
    if taken.is_empty() {
        return String::new();
    }
    format!(
        "\n\nІнші ігри студії вже мають такі стилі. Не переноси їх звідти за звичкою: кожен колір і шрифт має випливати \
з цього світу, а збіг допустимий лише тоді, коли його вимагає сам світ.\n{}",
        taken.join("\n")
    )
}

pub fn restyle_message(brief: &StyleBrief<'_>, earlier: &[Project]) -> String {
    let board = visual_references(brief.board);
    let references = if board.is_empty() {
        String::new()
    } else {
        format!(
            "Дошка референсів гри:\n<references>\n{}\n</references>\n\n",
            describe_references(&board)
        )
    };
    format!(
        "Промпт гравця:\n<prompt>\n{}\n</prompt>\n\n\
Гра: {} — {}\n\n<design_document>\n{}\n</design_document>\n\n{references}\
Поточний стиль цієї гри не випливає з її світу: палітра {}; шрифти {}; настрій {}. \
Виведи новий стиль саме з цього світу.\n\n{}{}",
        brief.prompt,
        brief.game_name,
        brief.pitch,
        brief.design_document,
        brief.style.palette.join(", "),
        brief.style.fonts,
        brief.style.mood,
        style_rules(),
        taken_styles(earlier)
    )
}

fn topic_codes() -> Vec<&'static str> {
    ReferenceTopic::ALL.iter().map(|topic| topic.code()).collect()
}

fn search_hint(topic: ReferenceTopic) -> Option<&'static str> {
    match topic {
        ReferenceTopic::World => Some("game environment concept art"),
        ReferenceTopic::Characters => Some("game character concept art"),
        ReferenceTopic::Characters3d => Some("stylized 3d game character render"),
        ReferenceTopic::Light => Some("game lighting mood concept art"),
        ReferenceTopic::Vfx => Some("game vfx effect"),
        ReferenceTopic::VfxSprites => Some("vfx sprite sheet animation"),
        ReferenceTopic::Interface => Some("game ui hud design"),
        ReferenceTopic::MainMenu => Some("game main menu screen ui"),
        ReferenceTopic::Panels => Some("game ui panel frame"),
        ReferenceTopic::Buttons => Some("game ui buttons"),
        ReferenceTopic::Icons => Some("game icons set"),
        ReferenceTopic::Materials => Some("stylized texture material"),
        ReferenceTopic::Props => Some("game props concept art"),
        ReferenceTopic::Animation => Some("sprite animation frames"),
        ReferenceTopic::Sounds | ReferenceTopic::Music => None,
    }
}

fn fallback_need(topic: ReferenceTopic) -> &'static str {
    match topic {
        ReferenceTopic::World => "Світ гри: локації, архітектура, природа й масштаб",
        ReferenceTopic::Characters => "Герої й вороги: силует, пропорції, одяг і читабельність у кадрі",
        ReferenceTopic::Characters3d => "Візуальний стиль 3D-персонажів: стилізація, пропорції, матеріали й рендер",
        ReferenceTopic::Light => "Світло й настрій сцен: час доби, контраст, кольорові акценти",
        ReferenceTopic::Vfx => "Ефекти ударів, магії, частинок і вибухів",
        ReferenceTopic::VfxSprites => "Покадрові спрайт-шити VFX: вибухи, удари, магія, дим",
        ReferenceTopic::Interface => "HUD і загальна мова інтерфейсу гри",
        ReferenceTopic::MainMenu => "Головне меню: композиція, фон, логотип і пункти меню",
        ReferenceTopic::Panels => "Панелі й рамки вікон, інвентаря та діалогів",
        ReferenceTopic::Buttons => "Кнопки: форма, стани, обведення й фактура",
        ReferenceTopic::Icons => "Іконки предметів, умінь і ресурсів",
        ReferenceTopic::Materials => "Матеріали й фактури поверхонь",
        ReferenceTopic::Props => "Предмети, зброя й деталі оточення",
        ReferenceTopic::Animation => "Анімація персонажів: пози, рух, ключові кадри",
        ReferenceTopic::Sounds => "Звуки ударів, кроків, інтерфейсу й оточення",
        ReferenceTopic::Music => "Музика гри: жанр, темп і настрій",
    }
}

fn query_hints() -> String {
    ReferenceTopic::ALL
        .into_iter()
        .filter_map(|topic| search_hint(topic).map(|hint| format!("{} — «{hint}»", topic.code())))
        .collect::<Vec<_>>()
        .join(", ")
}

fn reference_rules() -> String {
    let topics = topic_codes().join(", ");
    let always: String = ReferenceTopic::ALWAYS_SEARCHED
        .iter()
        .map(|topic| topic.code())
        .collect::<Vec<_>>()
        .join(", ");
    let hints = query_hints();
    format!(
        "- analysis: спершу розбери промпт гравця, щоб знати, що шукати в Pinterest і на ArtStation. \
summary — 2–3 речення до 300 символів: жанр, світ, камера, художній стиль і настрій, які випливають із промпту; \
keywords — 5–{MAX_ANALYSIS_KEYWORDS} коротких візуальних ключових слів із промпту, по 1–3 слова; \
needs — що саме треба знайти для цієї гри, по одному пункту на тему: topic — одне з: {topics}; \
what — що конкретно шукати й навіщо грі, до 120 символів. \
Теми {always} шукаються для кожної гри обов’язково: персонажі й вороги, візуальний стиль 3D-персонажів, інтерфейс і HUD, \
головне меню, панелі й рамки, кнопки, VFX-ефекти (удари, магія, частинки, вибухи) і VFX-спрайти — покадрові спрайт-шити анімацій ефектів. \
По кожній із них дай пункт needs і щонайменше один референс. Також продумай світ, світло, іконки, матеріали, предмети, анімацію, \
звуки й музику — пропусти таку тему лише тоді, коли в цій грі її справді немає.\n\
- references: {MIN_REFERENCES}–{MAX_REFERENCES} референсів за пунктами needs — щонайменше один на кожен пункт. \
topic — одне з: {topics}; title — що на референсі, 2–6 слів; note — що саме взяти з нього для гри, до 120 символів; \
query — для візуальних тем це пошуковий запит для Pinterest і ArtStation англійською, 4–9 слів: суть саме цієї гри плюс ігрове уточнення теми, \
щоб пошук віддав саме ігрові елементи, а не випадкові картинки: {hints}; \
для sounds і music query — це теги звучання англійською для генерації в ComfyUI: жанр, інструменти, темп, настрій, фактура звуку, \
напр. «dark ambient, low drones, slow 70 bpm» чи «short crunchy sword hit, metallic, dry»; \
palette — {REFERENCE_PALETTE_SIZE} кольори #rrggbb, що домінують на таких референсах, для sounds і music — порожній масив. \
У запитах не згадуй чужі ігри, фільми, бренди чи імена художників і композиторів — описуй те, що видно й чути: форму, світло, фактуру, епоху, звучання."
    )
}

pub fn references_message(brief: &ReferenceBrief<'_>) -> String {
    let style = brief.style;
    format!(
        "Промпт гравця:\n<prompt>\n{}\n</prompt>\n\n\
Гра: {} — {}\n\n<design_document>\n{}\n</design_document>\n\n\
Стиль інтерфейсу:\n- Палітра: {}\n- Шрифти: {}\n- Настрій: {}\n\n\
Розбери промпт і підбери дошку референсів для цієї гри:\n{}",
        brief.prompt,
        brief.game_name,
        brief.pitch,
        brief.design_document,
        style.palette.join(", "),
        style.fonts,
        style.mood,
        reference_rules()
    )
}

fn analysis_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "summary": { "type": "string" },
            "keywords": { "type": "array", "items": { "type": "string" } },
            "needs": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "topic": { "type": "string", "enum": topic_codes() },
                        "what": { "type": "string" }
                    },
                    "required": ["topic", "what"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["summary", "keywords", "needs"],
        "additionalProperties": false
    })
}

fn reference_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "topic": { "type": "string", "enum": topic_codes() },
                "title": { "type": "string" },
                "note": { "type": "string" },
                "query": { "type": "string" },
                "palette": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["topic", "title", "note", "query", "palette"],
            "additionalProperties": false
        }
    })
}

pub fn references_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "analysis": analysis_schema(),
            "references": reference_schema()
        },
        "required": ["analysis", "references"],
        "additionalProperties": false
    })
}

pub fn parse_references(text: &str) -> Result<ReferenceBoard, StudioError> {
    let board: RawReferenceBoard = serde_json::from_str(text.trim()).map_err(|error| {
        StudioError::Claude(format!("the reference board was not valid JSON: {error}"))
    })?;
    let mut references = clean_references(&board.references);
    if references.is_empty() {
        return Err(StudioError::Claude("the reference board was empty".into()));
    }
    let mut analysis = clean_analysis(&board.analysis);
    complete_board(&mut analysis, &mut references);
    Ok(ReferenceBoard { analysis, references })
}

fn complete_board(analysis: &mut PromptAnalysis, references: &mut Vec<Reference>) {
    if references.is_empty() {
        return;
    }
    let theme = board_theme(references);
    for topic in ReferenceTopic::ALWAYS_SEARCHED {
        if analysis.need_for(topic).is_none() {
            analysis.needs.push(SearchNeed {
                topic,
                what: fallback_need(topic).to_owned(),
            });
        }
        if !references.iter().any(|reference| reference.topic == topic) {
            references.push(fallback_reference(topic, &theme));
        }
    }
    while references.len() > MAX_REFERENCES {
        let Some(spare) = spare_reference(references) else {
            break;
        };
        references.remove(spare);
    }
}

fn board_theme(references: &[Reference]) -> String {
    let visual = || references.iter().filter(|reference| !reference.topic.is_audio());
    let Some(source) = visual()
        .find(|reference| reference.topic == ReferenceTopic::World)
        .or_else(|| visual().next())
    else {
        return String::new();
    };
    source
        .query
        .split_whitespace()
        .filter(|word| !GENERIC_QUERY_WORDS.contains(&word.to_lowercase().as_str()))
        .take(THEME_WORDS)
        .collect::<Vec<_>>()
        .join(" ")
}

fn fallback_reference(topic: ReferenceTopic, theme: &str) -> Reference {
    let hint = search_hint(topic).unwrap_or_default();
    Reference {
        topic,
        title: topic.label().to_owned(),
        note: fallback_need(topic).to_owned(),
        query: clip(&format!("{theme} {hint}"), MAX_REFERENCE_QUERY_CHARS),
        palette: Vec::new(),
        pins: Vec::new(),
        artworks: Vec::new(),
    }
}

fn spare_reference(references: &[Reference]) -> Option<usize> {
    let repeated = |topic: ReferenceTopic| {
        references
            .iter()
            .filter(|reference| reference.topic == topic)
            .count()
            > 1
    };
    let newest_first = || (0..references.len()).rev();
    newest_first()
        .find(|&index| repeated(references[index].topic))
        .or_else(|| {
            newest_first()
                .find(|&index| !ReferenceTopic::ALWAYS_SEARCHED.contains(&references[index].topic))
        })
}

fn clean_analysis(raw: &RawAnalysis) -> PromptAnalysis {
    let mut keywords: Vec<String> = Vec::new();
    for keyword in &raw.keywords {
        let keyword = clip(keyword, MAX_KEYWORD_CHARS);
        let known = keywords
            .iter()
            .any(|seen| seen.to_lowercase() == keyword.to_lowercase());
        if !keyword.is_empty() && !known {
            keywords.push(keyword);
        }
    }
    keywords.truncate(MAX_ANALYSIS_KEYWORDS);
    let mut needs: Vec<SearchNeed> = Vec::new();
    for need in &raw.needs {
        let Some(topic) = ReferenceTopic::from_code(need.topic.trim()) else {
            continue;
        };
        let what = clip(&need.what, MAX_NEED_CHARS);
        if !what.is_empty() && !needs.iter().any(|seen| seen.topic == topic) {
            needs.push(SearchNeed { topic, what });
        }
    }
    PromptAnalysis {
        summary: clip(&raw.summary, MAX_ANALYSIS_SUMMARY_CHARS),
        keywords,
        needs,
    }
}

fn clean_references(raw: &[RawReference]) -> Vec<Reference> {
    raw.iter()
        .filter_map(|reference| {
            let title = clip(&reference.title, MAX_REFERENCE_TITLE_CHARS);
            let query = clip(&reference.query, MAX_REFERENCE_QUERY_CHARS);
            if title.is_empty() || query.is_empty() {
                return None;
            }
            Some(Reference {
                topic: ReferenceTopic::from_code(reference.topic.trim())
                    .unwrap_or(ReferenceTopic::World),
                title,
                note: clip(&reference.note, MAX_REFERENCE_NOTE_CHARS),
                query,
                palette: reference
                    .palette
                    .iter()
                    .map(|color| color.trim().to_lowercase())
                    .filter(|color| is_hex_color(color))
                    .take(REFERENCE_PALETTE_SIZE)
                    .collect(),
                pins: Vec::new(),
                artworks: Vec::new(),
            })
        })
        .take(MAX_REFERENCES)
        .collect()
}

pub fn describe_references(references: &[Reference]) -> String {
    references
        .iter()
        .map(describe_reference)
        .collect::<Vec<_>>()
        .join("\n")
}

fn describe_reference(reference: &Reference) -> String {
    let topic = reference.topic.label();
    if reference.topic.is_audio() {
        return format!(
            "- [{topic}] {} — {} (звучання: «{}»)",
            reference.title, reference.note, reference.query
        );
    }
    let pins = sample_titles(reference.pins.iter().map(|pin| pin.title.as_str()));
    let artworks = sample_titles(reference.artworks.iter().map(|artwork| artwork.title.as_str()));
    let mut seen = String::new();
    if !pins.is_empty() {
        seen.push_str(&format!("; на пінах: {pins}"));
    }
    if !artworks.is_empty() {
        seen.push_str(&format!("; на ArtStation: {artworks}"));
    }
    format!(
        "- [{topic}] {} — {} (пошук: «{}»; кольори: {}{seen})",
        reference.title,
        reference.note,
        reference.query,
        reference.palette.join(", ")
    )
}

fn sample_titles<'a>(titles: impl Iterator<Item = &'a str>) -> String {
    titles
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .take(TITLES_PER_SOURCE)
        .collect::<Vec<_>>()
        .join(" · ")
}

pub fn visual_references(references: &[Reference]) -> Vec<Reference> {
    references
        .iter()
        .filter(|reference| !reference.topic.is_audio())
        .cloned()
        .collect()
}

pub fn plan_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "pitch": { "type": "string" },
            "design_document": { "type": "string" },
            "style": style_object_schema(),
            "screens": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "summary": { "type": "string" }
                    },
                    "required": ["title", "summary"],
                    "additionalProperties": false
                }
            },
            "analysis": analysis_schema(),
            "references": reference_schema()
        },
        "required": ["name", "pitch", "design_document", "style", "screens", "analysis", "references"],
        "additionalProperties": false
    })
}

fn style_object_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "look": { "type": "string" },
            "palette": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "hex": { "type": "string" },
                        "role": { "type": "string" },
                        "source": { "type": "string" }
                    },
                    "required": ["hex", "role", "source"],
                    "additionalProperties": false
                }
            },
            "fonts": { "type": "string" },
            "corner_radius": { "type": "integer" },
            "mood": { "type": "string" }
        },
        "required": ["look", "palette", "fonts", "corner_radius", "mood"],
        "additionalProperties": false
    })
}

pub fn style_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "style": style_object_schema() },
        "required": ["style"],
        "additionalProperties": false
    })
}

pub fn parse_style(text: &str) -> Result<StyleGuide, StudioError> {
    let raw: RawStyleAnswer = serde_json::from_str(text.trim()).map_err(|error| {
        StudioError::Claude(format!("the game style was not valid JSON: {error}"))
    })?;
    Ok(clean_style(&raw.style))
}

fn clean_style(raw: &RawStyle) -> StyleGuide {
    let chosen: Vec<(String, String)> = raw
        .palette
        .iter()
        .map(|color| (color.hex(), color.note()))
        .filter(|(hex, _)| is_hex_color(hex))
        .take(PALETTE_SIZE)
        .collect();
    let mut palette: Vec<String> = chosen.iter().map(|(hex, _)| hex.clone()).collect();
    let mut palette_notes: Vec<String> = chosen.into_iter().map(|(_, note)| note).collect();
    for color in DEFAULT_PALETTE.iter().skip(palette.len()) {
        palette.push((*color).to_owned());
        palette_notes.push(String::new());
    }
    let fonts = clip(&raw.fonts, MAX_FONTS_CHARS);
    StyleGuide {
        palette,
        palette_notes,
        fonts: if fonts.is_empty() {
            DEFAULT_FONTS.to_owned()
        } else {
            fonts
        },
        corner_radius: u8::try_from(raw.corner_radius.clamp(0, MAX_CORNER_RADIUS)).unwrap_or_default(),
        mood: clip(&raw.mood, MAX_MOOD_CHARS),
        look: clip(&raw.look, MAX_LOOK_CHARS),
    }
}

pub fn parse_plan(text: &str) -> Result<Plan, StudioError> {
    let raw: RawPlan = serde_json::from_str(text.trim()).map_err(|error| {
        StudioError::Claude(format!("the game plan was not valid JSON: {error}"))
    })?;
    let screens: Vec<PlannedScreen> = raw
        .screens
        .iter()
        .filter(|screen| !screen.title.trim().is_empty())
        .take(MAX_PLANNED_SCREENS)
        .map(|screen| PlannedScreen {
            title: clip(&screen.title, MAX_SCREEN_TITLE_CHARS),
            summary: clip(&screen.summary, MAX_SCREEN_SUMMARY_CHARS),
        })
        .collect();
    if screens.is_empty() {
        return Err(StudioError::Claude("the game plan had no screens".into()));
    }
    let name = clip(&raw.name, MAX_PROJECT_NAME_CHARS);
    let mut analysis = clean_analysis(&raw.analysis);
    let mut references = clean_references(&raw.references);
    complete_board(&mut analysis, &mut references);
    Ok(Plan {
        name: if name.is_empty() {
            "Нова гра".to_owned()
        } else {
            name
        },
        pitch: clip(&raw.pitch, MAX_PITCH_CHARS),
        design_document: raw.design_document.trim().to_owned(),
        style: clean_style(&raw.style),
        screens,
        analysis,
        references,
    })
}

fn style_lines(style: &StyleGuide) -> String {
    let mut lines = format!(
        "- Палітра: {}\n- Шрифти: {}\n- Радіус кутів: {} px\n- Настрій: {}\n",
        style.palette_lines(),
        style.fonts,
        style.corner_radius,
        style.mood
    );
    if !style.look.trim().is_empty() {
        lines.push_str(&format!("- Вигляд світу: {}\n", style.look.trim()));
    }
    lines
}

pub fn screen_message(brief: &ScreenBrief<'_>) -> String {
    let style = brief.style;
    let mut message = format!(
        "Гра: {} — {}\n\n<design_document>\n{}\n</design_document>\n\n\
Стиль інтерфейсу:\n{}\n\
Екрани гри: {}.\n\n",
        brief.game_name,
        brief.pitch,
        brief.design_document,
        style_lines(style),
        brief.sibling_titles.join(", "),
    );
    let board = visual_references(brief.board);
    if !board.is_empty() {
        message.push_str(&format!(
            "Дошка референсів гри з Pinterest і ArtStation — бери з неї форму панелей, рамок і кнопок, іконки, світло, фактуру й кольори; \
промпти слотів арту в data-prompt пиши в стилі референсів світу й персонажів:\n<references>\n{}\n</references>\n\n",
            describe_references(&board)
        ));
    }
    if let Some((title, html)) = brief.reference {
        message.push_str(&format!(
            "Уже готовий екран цієї гри — тримай ту саму мову оформлення:\n<reference_screen title=\"{title}\">\n{html}\n</reference_screen>\n\n"
        ));
    }
    match brief.revision {
        Some((current, feedback)) => message.push_str(&format!(
            "Екран «{}»: {}.\n\nПоточна версія:\n<current_screen>\n{current}\n</current_screen>\n\n\
Правки від гравця:\n<feedback>\n{feedback}\n</feedback>\n\n\
Перероби екран з урахуванням правок. Те, чого правки не стосуються, залиш як є.\n\n",
            brief.title, brief.summary
        )),
        None => message.push_str(&format!(
            "Створи екран «{}»: {}.\n\n",
            brief.title, brief.summary
        )),
    }
    message.push_str(&format!(
        "Вимоги до результату:\n\
- Один повний HTML-документ, що починається з <!DOCTYPE html>.\n\
- Екран рівно {SCREEN_WIDTH}×{SCREEN_HEIGHT} px: html і body без відступів, body має width:{SCREEN_WIDTH}px; height:{SCREEN_HEIGHT}px; overflow:hidden.\n\
- Увесь CSS усередині <style>. Без JavaScript.\n\
- Без зовнішніх зображень. Іконки інтерфейсу, рамки й роздільники — inline SVG або CSS; атмосферу фону — світло, туман, глибину — CSS-градієнтами.\n\
- Оформлення — матеріал панелей, форма рамок і кнопок, орнаменти, світло й фон — бери з вигляду світу цієї гри й дошки референсів. \
Кожен колір палітри використовуй у його ролі. Не збирай екран із загального набору темної гри — місяць, туман, жарини, віньєтка, \
зерно плівки, рядок підказок клавіш унизу, — якщо цього не просить сам світ і жанр.\n\
- Персонажів, істот, обличчя, предмети, локації та будь-які ілюстрації ніколи не малюй SVG, CSS-фігурами чи іншими примітивами — жодних кіл, еліпсів, трикутників і паличок замість арту. На їхньому місці постав слот арту: <figure class=\"art-slot\" data-art=\"art/<тека>/<назва>.png\" data-prompt=\"<промпт англійською для ComfyUI>\"> потрібного розміру, з тонкою рамкою в стилі екрана й підписом шляху файлу всередині. Слот заповнить арт, згенерований у ComfyUI на етапі збірки.\n\
- Шрифти — лише через <link> з fonts.googleapis.com.\n\
- Тексти інтерфейсу — мовою гри, з назвами й числами з дизайн-документа, без lorem ipsum.\n\
- Поверни лише HTML-документ, без пояснень і без Markdown-огорож."
    ));
    message
}

pub fn deliverable_message(job: &OutputJob) -> String {
    let style = &job.style;
    let mut message = format!(
        "Промпт гравця:\n<prompt>\n{}\n</prompt>\n\n\
{}\n\n\
Гра: {} — {}\n\n<brief>\n{}\n</brief>\n\n\
Стиль інтерфейсу:\n{}\n\
Екрани гри: {}.\n\n",
        job.prompt,
        engine_brief(job.engine),
        job.game_name,
        job.pitch,
        job.brief,
        style_lines(style),
        job.screen_titles.join(", "),
    );
    if !job.references.is_empty() {
        message.push_str(&format!(
            "Дошка референсів Pinterest і ArtStation, яку підібрало AI-ядро:\n<references>\n{}\n</references>\n\n",
            describe_references(&job.references)
        ));
    }
    for (title, content) in &job.previous {
        message.push_str(&format!(
            "Результат навички попереднього етапу:\n<previous_result skill=\"{title}\">\n{content}\n</previous_result>\n\n"
        ));
    }
    message.push_str(&format!(
        "Виконай навичку «{}» для цієї гри — суворо за її інструкціями з системного промпту, \
спираючись на результати попередніх етапів. Поверни лише результат у Markdown, без вступу й пояснень.",
        job.title
    ));
    message
}

pub fn extract_html(text: &str) -> Option<String> {    let lower = text.to_ascii_lowercase();
    let start = lower
        .find("<!doctype html")
        .or_else(|| lower.find("<html"))?;
    let end = lower
        .rfind("</html>")
        .map_or(text.len(), |at| at + "</html>".len());
    (end > start).then(|| text[start..end].trim().to_owned())
}

fn clip(text: &str, limit: usize) -> String {
    let single: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    single
        .chars()
        .take(limit)
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use noctics_shared::{Artwork, Pin, SkillCategory};

    use super::*;

    fn skill(name: &str, enabled: bool) -> Skill {
        Skill {
            id: 1,
            name: name.into(),
            stage: 1,
            title: "Лор".into(),
            summary: "Імена".into(),
            category: SkillCategory::GameDesign,
            instructions: "- Коротко.".into(),
            enabled,
        }
    }

    #[test]
    fn deliverable_brief_carries_previous_results_and_the_task() {
        let job = OutputJob {
            output_id: 1,
            project_id: 1,
            skill_name: "game-scripts".into(),
            title: "Скрипти гри".into(),
            prompt: "Стелс про злодія".into(),
            engine: GameEngine::Unreal,
            game_name: "Нічний злодій".into(),
            pitch: "Стелс у неоновому місті".into(),
            brief: "Бриф".into(),
            style: StyleGuide::default(),
            screen_titles: vec!["Головне меню".into(), "HUD".into()],
            previous: vec![("Специфікація моделей".into(), "| asset |".into())],
            references: vec![Reference {
                topic: ReferenceTopic::Light,
                title: "Мокрі дахи вночі".into(),
                note: "Відблиски ліхтарів у калюжах".into(),
                query: "rainy rooftops night lanterns".into(),
                palette: vec!["#0b0e1a".into(), "#f2b444".into()],
                pins: Vec::new(),
                artworks: vec![
                    Artwork::checked(
                        "2B1Xae",
                        "https://cdna.artstation.com/p/assets/images/images/085/683/878/smaller_square/roofs.jpg",
                        "Rainy roofs",
                        "",
                    )
                    .unwrap(),
                ],
            }],
        };
        let message = deliverable_message(&job);
        assert!(message.contains("<prompt>\nСтелс про злодія\n</prompt>"));
        assert!(message.contains("Рушій гри: Unreal Engine (C++ / Blueprints)."));
        assert!(message.contains(
            "- [Світло й настрій] Мокрі дахи вночі — Відблиски ліхтарів у калюжах (пошук: «rainy rooftops night lanterns»; кольори: #0b0e1a, #f2b444; на ArtStation: Rainy roofs)"
        ));
        assert!(message.contains("<previous_result skill=\"Специфікація моделей\">\n| asset |"));
        assert!(message.contains("Екрани гри: Головне меню, HUD."));
        assert!(message.contains("Виконай навичку «Скрипти гри»"));
    }

    #[test]
    fn plan_brief_names_the_engine() {
        let message = plan_message("Гонки на човнах", GameEngine::Penumbra, &[]);
        assert!(message.contains("<prompt>\nГонки на човнах\n</prompt>"));
        assert!(message.contains("Рушій гри: Penumbra (Rust · wgpu)."));
        assert!(message.contains("Спершу look"));
        assert!(message.contains("«фон», «поверхня панелей», «колір світу», «текст», «акцент головної дії»"));
        assert!(!message.contains("Інші ігри студії"));
    }

    #[test]
    fn reads_each_color_with_the_reason_it_was_chosen() {
        let plan = parse_plan(
            r##"{
                "name": "Шлях Тіні",
                "pitch": "Ніндзя в осінніх сутінках.",
                "design_document": "# GDD",
                "style": {
                    "look": "Осінні сутінки над рисовими терасами, паперові ліхтарі, лакове дерево.",
                    "palette": [
                        {"hex": "#1A1C2B", "role": "фон", "source": "сутінкове небо над горами"},
                        {"hex": "#3b2a22", "role": "поверхня панелей", "source": "лакове дерево замку"},
                        {"hex": "nope", "role": "колір світу", "source": "кленове листя"},
                        {"hex": "#e0782e", "role": "колір світу", "source": "кленове листя на терасах"},
                        {"hex": "#f3e3c3", "role": "текст", "source": ""},
                        {"hex": "#c8322a", "role": "", "source": "печатка клану Ворона"}
                    ],
                    "fonts": "Yuji Syuku / Noto Sans JP",
                    "corner_radius": 2,
                    "mood": "тушевий, осінній"
                },
                "screens": [{"title": "Головне меню", "summary": "Старт"}]
            }"##,
        )
        .unwrap();
        let style = plan.style;
        assert_eq!(style.palette, ["#1a1c2b", "#3b2a22", "#e0782e", "#f3e3c3", "#c8322a"]);
        assert_eq!(style.palette_note(0), Some("фон — сутінкове небо над горами"));
        assert_eq!(style.palette_note(2), Some("колір світу — кленове листя на терасах"));
        assert_eq!(style.palette_note(3), Some("текст"));
        assert_eq!(style.palette_note(4), Some("печатка клану Ворона"));
        assert!(style.look.starts_with("Осінні сутінки"));
        let restyled = parse_style(
            r##"{"style": {"look": "Туман", "palette": [{"hex": "#101010", "role": "фон", "source": "ніч"}], "fonts": "", "corner_radius": 8, "mood": "тиша"}}"##,
        )
        .unwrap();
        assert_eq!(restyled.palette.len(), PALETTE_SIZE);
        assert_eq!(restyled.palette_notes.len(), PALETTE_SIZE);
        assert_eq!(restyled.palette_note(1), None);
        assert_eq!(restyled.fonts, DEFAULT_FONTS);
    }

    #[test]
    fn screens_follow_the_look_of_their_own_world() {
        let style = StyleGuide {
            palette: vec!["#1a1c2b".into(), "#e0782e".into()],
            palette_notes: vec!["фон — сутінкове небо".into(), "колір світу — кленове листя".into()],
            fonts: "Yuji Syuku / Noto Sans JP".into(),
            corner_radius: 2,
            mood: "тушевий".into(),
            look: "Паперові ліхтарі й лакове дерево.".into(),
        };
        let titles = vec!["Головне меню".to_owned()];
        let message = screen_message(&ScreenBrief {
            game_name: "Шлях Тіні",
            pitch: "Ніндзя",
            design_document: "# GDD",
            style: &style,
            sibling_titles: &titles,
            title: "Головне меню",
            summary: "Старт",
            board: &[],
            reference: None,
            revision: None,
        });
        assert!(message.contains("- Палітра: #1a1c2b (фон — сутінкове небо); #e0782e (колір світу — кленове листя)"));
        assert!(message.contains("- Вигляд світу: Паперові ліхтарі й лакове дерево."));
        assert!(message.contains("Не збирай екран із загального набору темної гри"));
    }

    #[test]
    fn plan_brief_keeps_new_games_off_earlier_palettes() {
        let earlier = Project {
            id: 1,
            name: "Шлях Тіні".into(),
            prompt: "Ніндзя".into(),
            pitch: String::new(),
            engine: GameEngine::Godot,
            status: noctics_shared::ProjectStatus::Ready,
            style: StyleGuide {
                palette: vec!["#1b1d26".into(), "#d6402a".into()],
                fonts: "Rubik Dirt / Exo 2".into(),
                corner_radius: 2,
                mood: String::new(),
                ..StyleGuide::default()
            },
            references: Vec::new(),
            analysis: PromptAnalysis::default(),
            cover_screen_id: None,
            cover_html: None,
            screen_count: 0,
            approved_count: 0,
            pending_count: 0,
            error: None,
            created_at: 0,
        };
        let unplanned = Project {
            id: 2,
            name: "Нова гра".into(),
            style: StyleGuide::default(),
            ..earlier.clone()
        };
        let message = plan_message("Ще одна гра про ніндзя", GameEngine::Godot, &[unplanned, earlier.clone()]);
        assert!(message.contains("Не переноси їх звідти за звичкою"));
        assert!(message.contains("- «Шлях Тіні»: #1b1d26, #d6402a; шрифти Rubik Dirt / Exo 2"));
        assert!(!message.contains("«Нова гра»"));
        let board = Vec::new();
        let restyle = restyle_message(
            &StyleBrief {
                prompt: "Ніндзя",
                game_name: "Шлях Тіні",
                pitch: "Тінь у горах",
                design_document: "# GDD",
                style: &earlier.style,
                board: &board,
            },
            &[],
        );
        assert!(restyle.contains("Поточний стиль цієї гри не випливає з її світу: палітра #1b1d26, #d6402a"));
        assert!(restyle.contains("Спершу look"));
    }

    #[test]
    fn system_prompt_carries_only_enabled_skills() {
        let prompt = system_prompt(&[
            skill("lore-and-naming", true),
            skill("material-style", false),
        ]);
        assert!(prompt.contains("<skill name=\"lore-and-naming\" stage=\"01\">"));
        assert!(prompt.contains("name: lore-and-naming"));
        assert!(!prompt.contains("material-style"));
    }

    #[test]
    fn repairs_an_imperfect_plan() {
        let plan = parse_plan(
            r##"{
                "name": "  Нічний   Злодій  ",
                "pitch": "Стелс у неоновому місті.",
                "design_document": "# GDD\n",
                "style": {"palette": ["#0B0E1A", "blue", "#3b7bff"], "fonts": "", "corner_radius": 90, "mood": "Неонова акварель"},
                "screens": [
                    {"title": "Головне меню", "summary": "Старт"},
                    {"title": "  ", "summary": "порожній"},
                    {"title": "HUD", "summary": "Світлячки"}
                ]
            }"##,
        )
        .unwrap();
        assert_eq!(plan.name, "Нічний Злодій");
        assert_eq!(
            plan.style.palette,
            ["#0b0e1a", "#3b7bff", "#a78bfa", "#3dd6d0", "#f2b444"]
        );
        assert_eq!(plan.style.fonts, DEFAULT_FONTS);
        assert_eq!(plan.style.corner_radius, 32);
        let titles: Vec<&str> = plan
            .screens
            .iter()
            .map(|screen| screen.title.as_str())
            .collect();
        assert_eq!(titles, ["Головне меню", "HUD"]);
        assert!(plan.references.is_empty());
    }

    #[test]
    fn cleans_the_reference_board() {
        let board = parse_references(
            r##"{"references": [
                {"topic": "characters", "title": "  Злодій у   плащі ", "note": "Силует", "query": "hooded thief silhouette", "palette": ["#112233", "red", "#ABCDEF", "#000", "#fff"]},
                {"topic": "unknown", "title": "Місто", "note": "", "query": "foggy city alleys", "palette": []},
                {"topic": "light", "title": "", "note": "без назви", "query": "x", "palette": []}
            ]}"##,
        )
        .unwrap();
        let references = &board.references;
        assert_eq!(references.len(), 1 + ReferenceTopic::ALWAYS_SEARCHED.len());
        assert_eq!(references[0].topic, ReferenceTopic::Characters);
        assert_eq!(references[0].title, "Злодій у плащі");
        assert_eq!(references[0].palette, ["#112233", "#abcdef", "#000"]);
        assert_eq!(references[1].topic, ReferenceTopic::World);
        assert!(board.analysis.summary.is_empty() && board.analysis.keywords.is_empty());
        assert!(parse_references(r#"{"references": []}"#).is_err());
        assert!(parse_references("не json").is_err());
    }

    #[test]
    fn cleans_the_prompt_analysis() {
        let board = parse_references(
            r##"{
                "analysis": {
                    "summary": "  Темне   фентезі з ізометричною камерою. ",
                    "keywords": ["руни", " Руни ", "", "туман"],
                    "needs": [
                        {"topic": "vfx", "what": "Спалахи рун при ударі"},
                        {"topic": "vfx", "what": "Дубль теми"},
                        {"topic": "mystery", "what": "Невідома тема"},
                        {"topic": "buttons", "what": "  "},
                        {"topic": "panels", "what": "Кам’яні рамки інвентаря"}
                    ]
                },
                "references": [
                    {"topic": "vfx", "title": "Рунічні спалахи", "note": "Колір удару", "query": "game vfx rune burst", "palette": []}
                ]
            }"##,
        )
        .unwrap();
        let analysis = board.analysis;
        assert_eq!(analysis.summary, "Темне фентезі з ізометричною камерою.");
        assert_eq!(analysis.keywords, ["руни", "туман"]);
        let topics: Vec<ReferenceTopic> = analysis.needs.iter().map(|need| need.topic).collect();
        assert_eq!(topics[..2], [ReferenceTopic::Vfx, ReferenceTopic::Panels]);
        assert_eq!(topics.len(), ReferenceTopic::ALWAYS_SEARCHED.len());
        assert_eq!(
            analysis.need_for(ReferenceTopic::Vfx).unwrap().what,
            "Спалахи рун при ударі"
        );
        assert_eq!(board.references[0].topic, ReferenceTopic::Vfx);
    }

    #[test]
    fn reference_brief_carries_the_player_prompt() {
        let style = StyleGuide::default();
        let message = references_message(&ReferenceBrief {
            prompt: "Рогалик про алхіміка",
            game_name: "Алхімік",
            pitch: "Зілля й підземелля",
            design_document: "# GDD",
            style: &style,
        });
        assert!(message.contains("<prompt>\nРогалик про алхіміка\n</prompt>"));
        assert!(message.contains("- analysis:"));
        assert!(message.contains(
            "Теми characters, characters_3d, interface, main_menu, panels, buttons, vfx, vfx_sprites шукаються для кожної гри обов’язково"
        ));
        assert!(message.contains("vfx_sprites — «vfx sprite sheet animation»"));
        assert!(message.contains("main_menu — «game main menu screen ui»"));
        assert!(!message.contains("music — «"));
        let schema = references_schema();
        assert_eq!(schema["required"], json!(["analysis", "references"]));
        let codes = &schema["properties"]["references"]["items"]["properties"]["topic"]["enum"];
        assert!(codes.as_array().unwrap().contains(&json!("characters_3d")));
    }

    #[test]
    fn always_searches_the_core_game_topics() {
        let board = parse_references(
            r##"{"references": [
                {"topic": "world", "title": "Готичні руїни", "note": "", "query": "foggy gothic game cathedral ruins", "palette": []},
                {"topic": "main_menu", "title": "Меню в соборі", "note": "", "query": "gothic cathedral main menu game ui", "palette": []}
            ]}"##,
        )
        .unwrap();
        for topic in ReferenceTopic::ALWAYS_SEARCHED {
            assert!(board.references.iter().any(|reference| reference.topic == topic));
            assert!(board.analysis.need_for(topic).is_some());
        }
        let menus = board
            .references
            .iter()
            .filter(|reference| reference.topic == ReferenceTopic::MainMenu)
            .count();
        assert_eq!(menus, 1);
        let sprites = board
            .references
            .iter()
            .find(|reference| reference.topic == ReferenceTopic::VfxSprites)
            .unwrap();
        assert_eq!(sprites.title, "VFX-спрайти й анімація");
        assert_eq!(sprites.query, "foggy gothic cathedral vfx sprite sheet animation");
        let styles = board
            .references
            .iter()
            .find(|reference| reference.topic == ReferenceTopic::Characters3d)
            .unwrap();
        assert_eq!(styles.query, "foggy gothic cathedral stylized 3d game character render");
    }

    #[test]
    fn keeps_core_topics_when_the_board_is_full() {
        let crowded: Vec<String> = (0..MAX_REFERENCES)
            .map(|index| {
                format!(
                    r#"{{"topic": "props", "title": "Предмет {index}", "note": "", "query": "rusty lantern {index}", "palette": []}}"#
                )
            })
            .collect();
        let board = parse_references(&format!(r#"{{"references": [{}]}}"#, crowded.join(","))).unwrap();
        assert_eq!(board.references.len(), MAX_REFERENCES);
        for topic in ReferenceTopic::ALWAYS_SEARCHED {
            assert!(board.references.iter().any(|reference| reference.topic == topic));
        }
        assert_eq!(board.references[0].title, "Предмет 0");
        assert!(board.references.iter().any(|reference| reference.topic == ReferenceTopic::Props));
    }

    #[test]
    fn rejects_plans_without_screens_or_json() {
        assert!(parse_plan("не json").is_err());
        let empty = r#"{"name":"A","pitch":"","design_document":"","style":{"palette":[],"fonts":"","corner_radius":8,"mood":""},"screens":[]}"#;
        assert!(parse_plan(empty).is_err());
    }

    #[test]
    fn extracts_the_html_document() {
        let answer = "Ось екран:\n```html\n<!DOCTYPE html>\n<html><body>Пауза</body></html>\n```";
        assert_eq!(
            extract_html(answer).unwrap(),
            "<!DOCTYPE html>\n<html><body>Пауза</body></html>"
        );
        assert_eq!(
            extract_html("<HTML><body>Меню</body></HTML>").unwrap(),
            "<HTML><body>Меню</body></HTML>"
        );
        assert_eq!(extract_html("без розмітки"), None);
    }

    #[test]
    fn revision_brief_includes_current_screen_and_feedback() {
        let style = StyleGuide {
            palette: vec!["#000000".into()],
            fonts: "Unbounded / Manrope".into(),
            corner_radius: 12,
            mood: "Неон".into(),
            ..StyleGuide::default()
        };
        let titles = vec!["Головне меню".to_owned(), "HUD".to_owned()];
        let board = vec![
            Reference {
                topic: ReferenceTopic::Panels,
                title: "Кам’яні рамки".into(),
                note: "Фаска і тріщини".into(),
                query: "stone frame game ui".into(),
                palette: vec!["#334455".into()],
                pins: vec![Pin {
                    id: "1".into(),
                    image: "https://i.pinimg.com/1.jpg".into(),
                    title: "Carved stone inventory frame".into(),
                }],
                artworks: Vec::new(),
            },
            Reference {
                topic: ReferenceTopic::Music,
                title: "Нічна тема".into(),
                note: "Тиша між нотами".into(),
                query: "dark ambient, low drones".into(),
                palette: Vec::new(),
                pins: Vec::new(),
                artworks: Vec::new(),
            },
        ];
        let message = screen_message(&ScreenBrief {
            game_name: "Нічний злодій",
            pitch: "Стелс",
            design_document: "# GDD",
            style: &style,
            sibling_titles: &titles,
            title: "HUD",
            summary: "Світлячки",
            board: &board,
            reference: Some(("Головне меню", "<html>menu</html>")),
            revision: Some(("<html>hud</html>", "Збільш мінікарту")),
        });
        assert!(message.contains("<references>\n- [Панелі й рамки] Кам’яні рамки"));
        assert!(message.contains("на пінах: Carved stone inventory frame"));
        assert!(!message.contains("dark ambient"));
        assert!(describe_references(&board).contains("(звучання: «dark ambient, low drones»)"));
        assert!(message.contains("<reference_screen title=\"Головне меню\">"));
        assert!(message.contains("<current_screen>\n<html>hud</html>"));
        assert!(message.contains("Збільш мінікарту"));
        assert!(message.contains("1280×720"));
    }
}
