use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use noctics_shared::{PlaytestIssue, PlaytestReport, PlaytestShot, PlaytestVerdict};
use serde::Deserialize;

use crate::recipes;

pub const PLAY_SECONDS: u32 = 40;
pub const PLAYTEST_DIR: &str = "playtest";
pub const FIRST_ROUND: &str = "round-1";
pub const FINAL_ROUND: &str = "final";
pub const GODOT_LOG: &str = "godot.log";
pub const REVIEW_LOG: &str = "review.log";
const BOT_SCRIPT: &str = include_str!("playtest_bot.gd");
const BOT_DIR: &str = "noctics_playtest";
const BOT_FILE: &str = "bot.gd";
const BOT_KEY: &str = "NocticsPlaytestBot";
const OVERRIDE: &str = "override.cfg";
const OVERRIDE_BACKUP: &str = "override.cfg.noctics-backup";
const MANIFEST: &str = "manifest.json";
const VERDICT_FILE: &str = "verdict.json";
const WINDOW: &str = "1280x720";
const SILENT_AUDIO: &str = "Dummy";
const OUT_FLAG: &str = "--noctics-out=";
const SECONDS_FLAG: &str = "--noctics-seconds=";
const ERROR_PREFIXES: [&str; 5] = ["SCRIPT ERROR:", "USER SCRIPT ERROR:", "USER ERROR:", "ERROR:", "Parse Error"];
const LOCATION_PREFIX: &str = "at:";
const MAX_ERRORS: usize = 20;
const MAX_ERROR_CHARS: usize = 300;
const MAX_ISSUES: usize = 12;
const MAX_ISSUE_CHARS: usize = 240;
const MAX_SUMMARY_CHARS: usize = 400;
const REVIEW_ROUNDS: u32 = 3;
const URI_SAFE: &[u8] = b"-_.!~*'()";

pub struct Installed {
    source: PathBuf,
}

impl Installed {
    pub fn new(source: &Path) -> Result<Self, String> {
        uninstall(source);
        let installed = Self {
            source: source.to_path_buf(),
        };
        recipes::write_file(&source.join(BOT_DIR).join(BOT_FILE), BOT_SCRIPT.as_bytes())?;
        let overrides = source.join(OVERRIDE);
        let existing = std::fs::read_to_string(&overrides).ok();
        if existing.is_some() {
            std::fs::copy(&overrides, source.join(OVERRIDE_BACKUP))
                .map_err(|error| format!("Не вдалося зберегти {OVERRIDE}: {error}"))?;
        }
        let mut text = existing.unwrap_or_default();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("\n[autoload]\n\n{BOT_KEY}=\"*res://{BOT_DIR}/{BOT_FILE}\"\n"));
        recipes::write_file(&overrides, text.as_bytes())?;
        Ok(installed)
    }
}

impl Drop for Installed {
    fn drop(&mut self) {
        uninstall(&self.source);
    }
}

pub fn uninstall(source: &Path) {
    let _ = std::fs::remove_dir_all(source.join(BOT_DIR));
    let overrides = source.join(OVERRIDE);
    let backup = source.join(OVERRIDE_BACKUP);
    if backup.is_file() {
        let _ = std::fs::rename(&backup, &overrides);
    } else if std::fs::read_to_string(&overrides).is_ok_and(|text| text.contains(BOT_KEY)) {
        let _ = std::fs::remove_file(&overrides);
    }
}

pub fn import_args(source: &Path) -> Vec<OsString> {
    vec![
        "--headless".into(),
        "--path".into(),
        source.as_os_str().to_owned(),
        "--import".into(),
    ]
}

pub fn play_args(source: &Path, out: &Path, seconds: u32) -> Vec<OsString> {
    let mut target = OsString::from(OUT_FLAG);
    target.push(out.as_os_str());
    vec![
        "--path".into(),
        source.as_os_str().to_owned(),
        "--resolution".into(),
        WINDOW.into(),
        "--audio-driver".into(),
        SILENT_AUDIO.into(),
        "--".into(),
        target,
        format!("{SECONDS_FLAG}{seconds}").into(),
    ]
}

fn import_command(godot: &Path) -> String {
    format!("\"{}\" --headless --path . --import", slashes(godot))
}

fn play_command(godot: &Path, out: &Path, seconds: u32) -> String {
    format!(
        "\"{}\" --path . --resolution {WINDOW} --audio-driver {SILENT_AUDIO} -- {OUT_FLAG}\"{}\" {SECONDS_FLAG}{seconds}",
        slashes(godot),
        slashes(out)
    )
}

fn slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct Manifest {
    #[serde(default)]
    pub shots: Vec<ManifestShot>,
    #[serde(default)]
    pub pressed: Vec<String>,
    #[serde(default)]
    pub scenes: Vec<String>,
    #[serde(default)]
    pub seconds: f64,
    #[serde(default)]
    pub finished: bool,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct ManifestShot {
    pub file: String,
    #[serde(default)]
    pub thumb: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub second: f64,
    #[serde(default)]
    pub scene: String,
    #[serde(default)]
    pub blank: bool,
}

pub fn read_manifest(dir: &Path) -> Option<Manifest> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(MANIFEST)).ok()?).ok()
}

pub fn shots(dir: &Path, manifest: &Manifest, link: impl Fn(&Path) -> String) -> Vec<PlaytestShot> {
    manifest
        .shots
        .iter()
        .filter_map(|shot| {
            let image = inside(dir, &shot.file)?;
            let thumb = inside(dir, &shot.thumb).unwrap_or_else(|| image.clone());
            Some(PlaytestShot {
                label: shot.label.clone(),
                second: whole_seconds(shot.second),
                scene: shot.scene.clone(),
                blank: shot.blank,
                image: link(&image),
                thumb: link(&thumb),
            })
        })
        .collect()
}

fn inside(dir: &Path, name: &str) -> Option<PathBuf> {
    let plain = Path::new(name).file_name().is_some_and(|file| file == name);
    Some(dir.join(name)).filter(|path| plain && path.is_file())
}

fn whole_seconds(seconds: f64) -> u32 {
    if seconds.is_finite() && seconds > 0.0 {
        seconds.round() as u32
    } else {
        0
    }
}

pub fn asset_link(path: &Path) -> String {
    let encoded: String = path
        .to_string_lossy()
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || URI_SAFE.contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect();
    if cfg!(windows) {
        format!("http://asset.localhost/{encoded}")
    } else {
        format!("asset://localhost/{encoded}")
    }
}

#[derive(Default)]
pub struct ErrorLog {
    errors: Vec<String>,
    seen: HashSet<String>,
    attach: bool,
}

impl ErrorLog {
    pub fn push(&mut self, line: &str) {
        let trimmed = line.trim();
        if ERROR_PREFIXES.iter().any(|prefix| trimmed.starts_with(prefix)) {
            let head = clip(trimmed, MAX_ERROR_CHARS);
            self.attach = self.errors.len() < MAX_ERRORS && self.seen.insert(head.clone());
            if self.attach {
                self.errors.push(head);
            }
            return;
        }
        if self.attach
            && trimmed.starts_with(LOCATION_PREFIX)
            && let Some(last) = self.errors.last_mut()
        {
            *last = clip(&format!("{last} · {trimmed}"), MAX_ERROR_CHARS);
        }
        self.attach = false;
    }

    pub fn note(&mut self, text: String) {
        if self.errors.len() < MAX_ERRORS && self.seen.insert(text.clone()) {
            self.errors.push(text);
        }
    }

    pub fn errors(&self) -> Vec<String> {
        self.errors.clone()
    }
}

pub struct Review<'a> {
    pub title: &'a str,
    pub version: String,
    pub godot: &'a Path,
    pub brief: &'a Path,
    pub run: &'a Path,
    pub first: &'a Path,
}

pub fn system_prompt(review: &Review<'_>) -> String {
    let next = review.run.join("round-2");
    let after = review.run.join("round-3");
    format!(
        "Ти — AI-ядро Noctics Factory на етапі «Плейтест». Гру вже написано на Godot {} з GDScript, а бот студії щойно зіграв у неї й зняв скриншоти. \
Твоя задача — подивитися, як гра справді виглядає й грається, знайти, що зламано або не схоже на затверджені макети, і виправити це в коді.\n\n\
Матеріали:\n\
- Поточна папка — корінь гри.\n\
- Матеріали студії в {}: design.md (бриф, палітра, шрифти, список екранів), screens/ (затверджені макети інтерфейсу 1280×720 у HTML), skills/ (дизайн-документ і специфікації).\n\
- Перший прогін бота — у {}: PNG-кадри 1280×720 і {MANIFEST}. У маніфесті: shots — підпис, секунда й сцена кожного кадру, blank — кадр майже одного кольору; \
pressed — що й коли натискав бот; scenes — які сцени відкривались; finished — чи бот дограв до кінця, а не гра закрилась сама.\n\n\
Як грає бот: на 2,5 с тисне в меню кнопку старту (Почати, Грати, Нова гра, Play, Start) або ui_accept, далі по черзі тримає й тисне дії гри з Input Map \
(рух, стрибок, атака тощо), іноді клацає мишею в центр екрана, а наприкінці двічі тисне ui_cancel — пауза й повернення в гру.\n\n\
Порядок роботи:\n\
1. Переглянь кожен кадр через Read і прочитай {MANIFEST}. Порівняй кадри з макетами screens/ і палітрою з design.md.\n\
2. Шукай: чорний або порожній екран, гра не стартує з меню, персонаж чи камера не рухаються між кадрами, елементи за межами екрана чи обрізані, \
нечитабельний або накладений текст, не та палітра чи шрифти, рамки-слоти там, де мав бути арт, немає паузи на ui_cancel, помилки Godot із логу.\n\
3. Виправ знайдене в коді, сценах і ресурсах. Не переписуй гру з нуля й не чіпай те, що працює.\n\
4. Перевір себе прогоном бота: якщо додавав нові файли ассетів, спершу {}, потім {} — і переглянь нові кадри в {}. \
Наступний прогін пиши в {}. Не більше {REVIEW_ROUNDS} прогонів разом із першим.\n\
5. Наприкінці запиши звіт у {} — лише JSON без markdown: \
{{\"verdict\": \"passed\" | \"fixed\" | \"needs_work\", \"summary\": \"одне-два речення українською\", \"issues\": [{{\"text\": \"що було не так, українською\", \"fixed\": true}}]}}. \
passed — проблем не знайдено; fixed — усе важливе знайдено й виправлено; needs_work — лишилось те, що виправити не вдалося.\n\n\
Правила:\n\
- Не змінюй і не видаляй папку {BOT_DIR}/ і файл {OVERRIDE} — це бот студії, студія прибере його сама.\n\
- Не додавай у гру кнопок, дій чи коду лише заради бота.\n\
- Персонажі, предмети й сцени — лише справжній арт, ніколи не фігури з коду.\n\
- Жодних коментарів у коді, без заглушок і TODO.\n\
- Наприкінці коротко напиши, що зроблено.",
        review.version,
        review.brief.display(),
        review.first.display(),
        import_command(review.godot),
        play_command(review.godot, &next, PLAY_SECONDS),
        slashes(&next),
        slashes(&after),
        slashes(&review.run.join(VERDICT_FILE)),
    )
}

pub fn task_message(review: &Review<'_>, manifest: Option<&Manifest>, errors: &[String]) -> String {
    let mut message = match manifest.filter(|manifest| !manifest.shots.is_empty()) {
        Some(manifest) => {
            let blank: Vec<String> = manifest
                .shots
                .iter()
                .filter(|shot| shot.blank)
                .map(|shot| shot.label.clone())
                .collect();
            let mut lines = vec![format!(
                "Плейтест гри «{}». Перший прогін бота: {} кадрів за {} с.",
                review.title,
                manifest.shots.len(),
                whole_seconds(manifest.seconds)
            )];
            if !manifest.scenes.is_empty() {
                lines.push(format!("Сцени: {}.", manifest.scenes.join(" → ")));
            }
            if !manifest.finished {
                lines.push("Гра закрилась сама раніше, ніж бот дограв.".to_owned());
            }
            if !blank.is_empty() {
                lines.push(format!("Майже порожні кадри: {}.", blank.join(", ")));
            }
            lines.join("\n")
        }
        None => format!(
            "Плейтест гри «{}». Бот не зняв жодного кадру: гра не запустилась, впала на старті або не відкрила вікно. \
Знайди причину, виправ і перевір прогоном бота.",
            review.title
        ),
    };
    if !errors.is_empty() {
        message.push_str("\n\nПомилки Godot під час гри:\n");
        message.push_str(
            &errors
                .iter()
                .map(|error| format!("- {error}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    message.push_str(&format!("\n\nПочни з перегляду кадрів у {}.", slashes(review.first)));
    message
}

pub fn read_verdict(run: &Path) -> Option<PlaytestReport> {
    let text = std::fs::read_to_string(run.join(VERDICT_FILE)).ok()?;
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let report: PlaytestReport = serde_json::from_str(text.get(start..=end)?).ok()?;
    Some(PlaytestReport {
        verdict: report.verdict,
        summary: clip(&report.summary, MAX_SUMMARY_CHARS),
        issues: report
            .issues
            .into_iter()
            .filter(|issue| !issue.text.trim().is_empty())
            .take(MAX_ISSUES)
            .map(|issue| PlaytestIssue {
                text: clip(&issue.text, MAX_ISSUE_CHARS),
                fixed: issue.fixed,
            })
            .collect(),
    })
}

pub fn settle(report: Option<PlaytestReport>, last: Option<&Manifest>) -> PlaytestReport {
    let mut report = report.unwrap_or_else(|| PlaytestReport {
        verdict: PlaytestVerdict::NeedsWork,
        summary: "Claude завершив перевірку без звіту — переглянь кадри фінального прогону.".to_owned(),
        issues: Vec::new(),
    });
    if last.is_none_or(|manifest| manifest.shots.is_empty()) {
        report.verdict = PlaytestVerdict::NeedsWork;
        report.issues.push(PlaytestIssue {
            text: "У фінальному прогоні бот не зняв жодного кадру — гра не запускається.".to_owned(),
            fixed: false,
        });
    }
    report
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
    use super::*;

    fn sandbox(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("noctics-playtest-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn installs_the_bot_and_restores_the_project() {
        let game = sandbox("install");
        std::fs::write(game.join(OVERRIDE), "[display]\nwindow/vsync/vsync_mode=0").unwrap();
        {
            let _bot = Installed::new(&game).unwrap();
            let overrides = std::fs::read_to_string(game.join(OVERRIDE)).unwrap();
            assert!(overrides.starts_with("[display]\nwindow/vsync/vsync_mode=0\n"));
            assert!(overrides.contains("NocticsPlaytestBot=\"*res://noctics_playtest/bot.gd\""));
            assert!(game.join(BOT_DIR).join(BOT_FILE).is_file());
        }
        assert_eq!(
            std::fs::read_to_string(game.join(OVERRIDE)).unwrap(),
            "[display]\nwindow/vsync/vsync_mode=0"
        );
        assert!(!game.join(BOT_DIR).exists() && !game.join(OVERRIDE_BACKUP).exists());

        let clean = sandbox("clean");
        drop(Installed::new(&clean).unwrap());
        assert!(!clean.join(OVERRIDE).exists());
        let _ = std::fs::remove_dir_all(&game);
        let _ = std::fs::remove_dir_all(&clean);
    }

    #[test]
    fn reads_what_the_bot_captured() {
        let round = sandbox("round");
        std::fs::write(round.join("01.png"), b"png").unwrap();
        std::fs::write(round.join("01.jpg"), b"jpg").unwrap();
        std::fs::write(
            round.join(MANIFEST),
            r#"{"finished":false,"pressed":["2.5 «Почати гру»"],"scenes":["MainMenu","Level"],"seconds":12.4,
"shots":[{"blank":true,"file":"01.png","label":"Перший екран","scene":"MainMenu","second":1.7,"thumb":"01.jpg"},
{"file":"../secret.png","label":"Чужий"},{"file":"02.png","label":"Зник"}]}"#,
        )
        .unwrap();
        let manifest = read_manifest(&round).unwrap();
        assert_eq!(manifest.scenes, ["MainMenu", "Level"]);
        let shots = shots(&round, &manifest, |path| path.file_name().unwrap().to_string_lossy().into_owned());
        assert_eq!(shots.len(), 1);
        assert_eq!((shots[0].second, shots[0].blank), (2, true));
        assert_eq!((shots[0].image.as_str(), shots[0].thumb.as_str()), ("01.png", "01.jpg"));

        let review = Review {
            title: "Шлях Тіні",
            version: "4.7".into(),
            godot: Path::new("C:\\Godot\\godot.exe"),
            brief: Path::new("brief"),
            run: Path::new("C:\\games\\run"),
            first: &round,
        };
        let task = task_message(&review, Some(&manifest), &["SCRIPT ERROR: boom".into()]);
        assert!(task.contains("Сцени: MainMenu → Level."));
        assert!(task.contains("закрилась сама"));
        assert!(task.contains("Майже порожні кадри: Перший екран."));
        assert!(task.contains("- SCRIPT ERROR: boom"));
        assert!(task_message(&review, None, &[]).contains("не зняв жодного кадру"));
        let system = system_prompt(&review);
        assert!(system.contains("\"C:/Godot/godot.exe\" --path . --resolution 1280x720"));
        assert!(system.contains("--noctics-out=\"C:/games/run/round-2\""));
        assert!(system.contains("C:/games/run/verdict.json"));
        assert!(system.contains("Жодних коментарів"));
        let _ = std::fs::remove_dir_all(&round);
    }

    #[test]
    fn keeps_each_godot_error_once_with_its_location() {
        let mut log = ErrorLog::default();
        for _ in 0..3 {
            log.push("SCRIPT ERROR: Invalid call. Nonexistent function 'jump' in base 'Node2D'.");
            log.push("          at: _process (res://player.gd:12)");
        }
        log.push("Godot Engine v4.7.stable.official");
        log.push("   at: stray location");
        log.note("Гра зависла".into());
        assert_eq!(
            log.errors(),
            [
                "SCRIPT ERROR: Invalid call. Nonexistent function 'jump' in base 'Node2D'. · at: _process (res://player.gd:12)",
                "Гра зависла",
            ]
        );
    }

    #[test]
    fn trusts_the_report_only_when_the_game_still_runs() {
        let run = sandbox("verdict");
        std::fs::write(
            run.join(VERDICT_FILE),
            "```json\n{\"verdict\":\"fixed\",\"summary\":\"Меню не стартувало\",\"issues\":[{\"text\":\"Кнопка «Почати» нічого не робила\",\"fixed\":true},{\"text\":\" \"}]}\n```",
        )
        .unwrap();
        let report = read_verdict(&run).unwrap();
        assert_eq!(report.verdict, PlaytestVerdict::Fixed);
        assert_eq!(report.issues.len(), 1);
        let running = Manifest {
            shots: vec![ManifestShot {
                file: "01.png".into(),
                thumb: String::new(),
                label: String::new(),
                second: 1.0,
                scene: String::new(),
                blank: false,
            }],
            ..Manifest::default()
        };
        assert_eq!(settle(Some(report.clone()), Some(&running)).verdict, PlaytestVerdict::Fixed);
        let broken = settle(Some(report), None);
        assert_eq!(broken.verdict, PlaytestVerdict::NeedsWork);
        assert_eq!(broken.open(), 1);
        assert_eq!(settle(None, Some(&running)).verdict, PlaytestVerdict::NeedsWork);
        let _ = std::fs::remove_dir_all(&run);
    }

    #[test]
    fn links_files_through_the_asset_protocol() {
        let link = asset_link(Path::new("C:\\Ігри\\run 1\\01.jpg"));
        let encoded = "C%3A%5C%D0%86%D0%B3%D1%80%D0%B8%5Crun%201%5C01.jpg";
        if cfg!(windows) {
            assert_eq!(link, format!("http://asset.localhost/{encoded}"));
        } else {
            assert_eq!(link, format!("asset://localhost/{encoded}"));
        }
        let args = play_args(Path::new("game"), Path::new("shots"), 40);
        assert_eq!(args.last().unwrap(), "--noctics-seconds=40");
        assert!(args.iter().any(|arg| arg == "--noctics-out=shots"));
    }
}
