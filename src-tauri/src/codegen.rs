use std::ffi::OsString;
use std::path::Path;

use noctics_shared::{CLAUDE_MODEL, GameEngine, OutputStatus, ProjectDetail, Skill, StudioError, engine_takes_motions};
use serde_json::Value;

use crate::claude_code;
use crate::prompts;
use crate::recipes::{self, GameNames};
use crate::toolchains::Toolchains;

const ACTIVITY_CHARS: usize = 160;
const FAILURE_CHARS: usize = 300;
const CODE_TOOLS: &str = "Read,Write,Edit,Glob,Grep,Bash";
const FALLBACK_UNITY: &str = "2022.3 LTS";
const FALLBACK_UNREAL: &str = "5.4";
pub const ART_GUIDE: &str = "art.md";
pub const FOUND_ASSETS: &str = "found-assets.md";
pub const BUILD_SKILLS: &str = "build-skills";
const ART_PROMPTS: &str = "art/prompts.md";
const MOTION_WISHLIST: &str = "animations.md";
const ASSET_CREDITS: &str = "CREDITS.md";

pub struct Assignment<'a> {
    pub engine: GameEngine,
    pub names: &'a GameNames,
    pub brief: &'a Path,
    pub tools: &'a Toolchains,
    pub request: Option<&'a str>,
    pub fresh: bool,
    pub project_id: i64,
    pub art_api: Option<&'a str>,
    pub art_guide: bool,
    pub motion_api: Option<&'a str>,
    pub build_skills: bool,
}

pub enum Event {
    Activity(Vec<String>),
    Finished(Result<(), String>),
    Quiet,
}

pub fn write_brief(dir: &Path, detail: &ProjectDetail, art_guide: Option<&str>) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)
            .map_err(|error| format!("Не вдалося очистити {}: {error}", dir.display()))?;
    }
    let project = &detail.project;
    let style = &project.style;
    let screens = detail
        .screens
        .iter()
        .map(|screen| format!("- {} — {}", screen.title, screen.summary))
        .collect::<Vec<_>>()
        .join("\n");
    let design = format!(
        "# {}\n\n{}\n\n## Промпт гравця\n\n{}\n\n## Бриф\n\n{}\n\n## Стиль інтерфейсу\n\n\
- Палітра: {}\n- Шрифти: {}\n- Радіус кутів: {} px\n- Настрій: {}\n- Вигляд світу: {}\n\n## Екрани\n\n{}\n",
        project.name,
        project.pitch,
        project.prompt,
        detail.design_document,
        style.palette_lines(),
        style.fonts,
        style.corner_radius,
        style.mood,
        style.look,
        screens
    );
    recipes::write_file(&dir.join("design.md"), design.as_bytes())?;
    if !project.references.is_empty() {
        let board = format!(
            "# Референси\n\n{}\n",
            prompts::describe_references(&project.references)
        );
        recipes::write_file(&dir.join("references.md"), board.as_bytes())?;
    }
    let finished = detail
        .outputs
        .iter()
        .filter(|output| output.status == OutputStatus::Done)
        .filter_map(|output| output.content.as_ref().map(|content| (output, content)));
    for (index, (output, content)) in finished.enumerate() {
        let file = dir
            .join("skills")
            .join(format!("{:02}-{}.md", index + 1, recipes::slugify(&output.skill_name)));
        recipes::write_file(&file, format!("# {}\n\n{content}\n", output.title).as_bytes())?;
    }
    let drawn = detail
        .screens
        .iter()
        .filter_map(|screen| screen.html.as_ref().map(|html| (screen, html)));
    for (index, (screen, html)) in drawn.enumerate() {
        let file = dir
            .join("screens")
            .join(format!("{:02}-{}.html", index + 1, recipes::slugify(&screen.title)));
        recipes::write_file(&file, html.as_bytes())?;
    }
    if let Some(guide) = art_guide {
        recipes::write_file(&dir.join(ART_GUIDE), guide.as_bytes())?;
    }
    Ok(())
}

pub fn arguments(brief: &Path, system: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--model",
        CLAUDE_MODEL,
        "--effort",
        "high",
        "--permission-mode",
        "acceptEdits",
        "--tools",
        CODE_TOOLS,
        "--allowedTools",
        CODE_TOOLS,
        "--safe-mode",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--add-dir",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    args.push(brief.as_os_str().to_owned());
    args.push("--append-system-prompt".into());
    args.push(system.into());
    args
}

pub fn system_prompt(assignment: &Assignment<'_>) -> String {
    let engine = assignment.engine;
    format!(
        "Ти — AI-ядро Noctics Factory на етапі «06 Збірка». Ти пишеш повний робочий проєкт гри, \
який студія автоматично збере у фінальні білди для Windows, macOS, Linux, Web, Android та iOS.\n\n\
Рушій: {} ({}).\n{}\n\n\
Правила:\n\
- Поточна папка — корінь проєкту гри. Створюй і змінюй файли лише в ній.\n\
- Матеріали студії лежать у {}: design.md (бриф, стиль, екрани), skills/ (дизайн-документ, лор, специфікації й скрипти від навичок студії), \
screens/ (затверджені макети інтерфейсу 1280×720 у HTML), references.md (референси). Прочитай їх перед роботою.\n\
{}- Інтерфейс гри повторює макети зі screens/: палітра, шрифти, розкладка й тексти.\n\
{}{}\n\
- Гра повна й прохідна: головне меню, основний цикл з дизайн-документа, перемога й поразка, пауза, налаштування гучності.\n\
- Керування: клавіатура й миша, геймпад і сенсорне керування на телефоні.\n\
- Гра працює офлайн: жодних мережевих запитів, CDN чи зовнішніх ассетів під час гри.\n\
- Жодних коментарів у коді: зрозумілі назви, малі функції, іменовані константи.\n\
- Без заглушок, TODO і lorem ipsum.\n\
- {}\n\
- Наприкінці коротко напиши, що зроблено.",
        engine.label(),
        engine.stack(),
        engine_contract(engine, assignment.tools),
        assignment.brief.display(),
        build_skill_rules(assignment),
        art_rules(assignment),
        motion_rules(assignment),
        verification(engine, assignment.tools),
    )
}

fn build_skill_rules(assignment: &Assignment<'_>) -> String {
    if !assignment.build_skills {
        return String::new();
    }
    format!(
        "- Навички етапу «06 Збірка» лежать у {}: це правила саме для тебе. \
Виконай кожну прямо в коді й файлах гри — скрипти механік, ефекти, шейдери, — а не окремим документом.\n",
        assignment.brief.join(BUILD_SKILLS).display()
    )
}

pub fn write_build_skills(dir: &Path, skills: &[Skill]) -> Result<(), String> {
    for (index, skill) in skills.iter().enumerate() {
        let file = dir
            .join(BUILD_SKILLS)
            .join(format!("{:02}-{}.md", index + 1, recipes::slugify(&skill.name)));
        recipes::write_file(&file, skill.to_skill_markdown().as_bytes())?;
    }
    Ok(())
}

fn motion_rules(assignment: &Assignment<'_>) -> String {
    let engine = assignment.engine;
    if !engine_takes_motions(engine) {
        return String::new();
    }
    let Some(base) = assignment.motion_api else {
        return format!(
            "\n- Mixamo не підключено, тож готових анімацій скелета немає. Якщо в грі є 3D-персонажі, запиши в {MOTION_WISHLIST} \
список потрібних анімацій англійською (idle, walk, run…) і анімуй їх кодом через кістки чи твіни, без заглушок."
        );
    };
    let project = assignment.project_id;
    format!(
        "\n- Анімації 3D-персонажів з людським скелетом бери з Mixamo через Mixamo API Noctics {base}, поки пишеш код. \
Спершу GET {base}/plan?project_id={project} — Noctics розбере дизайн-документ і поставить у чергу набір під жанр (idle, walk, run, jump, hit, death і, наприклад, удари мечем чи стрільбу). \
Кожну іншу анімацію замов GET {base}/order?name=<коротка-латинська-назва>&q=<що шукати англійською>&project_id={project}&in_place=1. \
in_place=1 — для ходьби, бігу й стрейфу, бо персонажа рухає код; без in_place — для стрибків, перекатів і атак із зсувом; mirror=1 — дзеркальна копія; trim=10-90 — обрізати кліп у відсотках; fps=30 або 60. \
Щоб вибрати точніше, подивись GET {base}/search?q=<запит> і передай id найкращої в /order. \
Стеж GET {base}/animations/<id>, доки status не стане done або failed; поле file — шлях до FBX на диску: скопіюй його командою cp в ассети гри як <name>.fbx. \
Повтор того самого /order не створює дубль, тож перевіряти можна сміливо.\n\
- Усі кліпи запечені на одному скелеті Mixamo, тож вони змішуються на одній моделі. Допрацюй кожен: цикл для idle, walk і run, \
плавні переходи й блендинг у стейт-машині анімацій, швидкість програвання під реальну швидкість руху, вимкнений кореневий рух для in-place кліпів, \
події кроків і влучань на потрібних кадрах. Якщо модель персонажа без скелета, замов skin=1 для idle — цей FBX стане моделлю, а решта кліпів іде без скіну.\n\
- {}",
        motion_import(engine)
    )
}

fn motion_import(engine: GameEngine) -> &'static str {
    match engine {
        GameEngine::Godot => {
            "Godot 4 сам імпортує FBX: клади кліпи в assets/animations/, у сцені персонажа зв’яжи AnimationPlayer з AnimationTree (BlendSpace для ходьби й бігу), \
цикли вмикай через Animation.loop_mode."
        }
        GameEngine::Unity => {
            "Unity: FBX у Assets/Resources/Animations, кліпи бери Resources.LoadAll<AnimationClip>, а змішуй через Playables API (AnimationMixerPlayable), \
бо Animator Controller кодом не створити."
        }
        GameEngine::Unreal => {
            "Unreal імпортує FBX лише в редакторі: поклади кліпи в Content/Animations/Source і опиши імпорт у animations.md, а рух персонажа в C++ не роби залежним від них."
        }
        GameEngine::ThreeJs => {
            "Three.js: FBX у public/animations, вантаж FBXLoader з three/examples/jsm/loaders/FBXLoader.js, програвай через AnimationMixer і crossFadeTo між діями."
        }
        _ => {
            "Цей рушій читає glTF, а не FBX: якщо blender є в PATH, конвертуй кожен FBX у GLB через blender -b --python-expr \
з bpy.ops.import_scene.fbx і bpy.ops.export_scene.gltf; без Blender залиш FBX в assets/animations і анімуй кістки кодом."
        }
    }
}

fn art_rules(assignment: &Assignment<'_>) -> String {
    let guide = if assignment.art_guide {
        format!(" Повні правила арту — у {ART_GUIDE} поруч із design.md.")
    } else {
        String::new()
    };
    let source = match assignment.art_api {
        Some(base) => format!(
            "- Арт генеруй через ComfyUI API Noctics {base}, поки йде робота над кодом: \
POST {base}/tasks з Content-Type: application/json і тілом {{\"kind\": \"sprite\", \"brief\": \"що це, до 600 символів\", \
\"prompt\": \"повний промпт англійською\", \"negative\": \"flat vector, clip art, simple shapes\", \"project_id\": {}}}. \
kind — concept, sprite, texture, effect, interface, model, music або sound. Задача йде на всі увімкнені моделі: \
стеж GET {base}/tasks/<id>, доки status не стане done або failed, обери найкращий файл серед runs[].files[].url, \
завантаж його в ассети гри й переглянь через Read. Для серії персонажів тримай ту саму основу промпту.\n\
- Кожен ассет будуй на референсах його теми з references.md: VFX і світло → effect, панелі, кнопки, іконки й HUD → interface, \
персонажі, предмети й анімація → sprite або concept, матеріали → texture, звуки → sound, музика → music. \
Звуки й музику гри теж генеруй тут, за тегами звучання з референсів. Якщо не передати prompt, AI-ядро Noctics складе його саме з референсів проєкту.",
            assignment.project_id
        ),
        None => format!(
            "- ComfyUI зараз недоступний, тож арт згенерувати не вийде. Для кожного потрібного файлу запиши шлях і промпт англійською в {ART_PROMPTS}, \
а в грі на його місці постав тонку рамку-слот з назвою файлу — це єдиний дозволений виняток із правила про заглушки. Малюнок з фігур замість арту — ніколи."
        ),
    };
    format!(
        "- Персонажі, істоти, предмети, локації й ілюстрації — лише справжній арт у PNG або GLB, ніколи не фігури з коду, SVG, \
_draw, Polygon2D, CSG, PIL чи примітивів. Кодом і примітивами — лише рамки інтерфейсу, ефекти, геометрія рівня й колізії.{guide}\n\
- У макетах screens/ стоять слоти <figure class=\"art-slot\" data-art=\"шлях\" data-prompt=\"промпт\">: \
кожен такий арт має лягти в гру за шляхом data-art, згенерований з промпту data-prompt.\n{source}{}",
        found_asset_rules(assignment.brief)
    )
}

fn found_asset_rules(brief: &Path) -> String {
    if !brief.join(FOUND_ASSETS).is_file() {
        return String::new();
    }
    format!(
        "\n- У {FOUND_ASSETS} поруч із design.md — готові ассети, які AI-ядро знайшло в інтернеті (itch.io, OpenGameArt, Kenney тощо), \
з ліцензіями й цінами. Безкоштовні з ліцензією, що дозволяє використання в іграх, можна завантажити в ассети гри, \
якщо файл віддається за прямим посиланням без входу; перевір вміст архіву й переглянь картинки через Read. \
Автора, сторінку й ліцензію кожного такого ассета запиши в {ASSET_CREDITS} у корені гри. \
Платні ассети й ті, що вимагають входу, — лише орієнтир стилю: їх не завантажуй, а генеруй схожий арт."
    )
}

pub fn task_message(assignment: &Assignment<'_>) -> String {
    let title = &assignment.names.title;
    match (assignment.fresh, assignment.request) {
        (true, None) => format!(
            "Створи гру «{title}» з нуля за матеріалами студії в {}.",
            assignment.brief.display()
        ),
        (true, Some(request)) => format!(
            "Створи гру «{title}» з нуля за матеріалами студії в {}. Окремо врахуй:\n\n{request}",
            assignment.brief.display()
        ),
        (false, Some(request)) => format!(
            "Проєкт гри «{title}» уже є в поточній папці. Доопрацюй його:\n\n{request}"
        ),
        (false, None) => format!(
            "Проєкт гри «{title}» уже є в поточній папці. Перевір, що гра повна й збирається, і виправ помилки та недоробки."
        ),
    }
}

fn engine_contract(engine: GameEngine, tools: &Toolchains) -> String {
    match engine {
        GameEngine::Godot => {
            let version = tools.godot.as_ref().map_or_else(|| "4".to_owned(), |godot| godot.short_version());
            format!(
                "Godot {version} з GDScript. У корені — project.godot з config/name і run/main_scene. \
Сцени (.tscn) і ресурси пиши в текстовому форматі Godot 4. Не створюй export_presets.cfg і папку .godot — пресети експорту студія додає сама. \
Арт — PNG і GLB у assets/; кодом (_draw, шейдери, CSG) будуй лише інтерфейс, ефекти й геометрію рівня."
            )
        }
        GameEngine::Unity => {
            let version = tools
                .unity
                .first()
                .map_or(FALLBACK_UNITY, |editor| editor.version.as_str());
            format!(
                "Unity {version} з C#. Обов’язково: ProjectSettings/ProjectVersion.txt (m_EditorVersion: {version}), \
Packages/manifest.json з потрібними пакетами (com.unity.ugui тощо), скрипти в Assets/Scripts. \
Сцен вручну не пиши: студія сама створить Assets/Scenes/Main.unity з камерою і світлом, а гра має запускатися кодом через \
[RuntimeInitializeOnLoadMethod(RuntimeInitializeLoadType.AfterSceneLoad)] — рівні, матеріали й UI (UGUI) теж кодом; примітиви — лише для геометрії рівня, \
а арт — PNG у Assets/Resources/Art, завантажений через Resources.Load. \
Не створюй Assets/Editor/NocticsBuild.cs — це скрипт збірки студії."
            )
        }
        GameEngine::Unreal => {
            let version = tools
                .unreal
                .first()
                .map_or(FALLBACK_UNREAL, |engine| engine.version.as_str());
            format!(
                "Unreal Engine {version} з C++. У корені — <Назва>.uproject (EngineAssociation \"{version}\", один Runtime-модуль), \
Source/<Модуль>/ з <Модуль>.Build.cs, Source/<Модуль>.Target.cs і Source/<Модуль>Editor.Target.cs, Config/DefaultEngine.ini та Config/DefaultGame.ini. \
Назви проєкту й модуля — латиницею без пробілів. Мапи (.umap) бінарні, тож не створюй їх: GameDefaultMap=/Engine/Maps/Entry, \
а GlobalDefaultGameMode — твій C++ GameMode, який будує рівень кодом (актори, світло, камера, UI на UMG або Slate з C++). \
/Engine/BasicShapes — лише для геометрії рівня; арт — PNG у Content/Art, доданий у DirectoriesToAlwaysStageAsNonUFS і завантажений через FImageUtils::ImportFileAsTexture2D."
            )
        }
        GameEngine::ThreeJs | GameEngine::BabylonJs | GameEngine::Phaser => format!(
            "{} на Vite + TypeScript. package.json зі скриптами \"dev\" і \"build\": \"vite build\", vite.config.ts з base: './', \
точка входу index.html у корені. Ассети — локально в public/, без CDN. \
Полотно на весь екран з адаптацією під будь-яке співвідношення сторін: гру запускатимуть у браузері, у десктоп-оболонці й на телефоні.",
            engine.label()
        ),
        GameEngine::Bevy | GameEngine::ThreeD | GameEngine::Penumbra => format!(
            "{} на Rust. Cargo.toml з одним bin-крейтом, ассети — у assets/. Код має збиратися і нативно, і під wasm32-unknown-unknown: \
без потоків і доступу до файлів у веб-версії. Додай index.html для trunk з <link data-trunk rel=\"rust\" />. \
Для Android гра — це ще й [lib] з crate-type = [\"cdylib\", \"rlib\"], а точка входу android_main (для Bevy — #[bevy_main]) викликає ту саму логіку, що й main.",
            engine.label()
        ),
        GameEngine::Defold => "Defold з Lua. У корені — game.project з [bootstrap] main_collection = /main/main.collectionc \
і input/game.input_binding з усіма діями гри; колекції, game object-и, gui і скрипти — у текстовому форматі Defold. Арт — спрайти з PNG в atlas-ах у assets/; gui-ноди — лише для інтерфейсу. \
Не додавай bob.jar у проєкт."
            .to_owned(),
    }
}

fn verification(engine: GameEngine, tools: &Toolchains) -> String {
    match engine {
        GameEngine::Godot => match &tools.godot {
            Some(godot) => format!(
                "Перевір проєкт командою \"{}\" --headless --path . --quit-after 10 і виправ усі помилки скриптів.",
                godot.exe.display()
            ),
            None => "Godot на цьому комп’ютері не знайдено, тож перевір GDScript особливо уважно.".to_owned(),
        },
        GameEngine::ThreeJs | GameEngine::BabylonJs | GameEngine::Phaser => {
            "Перевір: npm install і npm run build мають проходити без помилок.".to_owned()
        }
        GameEngine::Bevy | GameEngine::ThreeD | GameEngine::Penumbra => {
            "Перевір: cargo build --release має проходити без помилок.".to_owned()
        }
        GameEngine::Unity | GameEngine::Unreal | GameEngine::Defold => {
            "Студія збере проєкт сама — переконайся, що всі файли узгоджені між собою і компілюються.".to_owned()
        }
    }
}

pub fn read_event(line: &str, root: &Path) -> Event {
    let Ok(event) = serde_json::from_str::<Value>(line) else {
        return Event::Quiet;
    };
    match event["type"].as_str() {
        Some("assistant") => Event::Activity(
            event["message"]["content"]
                .as_array()
                .map(|blocks| blocks.iter().filter_map(|block| describe_block(block, root)).collect())
                .unwrap_or_default(),
        ),
        Some("result") => {
            let failed = event["is_error"].as_bool().unwrap_or(false)
                || event["subtype"].as_str().is_some_and(|subtype| subtype != "success");
            if failed {
                let message = event["result"].as_str().unwrap_or_default();
                Event::Finished(Err(explain_failure(message, event["api_error_status"].as_u64())))
            } else {
                Event::Finished(Ok(()))
            }
        }
        _ => Event::Quiet,
    }
}

pub fn explain_failure(message: &str, status: Option<u64>) -> String {
    match claude_code::classify(message, status) {
        Some(StudioError::UsageLimit) => "Ліміт підписки Claude вичерпано — продовж, коли він оновиться.".into(),
        Some(StudioError::ClaudeSignedOut) => "Claude Code не підключено до підписки — увійди в Налаштуваннях.".into(),
        Some(StudioError::RateLimited) => "Забагато запитів до Claude — спробуй трохи згодом.".into(),
        Some(StudioError::Overloaded) => "Claude зараз перевантажений — спробуй трохи згодом.".into(),
        _ if message.trim().is_empty() => "Claude Code завершив роботу з помилкою.".into(),
        _ => format!("Claude: {}", clip(message, FAILURE_CHARS)),
    }
}

fn describe_block(block: &Value, root: &Path) -> Option<String> {
    match block["type"].as_str()? {
        "text" => block["text"]
            .as_str()?
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| clip(line, ACTIVITY_CHARS)),
        "tool_use" => Some(describe_tool(block["name"].as_str()?, &block["input"], root)),
        _ => None,
    }
}

fn describe_tool(name: &str, input: &Value, root: &Path) -> String {
    let file = || {
        input["file_path"]
            .as_str()
            .map_or_else(String::new, |path| relative(path, root))
    };
    let text = |key: &str| input[key].as_str().unwrap_or_default().to_owned();
    let line = match name {
        "Write" => format!("Створює {}", file()),
        "Edit" | "MultiEdit" => format!("Редагує {}", file()),
        "Read" => format!("Читає {}", file()),
        "Bash" => format!("$ {}", text("command")),
        "Glob" => format!("Шукає файли {}", text("pattern")),
        "Grep" => format!("Шукає «{}»", text("pattern")),
        other => other.to_owned(),
    };
    clip(&line, ACTIVITY_CHARS)
}

fn relative(path: &str, root: &Path) -> String {
    let path = Path::new(path);
    path.strip_prefix(root)
        .map(|inner| inner.to_string_lossy().replace('\\', "/"))
        .ok()
        .or_else(|| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

fn clip(text: &str, limit: usize) -> String {
    let single = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if single.chars().count() <= limit {
        return single;
    }
    let mut clipped: String = single.chars().take(limit.saturating_sub(1)).collect();
    clipped.push('…');
    clipped
}

#[cfg(test)]
mod tests {
    use noctics_shared::{
        Project, ProjectStatus, PromptAnalysis, Screen, ScreenStatus, SkillOutput, StyleGuide,
    };

    use super::*;

    #[test]
    fn turns_claude_events_into_activity() {
        let root = Path::new("games").join("heist").join("source");
        let write = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"\n Пишу головну сцену\nдалі"}},{{"type":"tool_use","name":"Write","input":{{"file_path":{},"content":"x"}}}},{{"type":"tool_use","name":"Bash","input":{{"command":"npm install\nnpm run build"}}}}]}}}}"#,
            serde_json::to_string(&root.join("scenes").join("main.tscn").to_string_lossy()).unwrap()
        );
        let Event::Activity(lines) = read_event(&write, &root) else {
            panic!("expected activity");
        };
        assert_eq!(lines, ["Пишу головну сцену", "Створює scenes/main.tscn", "$ npm install npm run build"]);

        let done = r#"{"type":"result","subtype":"success","is_error":false,"result":"Готово"}"#;
        assert!(matches!(read_event(done, &root), Event::Finished(Ok(()))));
        let limit = r#"{"type":"result","subtype":"success","is_error":true,"result":"Claude AI usage limit reached|1762041600"}"#;
        let Event::Finished(Err(message)) = read_event(limit, &root) else {
            panic!("expected failure");
        };
        assert!(message.starts_with("Ліміт підписки"));
        assert!(matches!(read_event("not json", &root), Event::Quiet));
        assert!(matches!(read_event(r#"{"type":"user"}"#, &root), Event::Quiet));
    }

    #[test]
    fn writes_the_studio_brief_for_claude() {
        let dir = std::env::temp_dir().join(format!("noctics-brief-{}", std::process::id()));
        let detail = ProjectDetail {
            project: Project {
                id: 1,
                name: "Нічний злодій".into(),
                prompt: "Стелс".into(),
                pitch: "Крадіжка світла".into(),
                engine: GameEngine::Godot,
                status: ProjectStatus::Ready,
                style: StyleGuide::default(),
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
            design_document: "# GDD".into(),
            screens: vec![Screen {
                id: 4,
                project_id: 1,
                position: 0,
                title: "Головне меню".into(),
                summary: "Старт".into(),
                status: ScreenStatus::Approved,
                html: Some("<!DOCTYPE html><html></html>".into()),
                revision: 1,
                error: None,
            }],
            outputs: vec![
                SkillOutput {
                    id: 9,
                    project_id: 1,
                    skill_name: "game-scripts".into(),
                    title: "Скрипти гри".into(),
                    stage: 6,
                    status: OutputStatus::Done,
                    content: Some("extends Node".into()),
                    error: None,
                },
                SkillOutput {
                    id: 10,
                    project_id: 1,
                    skill_name: "lore-and-naming".into(),
                    title: "Лор".into(),
                    stage: 1,
                    status: OutputStatus::Failed,
                    content: None,
                    error: None,
                },
            ],
        };
        write_brief(&dir, &detail, Some("# Справжній арт\n")).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(ART_GUIDE)).unwrap(),
            "# Справжній арт\n"
        );
        let design = std::fs::read_to_string(dir.join("design.md")).unwrap();
        assert!(design.starts_with("# Нічний злодій"));
        assert!(design.contains("- Головне меню — Старт"));
        assert_eq!(
            std::fs::read_to_string(dir.join("skills").join("01-game-scripts.md")).unwrap(),
            "# Скрипти гри\n\nextends Node\n"
        );
        assert!(dir.join("screens").join("01-holovne-meniu.html").is_file());
        assert!(!dir.join("references.md").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tells_claude_how_the_studio_will_build() {
        let names = GameNames::new("Нічний злодій");
        let tools = Toolchains::default();
        let brief = Path::new("brief");
        let assignment = Assignment {
            engine: GameEngine::Phaser,
            names: &names,
            brief,
            tools: &tools,
            request: None,
            fresh: true,
            project_id: 7,
            art_api: Some("http://127.0.0.1:47822"),
            art_guide: true,
            motion_api: Some("http://127.0.0.1:47834"),
            build_skills: true,
        };
        let system = system_prompt(&assignment);
        assert!(system.contains("Навички етапу «06 Збірка» лежать у"));
        assert!(system.contains(BUILD_SKILLS));
        assert!(
            !system_prompt(&Assignment {
                build_skills: false,
                ..assignment
            })
            .contains(BUILD_SKILLS)
        );
        assert!(system.contains("Рушій: Phaser (JavaScript · 2D)."));
        assert!(system.contains("base: './'"));
        assert!(system.contains("Жодних коментарів у коді"));
        assert!(system.contains("POST http://127.0.0.1:47822/tasks"));
        assert!(system.contains("\"project_id\": 7"));
        assert!(system.contains("data-art"));
        assert!(system.contains(ART_GUIDE));
        assert!(system.contains("звуки → sound, музика → music"));
        let offline = system_prompt(&Assignment {
            art_api: None,
            art_guide: false,
            ..assignment
        });
        assert!(offline.contains(ART_PROMPTS));
        assert!(!offline.contains("/tasks"));
        assert!(!offline.contains(ART_GUIDE));
        for engine in GameEngine::ALL {
            let contract = engine_contract(engine, &tools);
            assert!(!contract.contains("Графіку будуй кодом"));
            assert!(!contract.contains("згенеровані кодом"));
            assert!(!contract.contains("згенеруєш скриптом"));
        }
        assert!(task_message(&assignment).starts_with("Створи гру «Нічний злодій» з нуля"));
        let fix = Assignment {
            request: Some("Збірка під Web впала"),
            fresh: false,
            ..assignment
        };
        assert!(task_message(&fix).ends_with("Доопрацюй його:\n\nЗбірка під Web впала"));
        let args = arguments(brief, "system");
        assert_eq!(args.last().unwrap(), "system");
        assert!(args.iter().any(|arg| arg == "stream-json"));
    }

    #[test]
    fn tells_claude_where_to_take_character_animations() {
        let names = GameNames::new("Нічний злодій");
        let tools = Toolchains::default();
        let assignment = Assignment {
            engine: GameEngine::Godot,
            names: &names,
            brief: Path::new("brief"),
            tools: &tools,
            request: None,
            fresh: true,
            project_id: 7,
            art_api: None,
            art_guide: false,
            motion_api: Some("http://127.0.0.1:47834"),
            build_skills: false,
        };
        let system = system_prompt(&assignment);
        assert!(system.contains("GET http://127.0.0.1:47834/plan?project_id=7"));
        assert!(system.contains("/order?name="));
        assert!(system.contains("AnimationTree"));
        let offline = system_prompt(&Assignment {
            motion_api: None,
            ..assignment
        });
        assert!(offline.contains(MOTION_WISHLIST) && !offline.contains("/order?"));
        let flat = system_prompt(&Assignment {
            engine: GameEngine::Phaser,
            ..assignment
        });
        assert!(!flat.contains("Mixamo"));
    }

    #[test]
    fn hands_build_stage_skills_to_the_agent_as_files() {
        let dir = std::env::temp_dir().join(format!("noctics-build-skills-{}", std::process::id()));
        let skill = Skill {
            id: 1,
            name: "game-scripts".into(),
            stage: noctics_shared::BUILD_STAGE,
            title: "Скрипти гри".into(),
            summary: "Пише скрипти механік".into(),
            category: noctics_shared::SkillCategory::Code,
            instructions: "Кожна механіка — свій модуль.".into(),
            enabled: true,
        };
        write_build_skills(&dir, &[skill]).unwrap();
        let written = std::fs::read_to_string(dir.join(BUILD_SKILLS).join("01-game-scripts.md")).unwrap();
        assert!(written.contains("Кожна механіка — свій модуль."));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
