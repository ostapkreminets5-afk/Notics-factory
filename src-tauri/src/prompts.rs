use noctics_shared::{
    GameEngine, MAX_ANALYSIS_KEYWORDS, MAX_PLANNED_SCREENS, MAX_PROJECT_NAME_CHARS, MAX_REFERENCES,
    MAX_SCREEN_SUMMARY_CHARS, MAX_SCREEN_TITLE_CHARS, MIN_REFERENCES, PALETTE_SIZE, PromptAnalysis,
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
const MAX_REFERENCE_TITLE_CHARS: usize = 60;
const MAX_REFERENCE_NOTE_CHARS: usize = 160;
const MAX_REFERENCE_QUERY_CHARS: usize = 80;
const MAX_ANALYSIS_SUMMARY_CHARS: usize = 420;
const MAX_KEYWORD_CHARS: usize = 32;
const MAX_NEED_CHARS: usize = 160;
const PIN_TITLES_PER_REFERENCE: usize = 3;

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
    palette: Vec<String>,
    fonts: String,
    corner_radius: i64,
    mood: String,
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

pub fn plan_message(prompt: &str, engine: GameEngine) -> String {
    format!(
        "Промпт гравця:\n<prompt>\n{prompt}\n</prompt>\n\n\
{}\n\n\
Сплануй гру для конвеєра:\n\
- name: робоча назва гри, 1–3 слова, до {MAX_PROJECT_NAME_CHARS} символів.\n\
- pitch: одне речення для картки проєкту.\n\
- design_document: короткий бриф гри в Markdown, 5–8 речень — жанр, світ, герой, основний цикл. Повний дизайн-документ, лор та інше окремо зроблять навички студії.\n\
- style: візуальна мова інтерфейсу гри. palette — рівно {PALETTE_SIZE} кольорів #rrggbb від фону до акценту; \
fonts — два шрифти з Google Fonts у форматі «Display / Text»; corner_radius — радіус кутів у px від 0 до {MAX_CORNER_RADIUS}; \
mood — 2–4 слова про настрій і фактуру.\n\
- screens: 4–6 ключових екранів інтерфейсу саме цієї гри (головне меню, HUD, вибір рівня, пауза, інвентар, фінальні титри — що їй потрібно). \
title — 1–3 слова; summary — що на екрані, до 80 символів.\n\
{}\n\n\
Стиль має випливати з жанру й світу гри: не бери за замовчуванням темний фон з неоновим акцентом, \
кремовий фон із засічками чи моноширинні підписи, якщо світ гри цього не просить.",
        engine_brief(engine),
        reference_rules()
    )
}

fn topic_codes() -> Vec<&'static str> {
    ReferenceTopic::ALL.iter().map(|topic| topic.code()).collect()
}

fn reference_rules() -> String {
    let topics = topic_codes().join(", ");
    format!(
        "- analysis: спершу розбери промпт гравця, щоб знати, що шукати в Pinterest. \
summary — 2–3 речення до 300 символів: жанр, світ, камера, художній стиль і настрій, які випливають із промпту; \
keywords — 5–{MAX_ANALYSIS_KEYWORDS} коротких візуальних ключових слів із промпту, по 1–3 слова; \
needs — що саме треба знайти для цієї гри, по одному пункту на тему: topic — одне з: {topics}; \
what — що конкретно шукати й навіщо грі, до 120 символів. \
Обов’язково продумай VFX і ефекти (удари, магія, частинки, вибухи), панелі й рамки інтерфейсу, кнопки, іконки, \
персонажів і ворогів, анімацію, звуки й музику — пропусти тему лише тоді, коли в цій грі її справді немає.\n\
- references: {MIN_REFERENCES}–{MAX_REFERENCES} референсів за пунктами needs — щонайменше один на кожен пункт. \
topic — одне з: {topics}; title — що на референсі, 2–6 слів; note — що саме взяти з нього для гри, до 120 символів; \
query — для візуальних тем це пошуковий запит для Pinterest англійською, 3–7 слів; для vfx, interface, panels, buttons, icons та animation додавай у запит \
«game ui», «game vfx», «game asset» чи «sprite animation», щоб Pinterest віддав саме ігрові елементи; \
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
    let references = clean_references(&board.references);
    if references.is_empty() {
        return Err(StudioError::Claude("the reference board was empty".into()));
    }
    Ok(ReferenceBoard {
        analysis: clean_analysis(&board.analysis),
        references,
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
    let pins: Vec<&str> = reference
        .pins
        .iter()
        .map(|pin| pin.title.trim())
        .filter(|title| !title.is_empty())
        .take(PIN_TITLES_PER_REFERENCE)
        .collect();
    let seen = if pins.is_empty() {
        String::new()
    } else {
        format!("; на пінах: {}", pins.join(" · "))
    };
    format!(
        "- [{topic}] {} — {} (Pinterest: «{}»; кольори: {}{seen})",
        reference.title,
        reference.note,
        reference.query,
        reference.palette.join(", ")
    )
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
            "style": {
                "type": "object",
                "properties": {
                    "palette": { "type": "array", "items": { "type": "string" } },
                    "fonts": { "type": "string" },
                    "corner_radius": { "type": "integer" },
                    "mood": { "type": "string" }
                },
                "required": ["palette", "fonts", "corner_radius", "mood"],
                "additionalProperties": false
            },
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

pub fn parse_plan(text: &str) -> Result<Plan, StudioError> {
    let raw: RawPlan = serde_json::from_str(text.trim()).map_err(|error| {
        StudioError::Claude(format!("the game plan was not valid JSON: {error}"))
    })?;
    let mut palette: Vec<String> = raw
        .style
        .palette
        .iter()
        .map(|color| color.trim().to_lowercase())
        .filter(|color| is_hex_color(color))
        .take(PALETTE_SIZE)
        .collect();
    palette.extend(
        DEFAULT_PALETTE
            .iter()
            .skip(palette.len())
            .map(|color| (*color).to_owned()),
    );
    let fonts = clip(&raw.style.fonts, MAX_FONTS_CHARS);
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
    Ok(Plan {
        name: if name.is_empty() {
            "Нова гра".to_owned()
        } else {
            name
        },
        pitch: clip(&raw.pitch, MAX_PITCH_CHARS),
        design_document: raw.design_document.trim().to_owned(),
        style: StyleGuide {
            palette,
            fonts: if fonts.is_empty() {
                DEFAULT_FONTS.to_owned()
            } else {
                fonts
            },
            corner_radius: u8::try_from(raw.style.corner_radius.clamp(0, MAX_CORNER_RADIUS))
                .unwrap_or_default(),
            mood: clip(&raw.style.mood, MAX_MOOD_CHARS),
        },
        screens,
        analysis: clean_analysis(&raw.analysis),
        references: clean_references(&raw.references),
    })
}

pub fn screen_message(brief: &ScreenBrief<'_>) -> String {
    let style = brief.style;
    let mut message = format!(
        "Гра: {} — {}\n\n<design_document>\n{}\n</design_document>\n\n\
Стиль інтерфейсу:\n- Палітра: {}\n- Шрифти: {}\n- Радіус кутів: {} px\n- Настрій: {}\n\n\
Екрани гри: {}.\n\n",
        brief.game_name,
        brief.pitch,
        brief.design_document,
        style.palette.join(", "),
        style.fonts,
        style.corner_radius,
        style.mood,
        brief.sibling_titles.join(", "),
    );
    let board = visual_references(brief.board);
    if !board.is_empty() {
        message.push_str(&format!(
            "Дошка референсів гри з Pinterest — бери з неї форму панелей, рамок і кнопок, іконки, світло, фактуру й кольори; \
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
Стиль інтерфейсу:\n- Палітра: {}\n- Шрифти: {}\n- Радіус кутів: {} px\n- Настрій: {}\n\n\
Екрани гри: {}.\n\n",
        job.prompt,
        engine_brief(job.engine),
        job.game_name,
        job.pitch,
        job.brief,
        style.palette.join(", "),
        style.fonts,
        style.corner_radius,
        style.mood,
        job.screen_titles.join(", "),
    );
    if !job.references.is_empty() {
        message.push_str(&format!(
            "Дошка референсів Pinterest, яку підібрало AI-ядро:\n<references>\n{}\n</references>\n\n",
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
    use noctics_shared::{Pin, SkillCategory};

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
            }],
        };
        let message = deliverable_message(&job);
        assert!(message.contains("<prompt>\nСтелс про злодія\n</prompt>"));
        assert!(message.contains("Рушій гри: Unreal Engine (C++ / Blueprints)."));
        assert!(message.contains(
            "- [Світло й настрій] Мокрі дахи вночі — Відблиски ліхтарів у калюжах (Pinterest: «rainy rooftops night lanterns»; кольори: #0b0e1a, #f2b444)"
        ));
        assert!(message.contains("<previous_result skill=\"Специфікація моделей\">\n| asset |"));
        assert!(message.contains("Екрани гри: Головне меню, HUD."));
        assert!(message.contains("Виконай навичку «Скрипти гри»"));
    }

    #[test]
    fn plan_brief_names_the_engine() {
        let message = plan_message("Гонки на човнах", GameEngine::Penumbra);
        assert!(message.contains("<prompt>\nГонки на човнах\n</prompt>"));
        assert!(message.contains("Рушій гри: Penumbra (Rust · wgpu)."));
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
        assert_eq!(references.len(), 2);
        assert_eq!(references[0].topic, ReferenceTopic::Characters);
        assert_eq!(references[0].title, "Злодій у плащі");
        assert_eq!(references[0].palette, ["#112233", "#abcdef", "#000"]);
        assert_eq!(references[1].topic, ReferenceTopic::World);
        assert!(board.analysis.is_empty());
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
        assert_eq!(topics, [ReferenceTopic::Vfx, ReferenceTopic::Panels]);
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
        assert!(message.contains("vfx, interface, panels, buttons"));
        let schema = references_schema();
        assert_eq!(schema["required"], json!(["analysis", "references"]));
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
            },
            Reference {
                topic: ReferenceTopic::Music,
                title: "Нічна тема".into(),
                note: "Тиша між нотами".into(),
                query: "dark ambient, low drones".into(),
                palette: Vec::new(),
                pins: Vec::new(),
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
