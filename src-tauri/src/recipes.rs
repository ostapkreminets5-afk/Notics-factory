use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use noctics_shared::{BuildTarget, GameEngine, Readiness};
use serde_json::{Value, json};

use crate::process::{self, LineSink, RunFailure};
use crate::toolchains::{self, Host, MAX_GRADLE_JDK, Toolchains};

const MACOS_HOST: &str = "macOS із Xcode";
const SHELL_ICON: &[u8] = include_bytes!("../icons/icon.ico");
const MIN_JDK: u32 = 17;
const CAPACITOR_JDK: u32 = 21;
const CAPACITOR_VERSION: &str = "^7.0.0";
const TAURI_SHELL_CACHE: &str = "tauri-shell-target";
const WEB_OUTPUTS: [&str; 3] = ["dist", "build", "out"];
const SKIPPED_DIRS: [&str; 5] = ["node_modules", ".git", ".noctics", "target", ".godot"];
const MAX_SLUG_CHARS: usize = 32;
const FALLBACK_SLUG: &str = "game";
const TASK_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const SEARCH_DEPTH: usize = 6;
const WASM_BUNDLE: &str = "game";
const GODOT_MOBILE_TEXTURES: &str = "textures/vram_compression/import_etc2_astc";
const GODOT_JAVA_SDK: &str = "export/android/java_sdk_path";
const GODOT_ANDROID_SDK: &str = "export/android/android_sdk_path";
const CARGO_APK_SDK_SECTION: &str = "package.metadata.android.sdk";
const CARGO_APK_SDK_KEY: &str = "target_sdk_version";
const CARGO_APK_KEYSTORE: &str = "CARGO_APK_RELEASE_KEYSTORE";
const CARGO_APK_KEYSTORE_PASSWORD: &str = "CARGO_APK_RELEASE_KEYSTORE_PASSWORD";
const DEBUG_KEYSTORE_PASSWORD: &str = "android";
const UNITY_SCRIPT: &str = "using System;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEditor.Build.Reporting;
using UnityEditor.SceneManagement;
using UnityEngine;

public static class NocticsBuild
{
    const string StarterScene = \"Assets/Scenes/Main.unity\";

    public static void Run()
    {
        var arguments = Environment.GetCommandLineArgs();
        var output = ValueAfter(arguments, \"-nocticsOutput\");
        if (string.IsNullOrEmpty(output))
        {
            Debug.LogError(\"Noctics: no output path\");
            EditorApplication.Exit(2);
            return;
        }
        var scenes = EditorBuildSettings.scenes
            .Where(scene => scene.enabled)
            .Select(scene => scene.path)
            .ToArray();
        if (scenes.Length == 0)
        {
            scenes = AssetDatabase.FindAssets(\"t:Scene\", new[] { \"Assets\" })
                .Select(AssetDatabase.GUIDToAssetPath)
                .ToArray();
        }
        if (scenes.Length == 0)
        {
            scenes = new[] { CreateStarterScene() };
        }
        var report = BuildPipeline.BuildPlayer(new BuildPlayerOptions
        {
            scenes = scenes,
            locationPathName = output,
            target = EditorUserBuildSettings.activeBuildTarget,
            options = BuildOptions.None
        });
        Debug.Log(\"Noctics: build \" + report.summary.result + \", \" + report.summary.totalSize + \" bytes\");
        EditorApplication.Exit(report.summary.result == BuildResult.Succeeded ? 0 : 1);
    }

    static string CreateStarterScene()
    {
        Directory.CreateDirectory(Path.GetDirectoryName(StarterScene));
        var scene = EditorSceneManager.NewScene(NewSceneSetup.DefaultGameObjects, NewSceneMode.Single);
        EditorSceneManager.SaveScene(scene, StarterScene);
        EditorBuildSettings.scenes = new[] { new EditorBuildSettingsScene(StarterScene, true) };
        return StarterScene;
    }

    static string ValueAfter(string[] arguments, string name)
    {
        var index = Array.IndexOf(arguments, name);
        return index >= 0 && index + 1 < arguments.Length ? arguments[index + 1] : null;
    }
}
";

pub type JsonLines = Arc<Mutex<Vec<Value>>>;
pub type Task = Box<dyn FnOnce(&LineSink) -> Result<(), String> + Send>;

pub struct Run {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub env: Vec<(OsString, OsString)>,
    pub json: Option<JsonLines>,
    pub retry: Retry,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    Never,
    OnCrash,
    OnFailure,
}

impl Retry {
    fn applies(self, code: Option<i32>) -> bool {
        match self {
            Self::Never => false,
            Self::OnCrash => code.is_none_or(|code| code < 0),
            Self::OnFailure => true,
        }
    }
}

impl Run {
    fn new(program: &Path, cwd: &Path) -> Self {
        Self {
            program: program.to_path_buf(),
            args: Vec::new(),
            cwd: cwd.to_path_buf(),
            env: Vec::new(),
            json: None,
            retry: Retry::Never,
        }
    }

    fn retrying(mut self, retry: Retry) -> Self {
        self.retry = retry;
        self
    }

    fn arg(mut self, value: impl Into<OsString>) -> Self {
        self.args.push(value.into());
        self
    }

    fn args<I, S>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(values.into_iter().map(Into::into));
        self
    }

    fn env(mut self, key: &str, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    fn collect_json(mut self, sink: &JsonLines) -> Self {
        self.json = Some(Arc::clone(sink));
        self
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args).current_dir(&self.cwd);
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command
    }
}

pub enum Step {
    Run(String, Run),
    Task(String, Task),
}

pub enum Artifact {
    Exact(PathBuf),
    Find { root: PathBuf, suffix: &'static str },
}

pub struct Plan {
    pub toolchain: String,
    pub steps: Vec<Step>,
    pub artifact: Artifact,
}

pub struct GameNames {
    pub title: String,
    pub slug: String,
    pub app_id: String,
}

impl GameNames {
    pub fn new(title: &str) -> Self {
        let slug = slugify(title);
        let compact: String = slug.chars().filter(char::is_ascii_alphanumeric).collect();
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        Self {
            title: if title.is_empty() { slug.clone() } else { title },
            app_id: format!("com.noctics.{compact}"),
            slug,
        }
    }
}

pub struct Context<'a> {
    pub engine: GameEngine,
    pub target: BuildTarget,
    pub source: &'a Path,
    pub output: &'a Path,
    pub work: &'a Path,
    pub cache: &'a Path,
    pub names: &'a GameNames,
    pub tools: &'a Toolchains,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Godot,
    Unity,
    Unreal,
    Web,
    Rust,
    Defold,
}

fn kind(engine: GameEngine) -> Kind {
    match engine {
        GameEngine::Godot => Kind::Godot,
        GameEngine::Unity => Kind::Unity,
        GameEngine::Unreal => Kind::Unreal,
        GameEngine::ThreeJs | GameEngine::BabylonJs | GameEngine::Phaser => Kind::Web,
        GameEngine::Bevy | GameEngine::ThreeD | GameEngine::Penumbra => Kind::Rust,
        GameEngine::Defold => Kind::Defold,
    }
}

pub fn marker_present(engine: GameEngine, source: &Path) -> bool {
    match kind(engine) {
        Kind::Godot => source.join("project.godot").is_file(),
        Kind::Unity => {
            source.join("ProjectSettings").join("ProjectVersion.txt").is_file() && source.join("Assets").is_dir()
        }
        Kind::Unreal => toolchains::uproject(source).is_some(),
        Kind::Web => source.join("package.json").is_file() || source.join("index.html").is_file(),
        Kind::Rust => source.join("Cargo.toml").is_file(),
        Kind::Defold => source.join("game.project").is_file(),
    }
}

pub fn readiness(engine: GameEngine, target: BuildTarget, source: Option<&Path>, tools: &Toolchains) -> Readiness {
    match kind(engine) {
        Kind::Godot => godot_readiness(target, tools),
        Kind::Unity => unity_readiness(target, source, tools),
        Kind::Unreal => unreal_readiness(target, source, tools),
        Kind::Web => web_readiness(target, tools),
        Kind::Rust => rust_readiness(target, source, tools),
        Kind::Defold => defold_readiness(target, source, tools),
    }
}

pub fn plan(context: &Context<'_>) -> Result<Plan, String> {
    let toolchain = match readiness(context.engine, context.target, Some(context.source), context.tools) {
        Readiness::Ready(toolchain) => toolchain,
        Readiness::Missing(what) => return Err(format!("Не вистачає: {what}")),
        Readiness::NeedsHost(host) => return Err(format!("{} збирається лише на {host}", context.target.label())),
        Readiness::Unsupported => {
            return Err(format!(
                "{} не збирає гру під {}",
                context.engine.label(),
                context.target.label()
            ));
        }
    };
    let (steps, artifact) = match kind(context.engine) {
        Kind::Godot => godot_plan(context)?,
        Kind::Unity => unity_plan(context)?,
        Kind::Unreal => unreal_plan(context)?,
        Kind::Web => web_plan(context)?,
        Kind::Rust => rust_plan(context)?,
        Kind::Defold => defold_plan(context)?,
    };
    Ok(Plan {
        toolchain,
        steps,
        artifact,
    })
}

pub enum StepFailure {
    Cancelled,
    Failed(String),
}

pub fn execute(
    steps: Vec<Step>,
    log: &LineSink,
    cancelled: impl Fn() -> bool + Copy,
    timeout: Duration,
) -> Result<(), StepFailure> {
    for step in steps {
        if cancelled() {
            return Err(StepFailure::Cancelled);
        }
        match step {
            Step::Run(label, run) => {
                log(format!("▸ {label}"));
                let mut retry = run.retry;
                loop {
                    let sink = route_json(Arc::clone(log), run.json.clone());
                    match process::stream(run.command(), None, sink, drop, cancelled, timeout) {
                        Ok(status) if status.success() => break,
                        Ok(status) if retry.applies(status.code()) => {
                            retry = Retry::Never;
                            log(format!("↻ {label}: {}, пробуємо ще раз", exit_message(status.code())));
                        }
                        Ok(status) => {
                            return Err(StepFailure::Failed(format!("{label}: {}", exit_message(status.code()))));
                        }
                        Err(RunFailure::Cancelled) => return Err(StepFailure::Cancelled),
                        Err(failure) => {
                            return Err(StepFailure::Failed(format!("{label}: {}", failure_message(&failure))));
                        }
                    }
                }
            }
            Step::Task(label, work) => {
                log(format!("▸ {label}"));
                work(log).map_err(|message| StepFailure::Failed(format!("{label}: {message}")))?;
            }
        }
    }
    Ok(())
}

fn route_json(log: LineSink, json: Option<JsonLines>) -> LineSink {
    let Some(messages) = json else {
        return log;
    };
    Arc::new(move |line: String| {
        if line.starts_with('{')
            && let Ok(message) = serde_json::from_str::<Value>(&line)
            && message.get("reason").is_some()
        {
            if let Ok(mut messages) = messages.lock() {
                messages.push(message);
            }
            return;
        }
        log(line);
    })
}

pub fn locate(artifact: &Artifact) -> Option<PathBuf> {
    match artifact {
        Artifact::Exact(path) => path.exists().then(|| path.clone()),
        Artifact::Find { root, suffix } => find_entry(root, suffix),
    }
}

pub fn size_of(path: &Path) -> u64 {
    let measured = if path.file_name().is_some_and(|name| name == "index.html") {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    tree_size(measured)
}

pub fn slugify(text: &str) -> String {
    let mut slug = String::new();
    for c in text.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if let Some(latin) = transliterate(c) {
            slug.push_str(latin);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let mut slug: String = slug.trim_matches('-').chars().take(MAX_SLUG_CHARS).collect();
    slug = slug.trim_end_matches('-').to_owned();
    if slug.is_empty() {
        return FALLBACK_SLUG.to_owned();
    }
    if slug.starts_with(|c: char| c.is_ascii_digit()) {
        slug = format!("{FALLBACK_SLUG}-{slug}");
    }
    slug
}

fn transliterate(c: char) -> Option<&'static str> {
    Some(match c {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "h",
        'ґ' => "g",
        'д' => "d",
        'е' | 'ё' | 'э' => "e",
        'є' => "ie",
        'ж' => "zh",
        'з' => "z",
        'и' | 'ы' => "y",
        'і' | 'ї' | 'й' => "i",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "kh",
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "shch",
        'ю' => "iu",
        'я' => "ia",
        'ь' | 'ъ' | '\'' | '’' | 'ʼ' => "",
        _ => return None,
    })
}

fn host_of(target: BuildTarget) -> Option<Host> {
    match target {
        BuildTarget::Windows => Some(Host::Windows),
        BuildTarget::Macos => Some(Host::Macos),
        BuildTarget::Linux => Some(Host::Linux),
        _ => None,
    }
}

fn on_this_host(target: BuildTarget) -> bool {
    host_of(target) == Some(Host::CURRENT)
}

fn ios_elsewhere(target: BuildTarget) -> bool {
    target == BuildTarget::Ios && Host::CURRENT != Host::Macos
}

fn verdict(needs: Vec<String>, ready: String) -> Readiness {
    if needs.is_empty() {
        Readiness::Ready(ready)
    } else {
        Readiness::Missing(needs.join(", "))
    }
}

fn jdk_at_least(tools: &Toolchains, major: u32) -> bool {
    tools.jdk.as_ref().is_some_and(|jdk| jdk.major >= major)
}

fn executable_name(names: &GameNames) -> String {
    toolchains::executable(&names.slug)
}

fn godot_readiness(target: BuildTarget, tools: &Toolchains) -> Readiness {
    if ios_elsewhere(target) {
        return Readiness::NeedsHost(MACOS_HOST.into());
    }
    let Some(godot) = &tools.godot else {
        return Readiness::Missing("Godot 4 (godot у PATH або змінна GODOT)".into());
    };
    let mut needs = Vec::new();
    if !godot.templates {
        needs.push(format!(
            "шаблони експорту Godot {} (Editor → Manage Export Templates)",
            godot.short_version()
        ));
    }
    if target == BuildTarget::Android {
        if tools.android_sdk.is_none() {
            needs.push("Android SDK".into());
        }
        if !jdk_at_least(tools, MIN_JDK) {
            needs.push(format!("JDK {MIN_JDK}+"));
        }
    }
    verdict(needs, format!("Godot {}", godot.short_version()))
}

fn godot_plan(context: &Context<'_>) -> Result<(Vec<Step>, Artifact), String> {
    let godot = context.tools.godot.as_ref().ok_or("Godot не знайдено")?;
    let names = context.names;
    let (platform, file) = match context.target {
        BuildTarget::Windows => ("Windows Desktop", format!("{}.exe", names.slug)),
        BuildTarget::Macos => ("macOS", format!("{}.zip", names.slug)),
        BuildTarget::Linux => ("Linux", format!("{}.x86_64", names.slug)),
        BuildTarget::Web => ("Web", "index.html".to_owned()),
        BuildTarget::Android => ("Android", format!("{}.apk", names.slug)),
        BuildTarget::Ios => ("iOS", format!("{}.ipa", names.slug)),
    };
    let artifact = context.output.join(file);
    let presets_path = context.source.join("export_presets.cfg");
    let existing = std::fs::read_to_string(&presets_path).unwrap_or_default();
    let (preset, patched) = ensure_godot_preset(&existing, platform, names);
    let mut steps = Vec::new();
    if let Some(contents) = patched {
        steps.push(write_step(
            format!("Додаємо пресет експорту «{preset}»"),
            presets_path,
            contents.into_bytes(),
        ));
    }
    if matches!(context.target, BuildTarget::Macos | BuildTarget::Android | BuildTarget::Ios) {
        let project_file = context.source.join("project.godot");
        let current = std::fs::read_to_string(&project_file).unwrap_or_default();
        if let Some(updated) = ensure_project_setting(&current, "rendering", GODOT_MOBILE_TEXTURES, "true") {
            steps.push(write_step(
                "Вмикаємо текстури ETC2/ASTC у project.godot".into(),
                project_file,
                updated.into_bytes(),
            ));
        }
    }
    if context.target == BuildTarget::Android
        && let (Some(settings), Some(jdk)) = (godot.settings_file(), &context.tools.jdk)
    {
        let mut paths = vec![(GODOT_JAVA_SDK, jdk.home.clone())];
        paths.extend(context.tools.android_sdk.clone().map(|sdk| (GODOT_ANDROID_SDK, sdk)));
        steps.push(task("Вказуємо Godot шляхи до JDK і Android SDK", move |_| {
            let current = std::fs::read_to_string(&settings).unwrap_or_default();
            let updated = paths.iter().fold(current.clone(), |text, (key, path)| {
                fill_editor_setting(&text, key, &forward_slashes(path)).unwrap_or(text)
            });
            if updated == current {
                return Ok(());
            }
            write_file(&settings, updated.as_bytes())
        }));
    }
    let release_keystore = std::env::var_os("GODOT_ANDROID_KEYSTORE_RELEASE_PATH").is_some();
    let mode = if context.target == BuildTarget::Android && !release_keystore {
        "--export-debug"
    } else {
        "--export-release"
    };
    let mut run = Run::new(&godot.exe, context.source)
        .arg("--headless")
        .arg("--path")
        .arg(context.source)
        .arg(mode)
        .arg(&preset)
        .arg(&artifact)
        .retrying(Retry::OnCrash);
    if context.target == BuildTarget::Android {
        run = android_env(run, context.tools);
    }
    steps.push(Step::Run(
        format!("Godot експортує гру під {}", context.target.label()),
        run,
    ));
    Ok((steps, Artifact::Exact(artifact)))
}

pub fn ensure_godot_preset(existing: &str, platform: &str, names: &GameNames) -> (String, Option<String>) {
    let mut presets: Vec<(usize, String, String)> = Vec::new();
    for line in existing.lines().map(str::trim) {
        if let Some(index) = line
            .strip_prefix("[preset.")
            .and_then(|rest| rest.strip_suffix(']'))
            .and_then(|index| index.parse::<usize>().ok())
        {
            presets.push((index, String::new(), String::new()));
        } else if let Some((_, name, kind)) = presets.last_mut() {
            if let Some(value) = line.strip_prefix("name=") {
                *name = unquote(value);
            } else if let Some(value) = line.strip_prefix("platform=") {
                *kind = unquote(value);
            }
        }
    }
    if let Some((_, name, _)) = presets.iter().find(|(_, _, kind)| kind == platform) {
        return (name.clone(), None);
    }
    let index = presets.iter().map(|(index, _, _)| index + 1).max().unwrap_or(0);
    let name = format!("Noctics {platform}");
    let options = match platform {
        "Windows Desktop" => format!(
            "binary_format/embed_pck=true\napplication/modify_resources=false\napplication/product_name={}\n",
            cfg_string(&names.title)
        ),
        "Linux" => "binary_format/embed_pck=true\n".to_owned(),
        "macOS" | "iOS" => format!("application/bundle_identifier={}\n", cfg_string(&names.app_id)),
        "Web" => "variant/thread_support=false\n".to_owned(),
        "Android" => format!(
            "package/unique_name={}\npackage/name={}\n",
            cfg_string(&names.app_id),
            cfg_string(&names.title)
        ),
        _ => String::new(),
    };
    let mut contents = existing.trim_end().to_owned();
    if !contents.is_empty() {
        contents.push_str("\n\n");
    }
    contents.push_str(&format!(
        "[preset.{index}]\n\nname={}\nplatform={}\nrunnable=true\ndedicated_server=false\ncustom_features=\"\"\n\
export_filter=\"all_resources\"\ninclude_filter=\"\"\nexclude_filter=\"\"\nexport_path=\"\"\n\
encryption_include_filters=\"\"\nencryption_exclude_filters=\"\"\nencrypt_pck=false\nencrypt_directory=false\n\n\
[preset.{index}.options]\n\n{options}",
        cfg_string(&name),
        cfg_string(platform)
    ));
    (name, Some(contents))
}

pub fn ensure_project_setting(text: &str, section: &str, key: &str, value: &str) -> Option<String> {
    let header = format!("[{section}]");
    let entry = format!("{key}={value}");
    let prefix = format!("{key}=");
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let Some(start) = lines.iter().position(|line| line.trim() == header) else {
        return Some(format!("{}\n\n{header}\n\n{entry}\n", text.trim_end()));
    };
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map_or(lines.len(), |offset| start + 1 + offset);
    let existing = lines[start + 1..end]
        .iter()
        .position(|line| line.trim_start().starts_with(&prefix))
        .map(|offset| start + 1 + offset);
    match existing {
        Some(at) if lines[at].trim() == entry => return None,
        Some(at) => lines[at] = entry,
        None => {
            let mut at = end;
            while at > start + 1 && lines[at - 1].trim().is_empty() {
                at -= 1;
            }
            lines.insert(at, entry);
        }
    }
    Some(lines.join("\n") + "\n")
}

pub fn fill_editor_setting(text: &str, key: &str, value: &str) -> Option<String> {
    let quoted = cfg_string(value);
    if text.trim().is_empty() {
        return Some(format!(
            "[gd_resource type=\"EditorSettings\" format=3]\n\n[resource]\n{key} = {quoted}\n"
        ));
    }
    let prefix = format!("{key} = ");
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    match lines.iter().position(|line| line.starts_with(&prefix)) {
        Some(at) if lines[at][prefix.len()..].trim() == "\"\"" => lines[at] = format!("{prefix}{quoted}"),
        Some(_) => return None,
        None => lines.push(format!("{prefix}{quoted}")),
    }
    Some(lines.join("\n") + "\n")
}

fn forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn cfg_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('"').replace("\\\"", "\"").replace("\\\\", "\\")
}

fn unity_module(target: BuildTarget) -> (&'static str, &'static str, &'static str) {
    match target {
        BuildTarget::Windows => ("WindowsStandaloneSupport", "Windows Build Support", "Win64"),
        BuildTarget::Macos => ("MacStandaloneSupport", "Mac Build Support", "OSXUniversal"),
        BuildTarget::Linux => ("LinuxStandaloneSupport", "Linux Build Support", "Linux64"),
        BuildTarget::Web => ("WebGLSupport", "WebGL Build Support", "WebGL"),
        BuildTarget::Android => ("AndroidPlayer", "Android Build Support", "Android"),
        BuildTarget::Ios => ("iOSSupport", "iOS Build Support", "iOS"),
    }
}

fn unity_readiness(target: BuildTarget, source: Option<&Path>, tools: &Toolchains) -> Readiness {
    if ios_elsewhere(target) {
        return Readiness::NeedsHost(MACOS_HOST.into());
    }
    let Some(editor) = tools.unity_for(source) else {
        return Readiness::Missing("Unity Editor (через Unity Hub)".into());
    };
    let (module, label, _) = unity_module(target);
    if !editor.has_module(module) {
        return Readiness::Missing(format!("модуль Unity «{label}» (Unity Hub → Add modules)"));
    }
    Readiness::Ready(format!("Unity {}", editor.version))
}

fn unity_plan(context: &Context<'_>) -> Result<(Vec<Step>, Artifact), String> {
    let editor = context.tools.unity_for(Some(context.source)).ok_or("Unity не знайдено")?;
    let (_, _, build_target) = unity_module(context.target);
    let slug = &context.names.slug;
    let (location, artifact) = match context.target {
        BuildTarget::Windows => {
            let exe = context.output.join(format!("{slug}.exe"));
            (exe.clone(), exe)
        }
        BuildTarget::Macos => {
            let app = context.output.join(format!("{slug}.app"));
            (app.clone(), app)
        }
        BuildTarget::Linux => {
            let binary = context.output.join(format!("{slug}.x86_64"));
            (binary.clone(), binary)
        }
        BuildTarget::Web => {
            let folder = context.output.join("web");
            (folder.clone(), folder.join("index.html"))
        }
        BuildTarget::Android => {
            let apk = context.output.join(format!("{slug}.apk"));
            (apk.clone(), apk)
        }
        BuildTarget::Ios => {
            let xcode = context.output.join("xcode");
            (xcode.clone(), xcode)
        }
    };
    let script = context.source.join("Assets").join("Editor").join("NocticsBuild.cs");
    let steps = vec![
        write_step(
            "Додаємо скрипт збірки Assets/Editor/NocticsBuild.cs".into(),
            script,
            UNITY_SCRIPT.as_bytes().to_vec(),
        ),
        Step::Run(
            format!("Unity збирає гру під {}", context.target.label()),
            Run::new(&editor.exe, context.source)
                .args(["-batchmode", "-nographics", "-quit", "-projectPath"])
                .arg(context.source)
                .args(["-buildTarget", build_target, "-executeMethod", "NocticsBuild.Run", "-nocticsOutput"])
                .arg(&location)
                .args(["-logFile", "-"]),
        ),
    ];
    Ok((steps, Artifact::Exact(artifact)))
}

fn unreal_platform(target: BuildTarget) -> Option<&'static str> {
    match target {
        BuildTarget::Windows => Some("Win64"),
        BuildTarget::Macos => Some("Mac"),
        BuildTarget::Linux => Some("Linux"),
        BuildTarget::Android => Some("Android"),
        BuildTarget::Ios => Some("IOS"),
        BuildTarget::Web => None,
    }
}

fn unreal_readiness(target: BuildTarget, source: Option<&Path>, tools: &Toolchains) -> Readiness {
    match target {
        BuildTarget::Web => return Readiness::Unsupported,
        BuildTarget::Macos | BuildTarget::Ios if Host::CURRENT != Host::Macos => {
            return Readiness::NeedsHost(MACOS_HOST.into());
        }
        BuildTarget::Windows if Host::CURRENT != Host::Windows => {
            return Readiness::NeedsHost(BuildTarget::Windows.label().into());
        }
        _ => {}
    }
    let Some(engine) = tools.unreal_for(source) else {
        return Readiness::Missing("Unreal Engine 5 (Epic Games Launcher)".into());
    };
    let mut needs = Vec::new();
    if target == BuildTarget::Linux && Host::CURRENT == Host::Windows && !tools.linux_cross {
        needs.push("крос-тулчейн Linux для Unreal (LINUX_MULTIARCH_ROOT)".to_owned());
    }
    if target == BuildTarget::Android {
        if tools.android_sdk.is_none() {
            needs.push("Android SDK".into());
        }
        if tools.android_ndk.is_none() {
            needs.push("Android NDK".into());
        }
        if !jdk_at_least(tools, MIN_JDK) {
            needs.push(format!("JDK {MIN_JDK}+"));
        }
    }
    verdict(needs, format!("Unreal Engine {}", engine.version))
}

fn unreal_plan(context: &Context<'_>) -> Result<(Vec<Step>, Artifact), String> {
    let engine = context.tools.unreal_for(Some(context.source)).ok_or("Unreal Engine не знайдено")?;
    let project = toolchains::uproject(context.source).ok_or("У папці гри немає .uproject")?;
    let platform = unreal_platform(context.target).ok_or("Unreal не збирає веб-версію")?;
    let mut run = Run::new(&engine.uat(), context.source)
        .arg("BuildCookRun")
        .arg(joined("-project=", &project))
        .args(["-noP4", "-clientconfig=Shipping", "-build", "-cook", "-allmaps", "-stage", "-pak", "-archive", "-prereqs", "-nodebuginfo", "-utf8output", "-unattended"])
        .arg(format!("-platform={platform}"))
        .arg(joined("-archivedirectory=", context.output));
    if context.target == BuildTarget::Android {
        run = android_env(run.arg("-cookflavor=ASTC"), context.tools);
    }
    let suffix = match context.target {
        BuildTarget::Windows => ".exe",
        BuildTarget::Macos => ".app",
        BuildTarget::Linux => ".sh",
        BuildTarget::Android => ".apk",
        BuildTarget::Ios => ".ipa",
        BuildTarget::Web => ".html",
    };
    let steps = vec![Step::Run(
        format!("Unreal пакує гру під {}", context.target.label()),
        run,
    )];
    Ok((
        steps,
        Artifact::Find {
            root: context.output.to_path_buf(),
            suffix,
        },
    ))
}

fn web_readiness(target: BuildTarget, tools: &Toolchains) -> Readiness {
    if ios_elsewhere(target) {
        return Readiness::NeedsHost(MACOS_HOST.into());
    }
    if tools.npm.is_none() {
        return Readiness::Missing("Node.js з npm".into());
    }
    match target {
        BuildTarget::Web => Readiness::Ready("Node.js · npm".into()),
        BuildTarget::Windows | BuildTarget::Macos | BuildTarget::Linux => {
            if !on_this_host(target) {
                Readiness::NeedsHost(target.label().into())
            } else if tools.cargo.is_none() {
                Readiness::Missing("Rust (cargo) для десктоп-оболонки Tauri".into())
            } else {
                Readiness::Ready("npm · оболонка Tauri".into())
            }
        }
        BuildTarget::Android => {
            let mut needs = Vec::new();
            if !tools
                .jdk
                .as_ref()
                .is_some_and(|jdk| (CAPACITOR_JDK..=MAX_GRADLE_JDK).contains(&jdk.major))
            {
                needs.push(format!("JDK {CAPACITOR_JDK}–{MAX_GRADLE_JDK} для Gradle"));
            }
            if tools.android_sdk.is_none() {
                needs.push("Android SDK".into());
            }
            verdict(needs, "Capacitor · Android SDK".into())
        }
        BuildTarget::Ios => Readiness::Unsupported,
    }
}

fn web_plan(context: &Context<'_>) -> Result<(Vec<Step>, Artifact), String> {
    match context.target {
        BuildTarget::Web => {
            let folder = context.output.join("web");
            let steps = web_bundle_steps(context, &folder)?;
            Ok((steps, Artifact::Exact(folder.join("index.html"))))
        }
        BuildTarget::Windows | BuildTarget::Macos | BuildTarget::Linux => tauri_shell_plan(context),
        BuildTarget::Android => capacitor_plan(context),
        BuildTarget::Ios => Err("iOS для веб-рушіїв поки не підтримується".into()),
    }
}

fn web_bundle_steps(context: &Context<'_>, into: &Path) -> Result<Vec<Step>, String> {
    let npm = context.tools.npm.as_ref().ok_or("npm не знайдено")?;
    let source = context.source.to_path_buf();
    let target = into.to_path_buf();
    let manifest = std::fs::read_to_string(source.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let Some(manifest) = manifest else {
        return Ok(vec![task("Копіюємо статичну гру", move |_| replace_tree(&source, &target))]);
    };
    let mut steps = vec![Step::Run(
        "npm install".into(),
        Run::new(npm, &source).args(["install", "--no-audit", "--no-fund"]),
    )];
    let build_script = manifest["scripts"]["build"].as_str().unwrap_or_default().to_owned();
    if build_script.is_empty() {
        steps.push(task("Копіюємо статичну гру", move |_| replace_tree(&source, &target)));
        return Ok(steps);
    }
    let mut build = Run::new(npm, &source).args(["run", "build"]);
    let uses_vite = ["dependencies", "devDependencies"]
        .iter()
        .any(|section| manifest[*section]["vite"].is_string());
    if uses_vite && build_script.contains("vite build") && !build_script.contains("--base") {
        build = build.args(["--", "--base=./"]);
    }
    steps.push(Step::Run("npm run build".into(), build));
    steps.push(task("Збираємо веб-версію", move |_| {
        let built = WEB_OUTPUTS
            .iter()
            .map(|name| source.join(name))
            .find(|dir| dir.join("index.html").is_file())
            .ok_or_else(|| format!("Після npm run build не знайдено index.html у {}", WEB_OUTPUTS.join(", ")))?;
        replace_tree(&built, &target)
    }));
    Ok(steps)
}

fn tauri_shell_plan(context: &Context<'_>) -> Result<(Vec<Step>, Artifact), String> {
    let cargo = context.tools.cargo.as_ref().ok_or("cargo не знайдено")?;
    let shell = context.work.join("desktop-shell");
    let mut steps = web_bundle_steps(context, &shell.join("web"))?;
    let files = tauri_shell_files(context.names);
    let root = shell.clone();
    steps.push(task("Готуємо десктоп-оболонку Tauri", move |_| {
        for (relative, contents) in files {
            write_file(&root.join(relative), &contents)?;
        }
        Ok(())
    }));
    let cache = context.cache.join(TAURI_SHELL_CACHE);
    steps.push(Step::Run(
        "cargo build --release (оболонка Tauri, перший раз довго)".into(),
        Run::new(cargo, &shell)
            .args(["build", "--release", "--manifest-path"])
            .arg(shell.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &cache),
    ));
    let binary = executable_name(context.names);
    let built = cache.join("release").join(&binary);
    let artifact = context.output.join(&binary);
    let destination = artifact.clone();
    steps.push(task("Копіюємо гру", move |_| copy_file(&built, &destination)));
    Ok((steps, Artifact::Exact(artifact)))
}

fn tauri_shell_files(names: &GameNames) -> Vec<(PathBuf, Vec<u8>)> {
    let manifest = format!(
        "[package]\nname = \"{slug}\"\nversion = \"1.0.0\"\nedition = \"2021\"\n\n\
[[bin]]\nname = \"{slug}\"\npath = \"src/main.rs\"\n\n\
[build-dependencies]\ntauri-build = {{ version = \"2\", features = [] }}\n\n\
[dependencies]\ntauri = {{ version = \"2\", features = [\"custom-protocol\"] }}\n\n\
[profile.release]\ncodegen-units = 1\nlto = true\nopt-level = \"s\"\npanic = \"abort\"\nstrip = true\n\n[workspace]\n",
        slug = names.slug
    );
    let config = json!({
        "productName": names.title,
        "version": "1.0.0",
        "identifier": names.app_id,
        "build": { "frontendDist": "web" },
        "app": {
            "windows": [{
                "title": names.title,
                "width": 1280,
                "height": 720,
                "resizable": true,
                "center": true
            }],
            "security": { "csp": null }
        },
        "bundle": { "active": false, "icon": ["icons/icon.ico"] }
    });
    let main = "#![cfg_attr(not(debug_assertions), windows_subsystem = \"windows\")]\n\n\
fn main() {\n    tauri::Builder::default()\n        .run(tauri::generate_context!())\n        .expect(\"failed to start the game\");\n}\n";
    vec![
        (PathBuf::from("Cargo.toml"), manifest.into_bytes()),
        (PathBuf::from("build.rs"), b"fn main() {\n    tauri_build::build();\n}\n".to_vec()),
        (PathBuf::from("src").join("main.rs"), main.as_bytes().to_vec()),
        (
            PathBuf::from("tauri.conf.json"),
            serde_json::to_vec_pretty(&config).unwrap_or_default(),
        ),
        (PathBuf::from("icons").join("icon.ico"), SHELL_ICON.to_vec()),
    ]
}

fn capacitor_plan(context: &Context<'_>) -> Result<(Vec<Step>, Artifact), String> {
    let npm = context.tools.npm.as_ref().ok_or("npm не знайдено")?;
    let shell = context.work.join("android-shell");
    let mut steps = web_bundle_steps(context, &shell.join("www"))?;
    let names = context.names;
    let package = json!({
        "name": names.slug,
        "private": true,
        "version": "1.0.0",
        "dependencies": {
            "@capacitor/core": CAPACITOR_VERSION,
            "@capacitor/android": CAPACITOR_VERSION
        },
        "devDependencies": { "@capacitor/cli": CAPACITOR_VERSION }
    });
    let config = json!({
        "appId": names.app_id,
        "appName": names.title,
        "webDir": "www"
    });
    let root = shell.clone();
    steps.push(task("Готуємо Android-оболонку Capacitor", move |_| {
        write_file(&root.join("package.json"), &serde_json::to_vec_pretty(&package).unwrap_or_default())?;
        write_file(
            &root.join("capacitor.config.json"),
            &serde_json::to_vec_pretty(&config).unwrap_or_default(),
        )
    }));
    steps.push(Step::Run(
        "npm install (Capacitor)".into(),
        Run::new(npm, &shell).args(["install", "--no-audit", "--no-fund"]),
    ));
    let cap = shell.join("node_modules").join(".bin").join(if cfg!(windows) { "cap.cmd" } else { "cap" });
    let android = shell.join("android");
    if !android.join("app").is_dir() {
        steps.push(Step::Run(
            "Capacitor додає Android-проєкт".into(),
            android_env(Run::new(&cap, &shell).args(["add", "android"]), context.tools),
        ));
    }
    steps.push(Step::Run(
        "Capacitor синхронізує веб-версію".into(),
        android_env(Run::new(&cap, &shell).args(["sync", "android"]), context.tools),
    ));
    let properties = android.join("gradle.properties");
    steps.push(task("Налаштовуємо Gradle", move |_| {
        let current = std::fs::read_to_string(&properties).unwrap_or_default();
        if current.contains("android.overridePathCheck") {
            return Ok(());
        }
        let updated = format!("{}\nandroid.overridePathCheck=true\n", current.trim_end());
        write_file(&properties, updated.as_bytes())
    }));
    let gradle = android.join(if cfg!(windows) { "gradlew.bat" } else { "gradlew" });
    steps.push(Step::Run(
        "Gradle збирає APK".into(),
        android_env(
            Run::new(&gradle, &android).args(["assembleDebug", "--no-daemon", "--console=plain"]),
            context.tools,
        ),
    ));
    let apk = android
        .join("app")
        .join("build")
        .join("outputs")
        .join("apk")
        .join("debug")
        .join("app-debug.apk");
    let artifact = context.output.join(format!("{}.apk", names.slug));
    let destination = artifact.clone();
    steps.push(task("Копіюємо APK", move |log| {
        log("APK підписано ключем налагодження: встановлюється на телефон, для Google Play потрібен власний ключ.".into());
        copy_file(&apk, &destination)
    }));
    Ok((steps, Artifact::Exact(artifact)))
}

fn android_env(mut run: Run, tools: &Toolchains) -> Run {
    if let Some(sdk) = &tools.android_sdk {
        run = run.env("ANDROID_HOME", sdk).env("ANDROID_SDK_ROOT", sdk);
    }
    if let Some(ndk) = &tools.android_ndk {
        run = run.env("ANDROID_NDK_HOME", ndk).env("ANDROID_NDK_ROOT", ndk).env("NDKROOT", ndk);
    }
    if let Some(jdk) = &tools.jdk {
        run = run.env("JAVA_HOME", &jdk.home);
    }
    run
}

fn rust_readiness(target: BuildTarget, source: Option<&Path>, tools: &Toolchains) -> Readiness {
    if ios_elsewhere(target) {
        return Readiness::NeedsHost(MACOS_HOST.into());
    }
    if tools.cargo.is_none() {
        return Readiness::Missing("Rust (cargo через rustup)".into());
    }
    match target {
        BuildTarget::Windows | BuildTarget::Macos | BuildTarget::Linux => {
            if on_this_host(target) {
                Readiness::Ready("Rust · cargo".into())
            } else {
                Readiness::NeedsHost(target.label().into())
            }
        }
        BuildTarget::Web => {
            let mut needs = Vec::new();
            if !tools.wasm_target {
                needs.push("ціль wasm32 (rustup target add wasm32-unknown-unknown)".to_owned());
            }
            let has_page = source.is_some_and(|dir| dir.join("index.html").is_file());
            let packer = match (has_page, &tools.trunk, &tools.wasm_bindgen) {
                (true, Some(_), _) => "trunk",
                (_, _, Some(_)) => "wasm-bindgen",
                (_, _, None) => {
                    needs.push("wasm-bindgen-cli (cargo install wasm-bindgen-cli)".into());
                    "wasm-bindgen"
                }
            };
            verdict(needs, format!("Rust · {packer}"))
        }
        BuildTarget::Android => {
            let mut needs = Vec::new();
            if tools.cargo_apk.is_none() {
                needs.push("cargo-apk (cargo install cargo-apk)".to_owned());
            }
            if !tools.android_target {
                needs.push("ціль Android (rustup target add aarch64-linux-android)".into());
            }
            if let Some(level) = tools.ndk_max_platform
                && !tools.android_platforms.iter().any(|platform| *platform <= level)
            {
                needs.push(format!("Android SDK Platform {level} (SDK Manager) — NDK не знає новіших"));
            }
            if tools.android_sdk.is_none() {
                needs.push("Android SDK".into());
            }
            if tools.android_ndk.is_none() {
                needs.push("Android NDK".into());
            }
            verdict(needs, "Rust · cargo-apk".into())
        }
        BuildTarget::Ios => Readiness::Unsupported,
    }
}

fn rust_plan(context: &Context<'_>) -> Result<(Vec<Step>, Artifact), String> {
    let cargo = context.tools.cargo.as_ref().ok_or("cargo не знайдено")?;
    let source = context.source.to_path_buf();
    let output = context.output.to_path_buf();
    match context.target {
        BuildTarget::Windows | BuildTarget::Macos | BuildTarget::Linux => {
            let messages: JsonLines = Arc::default();
            let artifact = output.join(executable_name(context.names));
            let destination = artifact.clone();
            let collected = Arc::clone(&messages);
            let steps = vec![
                Step::Run(
                    "cargo build --release".into(),
                    Run::new(cargo, &source)
                        .args(["build", "--release", "--message-format=json-render-diagnostics"])
                        .collect_json(&messages),
                ),
                task("Збираємо гру в папку", move |_| {
                    let built = cargo_output(&collected, |path| {
                        path.extension().is_none_or(|extension| extension == "exe")
                    })
                    .ok_or("cargo не повідомив про виконуваний файл гри")?;
                    copy_file(&built, &destination)?;
                    copy_assets(&source, &output)
                }),
            ];
            Ok((steps, Artifact::Exact(artifact)))
        }
        BuildTarget::Web => {
            let folder = output.join("web");
            if source.join("index.html").is_file()
                && let Some(trunk) = &context.tools.trunk
            {
                let steps = vec![Step::Run(
                    "trunk build --release".into(),
                    Run::new(trunk, &source)
                        .args(["build", "--release", "--public-url", "./", "--dist"])
                        .arg(&folder),
                )];
                return Ok((steps, Artifact::Exact(folder.join("index.html"))));
            }
            let bindgen = context.tools.wasm_bindgen.clone().ok_or("wasm-bindgen не знайдено")?;
            let messages: JsonLines = Arc::default();
            let collected = Arc::clone(&messages);
            let title = context.names.title.clone();
            let destination = folder.clone();
            let steps = vec![
                Step::Run(
                    "cargo build --release --target wasm32-unknown-unknown".into(),
                    Run::new(cargo, &source)
                        .args([
                            "build",
                            "--release",
                            "--target",
                            "wasm32-unknown-unknown",
                            "--message-format=json-render-diagnostics",
                        ])
                        .collect_json(&messages),
                ),
                task("wasm-bindgen готує веб-версію", move |log| {
                    let wasm = cargo_output(&collected, |path| {
                        path.extension().is_some_and(|extension| extension == "wasm")
                    })
                    .ok_or("cargo не повідомив про .wasm-файл гри")?;
                    let mut command = Command::new(&bindgen);
                    command
                        .args(["--target", "web", "--no-typescript", "--out-name", WASM_BUNDLE, "--out-dir"])
                        .arg(&destination)
                        .arg(&wasm)
                        .current_dir(&source);
                    run_inside(command, log)?;
                    write_file(&destination.join("index.html"), wasm_page(&title).as_bytes())?;
                    copy_assets(&source, &destination)
                }),
            ];
            Ok((steps, Artifact::Exact(folder.join("index.html"))))
        }
        BuildTarget::Android => {
            let apk_root = source.join("target");
            let artifact = output.join(format!("{}.apk", context.names.slug));
            let destination = artifact.clone();
            let mut steps = Vec::new();
            let manifest_path = source.join("Cargo.toml");
            let manifest = std::fs::read_to_string(&manifest_path).unwrap_or_default();
            if let Some(level) = android_sdk_level(context.tools)
                && let Some(updated) = pin_android_sdk(&manifest, level)
            {
                steps.push(write_step(
                    format!("Вказуємо Android API {level} у Cargo.toml"),
                    manifest_path,
                    updated.into_bytes(),
                ));
            }
            let mut run = android_env(Run::new(cargo, &source).args(["apk", "build", "--release", "--lib"]), context.tools);
            if std::env::var_os(CARGO_APK_KEYSTORE).is_none()
                && let Some(keystore) = toolchains::android_debug_keystore()
            {
                if !keystore.is_file()
                    && let Some(jdk) = context.tools.jdk.clone()
                {
                    let created = keystore.clone();
                    steps.push(task("Створюємо ключ налагодження Android", move |log| {
                        if let Some(parent) = created.parent() {
                            std::fs::create_dir_all(parent).map_err(|error| describe_io("створити папку", parent, &error))?;
                        }
                        let mut command = Command::new(jdk.home.join("bin").join(toolchains::executable("keytool")));
                        command
                            .args(["-genkeypair", "-keystore"])
                            .arg(&created)
                            .args([
                                "-storepass",
                                DEBUG_KEYSTORE_PASSWORD,
                                "-keypass",
                                DEBUG_KEYSTORE_PASSWORD,
                                "-alias",
                                "androiddebugkey",
                                "-keyalg",
                                "RSA",
                                "-validity",
                                "10000",
                                "-dname",
                                "CN=Android Debug,O=Android,C=US",
                            ]);
                        run_inside(command, log)
                    }));
                }
                run = run
                    .env(CARGO_APK_KEYSTORE, &keystore)
                    .env(CARGO_APK_KEYSTORE_PASSWORD, DEBUG_KEYSTORE_PASSWORD);
            }
            steps.push(Step::Run("cargo apk build --release --lib".into(), run));
            steps.push(task("Копіюємо APK", move |_| {
                let apk = find_entry(&apk_root, ".apk").ok_or("cargo-apk не створив .apk")?;
                copy_file(&apk, &destination)
            }));
            Ok((steps, Artifact::Exact(artifact)))
        }
        BuildTarget::Ios => Err("iOS для Rust-рушіїв поки не підтримується".into()),
    }
}

fn android_sdk_level(tools: &Toolchains) -> Option<u32> {
    tools
        .android_platforms
        .iter()
        .copied()
        .filter(|level| tools.ndk_max_platform.is_none_or(|limit| *level <= limit))
        .max()
}

pub fn pin_android_sdk(manifest: &str, level: u32) -> Option<String> {
    if manifest.contains(CARGO_APK_SDK_KEY) {
        return None;
    }
    ensure_project_setting(manifest, CARGO_APK_SDK_SECTION, CARGO_APK_SDK_KEY, &level.to_string())
}

fn cargo_output(messages: &JsonLines, wanted: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let messages = messages.lock().ok()?;
    messages
        .iter()
        .filter(|message| message["reason"] == "compiler-artifact")
        .filter(|message| {
            message["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        })
        .flat_map(|message| {
            let executable = message["executable"].as_str().map(PathBuf::from);
            let files = message["filenames"]
                .as_array()
                .map(|files| files.iter().filter_map(Value::as_str).map(PathBuf::from).collect::<Vec<_>>())
                .unwrap_or_default();
            executable.into_iter().chain(files)
        })
        .filter(|path| wanted(path) && path.is_file())
        .last()
}

fn copy_assets(source: &Path, destination: &Path) -> Result<(), String> {
    let assets = source.join("assets");
    if assets.is_dir() {
        replace_tree(&assets, &destination.join("assets"))
    } else {
        Ok(())
    }
}

fn wasm_page(title: &str) -> String {
    let title = title.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{title}</title>\n\
<style>html,body{{margin:0;height:100%;background:#000;overflow:hidden}}canvas{{display:block;width:100%;height:100%}}</style>\n\
</head>\n<body>\n<script type=\"module\">import init from \"./{WASM_BUNDLE}.js\"; init();</script>\n</body>\n</html>\n"
    )
}

fn defold_platform(target: BuildTarget) -> (&'static str, Option<&'static str>, &'static str) {
    match target {
        BuildTarget::Windows => ("x86_64-win32", None, ".exe"),
        BuildTarget::Macos => ("x86_64-macos", None, ".app"),
        BuildTarget::Linux => ("x86_64-linux", None, ".x86_64"),
        BuildTarget::Web => ("wasm-web", None, "index.html"),
        BuildTarget::Android => ("armv7-android", Some("arm64-android"), ".apk"),
        BuildTarget::Ios => ("arm64-ios", None, ".ipa"),
    }
}

fn defold_readiness(target: BuildTarget, source: Option<&Path>, tools: &Toolchains) -> Readiness {
    if ios_elsewhere(target) {
        return Readiness::NeedsHost(MACOS_HOST.into());
    }
    let mut needs = Vec::new();
    if !jdk_at_least(tools, MIN_JDK) {
        needs.push(format!("JDK {MIN_JDK}+"));
    }
    if tools.bob_for(source).is_none() {
        needs.push("bob.jar (змінна DEFOLD_BOB або bob.jar у папці гри)".into());
    }
    verdict(needs, "Defold · bob.jar".into())
}

fn defold_plan(context: &Context<'_>) -> Result<(Vec<Step>, Artifact), String> {
    let jdk = context.tools.jdk.as_ref().ok_or("JDK не знайдено")?;
    let bob = context.tools.bob_for(Some(context.source)).ok_or("bob.jar не знайдено")?;
    let (platform, architectures, suffix) = defold_platform(context.target);
    let mut run = Run::new(&jdk.java(), context.source)
        .arg("-jar")
        .arg(&bob)
        .args(["--archive", "--variant", "release", "--platform", platform]);
    if let Some(architectures) = architectures {
        run = run.args(["--architectures", architectures]);
    }
    run = run
        .arg("--bundle-output")
        .arg(context.output)
        .args(["resolve", "distclean", "build", "bundle"])
        .env("JAVA_HOME", &jdk.home)
        .retrying(Retry::OnFailure);
    let steps = vec![Step::Run(
        format!("Defold збирає гру під {}", context.target.label()),
        run,
    )];
    Ok((
        steps,
        Artifact::Find {
            root: context.output.to_path_buf(),
            suffix,
        },
    ))
}

fn joined(flag: &str, path: &Path) -> OsString {
    let mut value = OsString::from(flag);
    value.push(path.as_os_str());
    value
}

fn task(label: &str, work: impl FnOnce(&LineSink) -> Result<(), String> + Send + 'static) -> Step {
    Step::Task(label.to_owned(), Box::new(work))
}

fn write_step(label: String, path: PathBuf, contents: Vec<u8>) -> Step {
    Step::Task(label, Box::new(move |_| write_file(&path, &contents)))
}

pub fn write_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| describe_io("створити папку", parent, &error))?;
    }
    std::fs::write(path, contents).map_err(|error| describe_io("записати файл", path, &error))
}

fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|error| describe_io("створити папку", parent, &error))?;
    }
    std::fs::copy(from, to)
        .map(drop)
        .map_err(|error| describe_io("скопіювати", from, &error))
}

pub fn replace_tree(from: &Path, to: &Path) -> Result<(), String> {
    if to.exists() {
        std::fs::remove_dir_all(to).map_err(|error| describe_io("очистити папку", to, &error))?;
    }
    copy_tree(from, to)
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|error| describe_io("створити папку", to, &error))?;
    let entries = std::fs::read_dir(from).map_err(|error| describe_io("прочитати папку", from, &error))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if SKIPPED_DIRS.iter().any(|skipped| name == *skipped) {
            continue;
        }
        let destination = to.join(&name);
        if path.is_dir() {
            copy_tree(&path, &destination)?;
        } else {
            copy_file(&path, &destination)?;
        }
    }
    Ok(())
}

fn describe_io(action: &str, path: &Path, error: &std::io::Error) -> String {
    format!("Не вдалося {action} {}: {error}", path.display())
}

fn find_entry(root: &Path, suffix: &str) -> Option<PathBuf> {
    let suffix = suffix.to_lowercase();
    let mut queue = VecDeque::from([(root.to_path_buf(), 0)]);
    while let Some((dir, depth)) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        children.sort();
        if let Some(found) = children.iter().find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().to_lowercase().ends_with(&suffix))
        }) {
            return Some(found.clone());
        }
        if depth < SEARCH_DEPTH {
            queue.extend(children.into_iter().filter(|path| path.is_dir()).map(|path| (path, depth + 1)));
        }
    }
    None
}

pub fn count_files(root: &Path) -> u32 {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            !SKIPPED_DIRS
                .iter()
                .any(|skipped| entry.file_name().to_string_lossy() == *skipped)
        })
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => count_files(&entry.path()),
            Ok(kind) if kind.is_file() => 1,
            _ => 0,
        })
        .sum()
}

fn tree_size(path: &Path) -> u64 {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !metadata.is_dir() {
        return metadata.len();
    }
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().map(|entry| tree_size(&entry.path())).sum())
        .unwrap_or(0)
}

fn run_inside(command: Command, log: &LineSink) -> Result<(), String> {
    match process::stream(command, None, Arc::clone(log), drop, || false, TASK_TIMEOUT) {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(exit_message(status.code())),
        Err(failure) => Err(failure_message(&failure)),
    }
}

pub fn exit_message(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("процес завершився з кодом {code}"),
        None => "процес зупинено".to_owned(),
    }
}

pub fn failure_message(failure: &RunFailure) -> String {
    match failure {
        RunFailure::Spawn(error) => format!("не вдалося запустити: {error}"),
        RunFailure::Cancelled => "скасовано".into(),
        RunFailure::TimedOut => "перевищено час очікування".into(),
        RunFailure::Wait(error) => format!("процес зник: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_ascii_file_names() {
        assert_eq!(slugify("Нічний Злодій"), "nichnyi-zlodii");
        assert_eq!(slugify("  Neon: Drift 2049!! "), "neon-drift-2049");
        assert_eq!(slugify("2049"), "game-2049");
        assert_eq!(slugify("★★★"), "game");
        let names = GameNames::new("Сяйво  Ліхтарів");
        assert_eq!(names.slug, "siaivo-likhtariv");
        assert_eq!(names.app_id, "com.noctics.siaivolikhtariv");
        assert_eq!(names.title, "Сяйво Ліхтарів");
    }

    #[test]
    fn adds_a_godot_preset_only_when_missing() {
        let names = GameNames::new("Нічний злодій");
        let (name, created) = ensure_godot_preset("", "Windows Desktop", &names);
        assert_eq!(name, "Noctics Windows Desktop");
        let created = created.unwrap();
        assert!(created.starts_with("[preset.0]"));
        assert!(created.contains("platform=\"Windows Desktop\""));
        assert!(created.contains("[preset.0.options]"));
        assert!(created.contains("application/product_name=\"Нічний злодій\""));

        let (same, unchanged) = ensure_godot_preset(&created, "Windows Desktop", &names);
        assert_eq!(same, name);
        assert!(unchanged.is_none());

        let (web, appended) = ensure_godot_preset(&created, "Web", &names);
        assert_eq!(web, "Noctics Web");
        let appended = appended.unwrap();
        assert!(appended.contains("[preset.1]"));
        assert!(appended.contains("[preset.0]"));

        let custom = "[preset.3]\n\nname=\"My PC\"\nplatform=\"Windows Desktop\"\n\n[preset.3.options]\n";
        assert_eq!(ensure_godot_preset(custom, "Windows Desktop", &names).0, "My PC");
        assert!(ensure_godot_preset(custom, "Linux", &names).1.unwrap().contains("[preset.4]"));
    }

    #[test]
    fn pins_the_android_api_only_when_the_game_left_it_open() {
        let manifest = "[package]\nname = \"heist\"\n\n[package.metadata.android]\nbuild_targets = [\"aarch64-linux-android\"]\n";
        let pinned = pin_android_sdk(manifest, 35).unwrap();
        assert!(pinned.ends_with("[package.metadata.android.sdk]\n\ntarget_sdk_version=35\n"));
        assert!(pin_android_sdk(&pinned, 34).is_none());
        let tools = Toolchains {
            android_platforms: vec![34, 35, 36],
            ndk_max_platform: Some(35),
            ..Toolchains::default()
        };
        assert_eq!(android_sdk_level(&tools), Some(35));
    }

    #[test]
    fn retries_only_what_it_was_told_to() {
        const ACCESS_VIOLATION: i32 = -1_073_741_819;
        assert!(Retry::OnCrash.applies(Some(ACCESS_VIOLATION)));
        assert!(Retry::OnCrash.applies(None));
        assert!(!Retry::OnCrash.applies(Some(1)));
        assert!(Retry::OnFailure.applies(Some(1)));
        assert!(!Retry::Never.applies(Some(ACCESS_VIOLATION)));
    }

    #[test]
    fn switches_on_godot_project_settings_in_place() {
        let bare = "config_version=5\n\n[application]\n\nconfig/name=\"Probe\"\n";
        let added = ensure_project_setting(bare, "rendering", GODOT_MOBILE_TEXTURES, "true").unwrap();
        assert!(added.ends_with("[rendering]\n\ntextures/vram_compression/import_etc2_astc=true\n"));
        assert!(ensure_project_setting(&added, "rendering", GODOT_MOBILE_TEXTURES, "true").is_none());

        let rendering = "[rendering]\n\nrenderer/rendering_method=\"mobile\"\n\n[input]\n\njump=1\n";
        let inserted = ensure_project_setting(rendering, "rendering", GODOT_MOBILE_TEXTURES, "true").unwrap();
        assert_eq!(
            inserted,
            "[rendering]\n\nrenderer/rendering_method=\"mobile\"\ntextures/vram_compression/import_etc2_astc=true\n\n[input]\n\njump=1\n"
        );
        let disabled = "[rendering]\ntextures/vram_compression/import_etc2_astc=false\n";
        assert_eq!(
            ensure_project_setting(disabled, "rendering", GODOT_MOBILE_TEXTURES, "true").unwrap(),
            "[rendering]\ntextures/vram_compression/import_etc2_astc=true\n"
        );
    }

    #[test]
    fn fills_only_empty_godot_editor_paths() {
        let settings = "[gd_resource type=\"EditorSettings\" format=3]\n\n[resource]\nexport/android/java_sdk_path = \"\"\nexport/android/android_sdk_path = \"D:/Sdk\"\n";
        let filled = fill_editor_setting(settings, GODOT_JAVA_SDK, "C:/Program Files/Android/Android Studio/jbr").unwrap();
        assert!(filled.contains("export/android/java_sdk_path = \"C:/Program Files/Android/Android Studio/jbr\"\n"));
        assert!(fill_editor_setting(&filled, GODOT_JAVA_SDK, "C:/other").is_none());
        assert!(fill_editor_setting(settings, GODOT_ANDROID_SDK, "C:/Sdk").is_none());
        let fresh = fill_editor_setting("", GODOT_JAVA_SDK, "C:/jdk").unwrap();
        assert_eq!(
            fresh,
            "[gd_resource type=\"EditorSettings\" format=3]\n\n[resource]\nexport/android/java_sdk_path = \"C:/jdk\"\n"
        );
        assert_eq!(forward_slashes(Path::new(r"C:\Users\Yonix\jdk")), "C:/Users/Yonix/jdk");
    }

    #[test]
    fn engines_without_toolchains_explain_what_is_missing() {
        let bare = Toolchains::default();
        assert_eq!(
            readiness(GameEngine::Unreal, BuildTarget::Web, None, &bare),
            Readiness::Unsupported
        );
        assert!(matches!(
            readiness(GameEngine::Godot, BuildTarget::Windows, None, &bare),
            Readiness::Missing(_)
        ));
        assert!(matches!(
            readiness(GameEngine::Phaser, BuildTarget::Web, None, &bare),
            Readiness::Missing(_)
        ));
        let web_ready = Toolchains {
            npm: Some(PathBuf::from("npm")),
            ..Toolchains::default()
        };
        assert!(readiness(GameEngine::ThreeJs, BuildTarget::Web, None, &web_ready).is_ready());
        if Host::CURRENT != Host::Macos {
            assert_eq!(
                readiness(GameEngine::Godot, BuildTarget::Ios, None, &web_ready),
                Readiness::NeedsHost(MACOS_HOST.into())
            );
        }
    }

    #[test]
    fn finds_cargo_binaries_in_build_messages() {
        let dir = std::env::temp_dir().join(format!("noctics-cargo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let binary = dir.join(toolchains::executable("heist"));
        std::fs::write(&binary, b"bin").unwrap();
        let messages: JsonLines = Arc::new(Mutex::new(vec![
            json!({"reason": "compiler-artifact", "target": {"kind": ["lib"]}, "filenames": ["x.rlib"], "executable": null}),
            json!({"reason": "compiler-artifact", "target": {"kind": ["bin"]}, "filenames": [], "executable": binary.to_string_lossy()}),
            json!({"reason": "build-finished", "success": true}),
        ]));
        assert_eq!(cargo_output(&messages, |_| true), Some(binary));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "runs real npm and cargo builds on this machine"]
    fn live_builds_a_web_game_for_this_machine() {
        let root = std::env::temp_dir().join("noctics-live-build");
        let source = root.join("source");
        let _ = std::fs::remove_dir_all(&source);
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("package.json"),
            r#"{"name":"probe","version":"1.0.0","private":true,"scripts":{"build":"node build.mjs"}}"#,
        )
        .unwrap();
        std::fs::write(
            source.join("build.mjs"),
            "import { mkdirSync, copyFileSync } from \"node:fs\";\nmkdirSync(\"dist\", { recursive: true });\ncopyFileSync(\"index.html\", \"dist/index.html\");\n",
        )
        .unwrap();
        std::fs::write(
            source.join("index.html"),
            "<!DOCTYPE html><html><body style=\"background:#0b0e1a;color:#fff\"><h1>Noctics probe</h1></body></html>",
        )
        .unwrap();
        let tools = Toolchains::detect();
        for engine in GameEngine::ALL {
            for target in BuildTarget::ALL {
                println!("{:>12} {:>8} {:?}", engine.label(), target.label(), readiness(engine, target, None, &tools));
            }
        }
        let names = GameNames::new("Нічний злодій");
        let log: LineSink = Arc::new(|line: String| println!("    {line}"));
        let mut targets = vec![BuildTarget::Web];
        targets.extend(host_of(BuildTarget::Windows).filter(|host| *host == Host::CURRENT).map(|_| BuildTarget::Windows));
        for target in targets {
            let output = root.join("builds").join(target.code());
            let _ = std::fs::remove_dir_all(&output);
            std::fs::create_dir_all(&output).unwrap();
            let plan = plan(&Context {
                engine: GameEngine::ThreeJs,
                target,
                source: &source,
                output: &output,
                work: &root.join("work"),
                cache: &root.join("cache"),
                names: &names,
                tools: &tools,
            })
            .unwrap();
            println!("== {} via {}", target.label(), plan.toolchain);
            match execute(plan.steps, &log, || false, Duration::from_secs(60 * 60)) {
                Ok(()) => {}
                Err(StepFailure::Failed(message)) => panic!("{} build failed: {message}", target.label()),
                Err(StepFailure::Cancelled) => panic!("{} build was cancelled", target.label()),
            }
            let artifact = locate(&plan.artifact).expect("artifact is missing");
            println!("== {} → {} ({} bytes)", target.label(), artifact.display(), size_of(&artifact));
        }
    }

    #[test]
    #[ignore = "runs a real Godot export on this machine"]
    fn live_exports_a_godot_game_for_this_machine() {
        export_everywhere(
            GameEngine::Godot,
            "noctics-live-godot",
            &[
                (
                    "project.godot",
                    "config_version=5\n\n[application]\n\nconfig/name=\"Probe\"\nrun/main_scene=\"res://main.tscn\"\n",
                ),
                (
                    "main.tscn",
                    "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n\n[node name=\"Title\" type=\"Label\" parent=\".\"]\ntext = \"Noctics probe\"\n",
                ),
            ],
        );
    }

    #[test]
    #[ignore = "runs a real Defold bob.jar bundle on this machine"]
    fn live_bundles_a_defold_game_for_this_machine() {
        export_everywhere(
            GameEngine::Defold,
            "noctics-live-defold",
            &[
                (
                    "game.project",
                    "[project]\ntitle = Probe\n\n[bootstrap]\nmain_collection = /main/main.collectionc\n",
                ),
                ("main/main.collection", "name: \"main\"\nscale_along_z: 0\n"),
                ("input/game.input_binding", "key_trigger {\n  input: KEY_SPACE\n  action: \"jump\"\n}\n"),
            ],
        );
    }

    #[test]
    #[ignore = "runs real npm, Tauri and Capacitor builds on this machine"]
    fn live_packs_a_web_game_everywhere() {
        export_everywhere(
            GameEngine::Phaser,
            "noctics-live-web",
            &[
                (
                    "package.json",
                    r#"{"name":"probe","version":"1.0.0","private":true,"scripts":{"build":"node build.mjs"}}"#,
                ),
                (
                    "build.mjs",
                    "import { mkdirSync, copyFileSync } from \"node:fs\";\nmkdirSync(\"dist\", { recursive: true });\ncopyFileSync(\"index.html\", \"dist/index.html\");\n",
                ),
                (
                    "index.html",
                    "<!DOCTYPE html><html><body style=\"background:#0b0e1a;color:#fff\"><h1>Noctics probe</h1></body></html>",
                ),
            ],
        );
    }

    #[test]
    #[ignore = "runs real cargo, wasm-bindgen and cargo-apk builds on this machine"]
    fn live_builds_a_rust_game_everywhere() {
        export_everywhere(
            GameEngine::Bevy,
            "noctics-live-rust",
            &[
                (
                    "Cargo.toml",
                    "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
[lib]\ncrate-type = [\"cdylib\", \"rlib\"]\n\n[[bin]]\nname = \"probe\"\npath = \"src/main.rs\"\n\n\
[target.'cfg(target_arch = \"wasm32\")'.dependencies]\nwasm-bindgen = \"=0.2.130\"\n\n\
[target.'cfg(target_os = \"android\")'.dependencies]\nandroid-activity = { version = \"0.6\", features = [\"native-activity\"] }\n\n\
[package.metadata.android]\nbuild_targets = [\"aarch64-linux-android\"]\n\n[workspace]\n",
                ),
                (
                    "src/main.rs",
                    "fn main() {\n    #[cfg(target_arch = \"wasm32\")]\n    let _ = wasm_bindgen::JsValue::from_str(\"Noctics probe\");\n    println!(\"Noctics probe\");\n}\n",
                ),
                (
                    "src/lib.rs",
                    "#[cfg(target_os = \"android\")]\n#[no_mangle]\nfn android_main(_app: android_activity::AndroidApp) {}\n",
                ),
                ("assets/title.txt", "Noctics probe\n"),
            ],
        );
    }

    fn export_everywhere(engine: GameEngine, folder: &str, files: &[(&str, &str)]) {
        let root = std::env::temp_dir().join(folder);
        let source = root.join("source");
        let _ = std::fs::remove_dir_all(&root);
        for (relative, contents) in files {
            write_file(&source.join(relative), contents.as_bytes()).unwrap();
        }
        let tools = Toolchains::detect();
        let names = GameNames::new("Нічний злодій");
        let log: LineSink = Arc::new(|line: String| println!("    {line}"));
        let only = std::env::var("NOCTICS_LIVE_TARGET").ok().and_then(|code| BuildTarget::from_code(&code));
        for target in BuildTarget::ALL.into_iter().filter(|target| only.is_none_or(|only| only == *target)) {
            let readiness = readiness(engine, target, Some(&source), &tools);
            println!("== {} {readiness:?}", target.label());
            if !readiness.is_ready() {
                continue;
            }
            let output = root.join("builds").join(target.code());
            std::fs::create_dir_all(&output).unwrap();
            let plan = plan(&Context {
                engine,
                target,
                source: &source,
                output: &output,
                work: &root.join("work"),
                cache: &root.join("cache"),
                names: &names,
                tools: &tools,
            })
            .unwrap();
            let outcome = execute(plan.steps, &log, || false, Duration::from_secs(20 * 60));
            match (outcome, locate(&plan.artifact)) {
                (Ok(()), Some(artifact)) => {
                    println!("== {} → {} ({} bytes)", target.label(), artifact.display(), size_of(&artifact))
                }
                (Ok(()), None) => println!("== {} finished without an artifact", target.label()),
                (Err(StepFailure::Failed(message)), _) => println!("== {} FAILED: {message}", target.label()),
                (Err(StepFailure::Cancelled), _) => println!("== {} cancelled", target.label()),
            }
        }
    }

    #[test]
    fn locates_artifacts_by_suffix() {
        let dir = std::env::temp_dir().join(format!("noctics-artifact-{}", std::process::id()));
        let nested = dir.join("Windows").join("Heist").join("Binaries");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("Heist-Win64-Shipping.exe"), b"deep").unwrap();
        std::fs::write(dir.join("Windows").join("Heist.exe"), b"top").unwrap();
        let found = locate(&Artifact::Find {
            root: dir.clone(),
            suffix: ".exe",
        });
        assert_eq!(found, Some(dir.join("Windows").join("Heist.exe")));
        assert_eq!(size_of(&dir), 7);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
