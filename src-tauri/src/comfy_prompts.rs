use noctics_shared::{AssetKind, Reference, ReferenceTopic, StyleGuide};
use serde_json::{Value, json};

use crate::comfy_recipes::valid_keyscale;
use crate::comfy_store::Plan;
use crate::prompts;

const MAX_TITLE_CHARS: usize = 48;
const DEFAULT_BPM: u32 = 120;
const DEFAULT_KEYSCALE: &str = "C minor";
const ASPECTS: [&str; 3] = ["square", "portrait", "landscape"];

pub struct ProjectStyle<'a> {
    pub name: &'a str,
    pub pitch: &'a str,
    pub style: &'a StyleGuide,
    pub references: &'a [Reference],
}

fn topics_for(kind: AssetKind) -> &'static [ReferenceTopic] {
    match kind {
        AssetKind::Concept => &[
            ReferenceTopic::World,
            ReferenceTopic::Characters,
            ReferenceTopic::Characters3d,
            ReferenceTopic::Light,
            ReferenceTopic::Props,
        ],
        AssetKind::Sprite => &[
            ReferenceTopic::Characters,
            ReferenceTopic::Props,
            ReferenceTopic::Animation,
            ReferenceTopic::VfxSprites,
        ],
        AssetKind::Texture => &[ReferenceTopic::Materials, ReferenceTopic::World],
        AssetKind::Effect => &[
            ReferenceTopic::Vfx,
            ReferenceTopic::VfxSprites,
            ReferenceTopic::Light,
        ],
        AssetKind::Interface => &[
            ReferenceTopic::Interface,
            ReferenceTopic::MainMenu,
            ReferenceTopic::Panels,
            ReferenceTopic::Buttons,
            ReferenceTopic::Icons,
        ],
        AssetKind::Model => &[
            ReferenceTopic::Characters,
            ReferenceTopic::Characters3d,
            ReferenceTopic::Props,
            ReferenceTopic::Materials,
        ],
        AssetKind::Music => &[ReferenceTopic::Music],
        AssetKind::Sound => &[ReferenceTopic::Sounds],
    }
}

fn matching_references(kind: AssetKind, references: &[Reference]) -> Vec<Reference> {
    let topics = topics_for(kind);
    references
        .iter()
        .filter(|reference| topics.contains(&reference.topic))
        .cloned()
        .collect()
}

pub fn system_prompt() -> String {
    "\
Ти арт-директор і саунд-дизайнер ігрової студії Noctics. Перетворюєш коротке завдання на промпти для локальних моделей ComfyUI.
Ту саму задачу виконують одразу кілька моделей, щоб порівняти результат у галереї, тож промпт має бути однозначним, самодостатнім і не залежати від конкретної моделі.

Правила:
- prompt — англійською, 40–90 слів природною мовою: що зображено, форма, матеріали, кольори, світло, ракурс, композиція, стиль.
- tags — англійські теги через кому для SD-чекпойнтів і аудіомоделей (ACE-Step, Stable Audio).
- negative — англійською, що прибрати з результату.
- title — коротка назва задачі українською, до 48 символів.
- Без реальних брендів, людей, чужих ігрових персонажів і імен художників.
- Якщо завдання українською — перекладай зміст, а не транслітеруй.
- Відповідай лише JSON за схемою."
        .to_owned()
}

fn guidance(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Concept => {
            "Концепт-арт для геймдеву: персонаж, істота, локація або предмет. Виразний силует, зрозумілий дизайн, художнє світло, єдиний стиль гри. aspect: portrait для персонажів, landscape для локацій, square для предметів."
        }
        AssetKind::Sprite => {
            "Ігровий спрайт: рівно один об’єкт або персонаж повністю в кадрі по центру з відступом від країв, чіткий силует, однотонне світле тло без градієнтів, без тіні на підлозі й без тексту. Тло потім прибирається автоматично. aspect: square."
        }
        AssetKind::Texture => {
            "Безшовна тайлова текстура матеріалу (камінь, дерево, метал, трава, тканина…). Вигляд строго згори, рівне розсіяне світло, без перспективи, тіней, рамок і об’єктів у центрі. У prompt обов’язково: seamless tileable texture, top-down flat view, even lighting. aspect: square."
        }
        AssetKind::Effect => {
            "VFX-спрайт ефекту (вибух, магія, удар, іскри, дим, портал, аура…) по центру на абсолютно чорному тлі #000000. Яскраве світіння й насичені кольори, без персонажів, землі й тексту — для additive blending у рушії. aspect: square."
        }
        AssetKind::Interface => {
            "Елемент інтерфейсу гри: кнопка, рамка, панель, слот інвентаря, іконка предмета чи навички. Фронтально, по центру, однотонне тло, чіткі краї, без тексту й літер. aspect: square, для широких панелей landscape."
        }
        AssetKind::Model => {
            "Референс для перетворення картинки на 3D: рівно один об’єкт або персонаж повністю в кадрі, вигляд спереду трохи згори, персонажі в нейтральній A-позі, однотонне світло-сіре тло, рівне м’яке студійне світло, без тіней на підлозі й обрізаних частин. aspect: square."
        }
        AssetKind::Music => {
            "Ігрова музика. tags — англійські теги для ACE-Step: жанр, настрій, інструменти, темп, instrumental, game soundtrack; для фонової музики додай seamless loop. lyrics — [Instrumental] або розмітка секцій без слів. bpm і keyscale (напр. D minor) під настрій. seconds 20–60. prompt — коротко англійською, яка це музика."
        }
        AssetKind::Sound => {
            "Короткий звуковий ефект (SFX) для гри. tags англійською описують сам звук: джерело, матеріал, атака, хвіст, простір, плюс sound effect, foley, one shot, no music. lyrics — [Instrumental]. seconds 1–8, bpm 120. prompt — коротко англійською, що звучить."
        }
    }
}

pub fn message(kind: AssetKind, brief: &str, project: Option<&ProjectStyle<'_>>) -> String {
    let mut text = format!(
        "Тип ассета: {} ({}).\n{}\n\nЗавдання: {brief}\n",
        kind.label(),
        kind.code(),
        guidance(kind)
    );
    if let Some(project) = project {
        text.push_str(&format!(
            "\nЦе для гри «{}»: {}\nНастрій стилю: {}\nПалітра: {}\nТримай промпт у стилі цієї гри.\n",
            project.name,
            project.pitch,
            project.style.mood,
            project.style.palette.join(", ")
        ));
        let matching = matching_references(kind, project.references);
        if !matching.is_empty() {
            text.push_str(&format!(
                "\nРеференси гри для цього типу ассета — бери з них форму, фактуру, світло, кольори й звучання:\n{}\n",
                prompts::describe_references(&matching)
            ));
        }
    }
    text
}

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "prompt": { "type": "string" },
            "tags": { "type": "string" },
            "negative": { "type": "string" },
            "aspect": { "type": "string", "enum": ASPECTS },
            "seconds": { "type": "number" },
            "bpm": { "type": "integer" },
            "keyscale": { "type": "string" },
            "lyrics": { "type": "string" }
        },
        "required": ["title", "prompt", "tags", "negative", "aspect", "seconds", "bpm", "keyscale", "lyrics"],
        "additionalProperties": false
    })
}

pub fn parse(answer: &str, kind: AssetKind, brief: &str) -> Option<Plan> {
    let plan: Plan = serde_json::from_str(answer.trim()).ok()?;
    (!plan.prompt.trim().is_empty() || !plan.tags.trim().is_empty()).then(|| settle(plan, kind, brief))
}

pub fn fallback(kind: AssetKind, brief: &str) -> Plan {
    let prompt = match kind {
        AssetKind::Concept => format!(
            "Game concept art of {brief}. Clear readable silhouette, detailed design, dramatic lighting, painterly game art style."
        ),
        AssetKind::Sprite => format!(
            "Game sprite of {brief}, single object fully in frame, centered, clean silhouette, plain light background, no shadow, no text."
        ),
        AssetKind::Texture => format!(
            "Seamless tileable texture of {brief}, top-down flat view, even diffuse lighting, no perspective, no shadows."
        ),
        AssetKind::Effect => format!(
            "Game VFX sprite of {brief}, centered, bright glow, on pure black background, no characters, no text."
        ),
        AssetKind::Interface => format!(
            "Game UI element: {brief}, front view, centered, clean edges, plain background, no text, no letters."
        ),
        AssetKind::Model => format!(
            "{brief}, single object, full view, front three-quarter view, neutral pose, plain light gray background, soft even studio lighting, no shadows."
        ),
        AssetKind::Music => format!("{brief}, game soundtrack, instrumental"),
        AssetKind::Sound => format!("{brief}, sound effect, one shot, foley, no music"),
    };
    let tags = match kind {
        AssetKind::Music | AssetKind::Sound => prompt.clone(),
        _ => String::new(),
    };
    settle(
        Plan {
            prompt,
            tags,
            ..Plan::default()
        },
        kind,
        brief,
    )
}

pub fn settle(mut plan: Plan, kind: AssetKind, brief: &str) -> Plan {
    plan.title = single_line(&plan.title);
    if plan.title.is_empty() {
        plan.title = single_line(brief);
    }
    plan.title = plan.title.chars().take(MAX_TITLE_CHARS).collect();
    plan.prompt = plan.prompt.trim().to_owned();
    plan.tags = single_line(&plan.tags);
    if plan.prompt.is_empty() {
        plan.prompt = if plan.tags.is_empty() {
            brief.to_owned()
        } else {
            plan.tags.clone()
        };
    }
    plan.negative = single_line(&plan.negative);
    if !ASPECTS.contains(&plan.aspect.as_str()) {
        plan.aspect = default_aspect(kind).to_owned();
    }
    if matches!(kind, AssetKind::Model | AssetKind::Sprite | AssetKind::Effect | AssetKind::Texture) {
        plan.aspect = "square".to_owned();
    }
    let (low, high, usual) = match kind {
        AssetKind::Music => (10.0, 90.0, 30.0),
        AssetKind::Sound => (1.0, 12.0, 4.0),
        _ => (0.0, 0.0, 0.0),
    };
    plan.seconds = if plan.seconds.is_finite() && plan.seconds > 0.0 {
        plan.seconds.clamp(low, high)
    } else {
        usual
    };
    plan.bpm = if (40..=220).contains(&plan.bpm) {
        plan.bpm
    } else {
        DEFAULT_BPM
    };
    if !valid_keyscale(&plan.keyscale) {
        plan.keyscale = DEFAULT_KEYSCALE.to_owned();
    }
    if plan.lyrics.trim().is_empty() {
        plan.lyrics = "[Instrumental]".to_owned();
    }
    plan
}

fn default_aspect(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Concept => "portrait",
        _ => "square",
    }
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_clamps_claude_answers() {
        let answer = r#"{"title":"  Вибух   магії ","prompt":"A burst of arcane energy","tags":"magic, burst","negative":"text","aspect":"landscape","seconds":0,"bpm":500,"keyscale":"H minor","lyrics":""}"#;
        let plan = parse(answer, AssetKind::Effect, "вибух магії").unwrap();
        assert_eq!(plan.title, "Вибух магії");
        assert_eq!(plan.aspect, "square");
        assert_eq!(plan.bpm, DEFAULT_BPM);
        assert_eq!(plan.keyscale, DEFAULT_KEYSCALE);
        assert_eq!(plan.lyrics, "[Instrumental]");

        let music = r#"{"title":"Тема","prompt":"","tags":"epic orchestral","negative":"","aspect":"square","seconds":400,"bpm":90,"keyscale":"D minor","lyrics":"[Instrumental]"}"#;
        let plan = parse(music, AssetKind::Music, "тема").unwrap();
        assert_eq!(plan.seconds, 90.0);
        assert_eq!(plan.prompt, "epic orchestral");
        assert_eq!(plan.keyscale, "D minor");

        assert!(parse("not json", AssetKind::Concept, "x").is_none());
    }

    #[test]
    fn falls_back_to_the_brief_without_claude() {
        let sound = fallback(AssetKind::Sound, "скрип дверей");
        assert_eq!(sound.seconds, 4.0);
        assert!(sound.tags.contains("скрип дверей"));
        let concept = fallback(AssetKind::Concept, "нічний злодій");
        assert_eq!(concept.aspect, "portrait");
        assert_eq!(concept.title, "нічний злодій");
    }

    #[test]
    fn mentions_the_project_style() {
        let style = StyleGuide {
            palette: vec!["#101820".into(), "#f2aa4c".into()],
            fonts: "Unbounded".into(),
            corner_radius: 8,
            mood: "нуар під дощем".into(),
            ..StyleGuide::default()
        };
        let references = vec![
            Reference {
                topic: ReferenceTopic::Vfx,
                title: "Іскри ліхтаря".into(),
                note: "Теплі іскри".into(),
                query: "game vfx ember sparks".into(),
                palette: vec!["#f2aa4c".into()],
                pins: Vec::new(),
                artworks: Vec::new(),
            },
            Reference {
                topic: ReferenceTopic::Sounds,
                title: "Скрип заліза".into(),
                note: "Сухий метал".into(),
                query: "dry metallic creak".into(),
                palette: Vec::new(),
                pins: Vec::new(),
                artworks: Vec::new(),
            },
        ];
        let project = ProjectStyle {
            name: "Ліхтарник",
            pitch: "стелс про злодія світла",
            style: &style,
            references: &references,
        };
        let text = message(AssetKind::Sprite, "ліхтар", Some(&project));
        assert!(text.contains("«Ліхтарник»"));
        assert!(text.contains("#101820, #f2aa4c"));
        assert!(text.contains("Спрайт"));
        assert!(!text.contains("Референси гри"));
        let effect = message(AssetKind::Effect, "спалах", Some(&project));
        assert!(effect.contains("Іскри ліхтаря"));
        assert!(!effect.contains("Скрип заліза"));
        let sound = message(AssetKind::Sound, "двері", Some(&project));
        assert!(sound.contains("звучання: «dry metallic creak»"));
        assert!(!sound.contains("Іскри ліхтаря"));
    }
}
