use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::process;

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const WASM_TARGET: &str = "wasm32-unknown-unknown";
const ANDROID_TARGET: &str = "aarch64-linux-android";
pub const MIN_GRADLE_JDK: u32 = 17;
pub const MAX_GRADLE_JDK: u32 = 23;
const GODOT_PREFIX: &str = "godot";
const UNREAL_PREFIX: &str = "UE_";
const MIN_GODOT_MAJOR: u32 = 4;
const MAX_GODOT_PROBES: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Host {
    Windows,
    Macos,
    Linux,
}

impl Host {
    pub const CURRENT: Self = if cfg!(windows) {
        Self::Windows
    } else if cfg!(target_os = "macos") {
        Self::Macos
    } else {
        Self::Linux
    };
}

#[derive(Clone, Debug)]
pub struct Godot {
    pub exe: PathBuf,
    pub version: String,
    pub templates: bool,
}

impl Godot {
    pub fn short_version(&self) -> String {
        self.version.split('.').take(2).collect::<Vec<_>>().join(".")
    }

    pub fn settings_file(&self) -> Option<PathBuf> {
        let name = format!("editor_settings-{}.tres", self.short_version());
        let portable = self.exe.parent().map(|dir| dir.join("editor_data")).filter(|dir| {
            dir.parent()
                .is_some_and(|root| root.join("._sc_").is_file() || root.join("_sc_").is_file())
        });
        portable.or_else(godot_settings_dir).map(|dir| dir.join(name))
    }
}

#[derive(Clone, Debug)]
pub struct UnityEditor {
    pub exe: PathBuf,
    pub version: String,
    pub engines: PathBuf,
}

impl UnityEditor {
    pub fn has_module(&self, module: &str) -> bool {
        self.engines.join(module).is_dir()
    }
}

#[derive(Clone, Debug)]
pub struct UnrealEngine {
    pub root: PathBuf,
    pub version: String,
}

impl UnrealEngine {
    pub fn uat(&self) -> PathBuf {
        let script = if cfg!(windows) { "RunUAT.bat" } else { "RunUAT.sh" };
        self.root
            .join("Engine")
            .join("Build")
            .join("BatchFiles")
            .join(script)
    }
}

#[derive(Clone, Debug)]
pub struct Jdk {
    pub home: PathBuf,
    pub major: u32,
}

impl Jdk {
    pub fn java(&self) -> PathBuf {
        self.home.join("bin").join(executable("java"))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Toolchains {
    pub godot: Option<Godot>,
    pub unity: Vec<UnityEditor>,
    pub unreal: Vec<UnrealEngine>,
    pub npm: Option<PathBuf>,
    pub cargo: Option<PathBuf>,
    pub wasm_target: bool,
    pub android_target: bool,
    pub wasm_bindgen: Option<PathBuf>,
    pub trunk: Option<PathBuf>,
    pub cargo_apk: Option<PathBuf>,
    pub jdk: Option<Jdk>,
    pub android_sdk: Option<PathBuf>,
    pub android_ndk: Option<PathBuf>,
    pub android_platforms: Vec<u32>,
    pub ndk_max_platform: Option<u32>,
    pub bob: Option<PathBuf>,
    pub linux_cross: bool,
}

impl Toolchains {
    pub fn detect() -> Self {
        let cargo = cargo_tool("cargo");
        let android_sdk = android_sdk();
        let android_ndk = android_ndk(android_sdk.as_deref());
        let rust_targets = installed_rust_targets();
        Self {
            android_platforms: android_sdk.as_deref().map(android_platforms).unwrap_or_default(),
            ndk_max_platform: android_ndk.as_deref().and_then(ndk_max_platform),
            godot: find_godot(),
            unity: find_unity(),
            unreal: find_unreal(),
            npm: on_path("npm"),
            wasm_target: rust_targets.iter().any(|target| target == WASM_TARGET),
            android_target: rust_targets.iter().any(|target| target == ANDROID_TARGET),
            wasm_bindgen: cargo_tool("wasm-bindgen"),
            trunk: cargo_tool("trunk"),
            cargo_apk: cargo_tool("cargo-apk"),
            jdk: find_jdk(),
            android_ndk,
            android_sdk,
            bob: find_bob(),
            linux_cross: env_dir("LINUX_MULTIARCH_ROOT").is_some(),
            cargo,
        }
    }

    pub fn unity_for(&self, source: Option<&Path>) -> Option<&UnityEditor> {
        let wanted = source.and_then(unity_project_version);
        wanted
            .and_then(|version| self.unity.iter().find(|editor| editor.version == version))
            .or_else(|| self.unity.first())
    }

    pub fn unreal_for(&self, source: Option<&Path>) -> Option<&UnrealEngine> {
        let wanted = source.and_then(unreal_association);
        wanted
            .and_then(|version| {
                self.unreal
                    .iter()
                    .find(|engine| engine.version == version || engine.version.starts_with(&format!("{version}.")))
            })
            .or_else(|| self.unreal.first())
    }

    pub fn bob_for(&self, source: Option<&Path>) -> Option<PathBuf> {
        source
            .map(|dir| dir.join("bob.jar"))
            .filter(|jar| jar.is_file())
            .or_else(|| self.bob.clone())
    }
}

pub fn executable(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

pub fn on_path(name: &str) -> Option<PathBuf> {
    let suffixes: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    path_dirs().into_iter().find_map(|dir| {
        suffixes
            .iter()
            .map(|suffix| dir.join(format!("{name}{suffix}")))
            .find(|candidate| candidate.is_file())
    })
}

pub fn cargo_tool(name: &str) -> Option<PathBuf> {
    on_path(name).or_else(|| {
        home()
            .map(|home| home.join(".cargo").join("bin").join(executable(name)))
            .filter(|candidate| candidate.is_file())
    })
}

pub fn compare_versions(left: &str, right: &str) -> Ordering {
    version_key(left).cmp(&version_key(right))
}

pub fn unity_project_version(source: &Path) -> Option<String> {
    let text = std::fs::read_to_string(source.join("ProjectSettings").join("ProjectVersion.txt")).ok()?;
    text.lines()
        .find_map(|line| line.trim().strip_prefix("m_EditorVersion:"))
        .map(|version| version.trim().to_owned())
        .filter(|version| !version.is_empty())
}

pub fn uproject(source: &Path) -> Option<PathBuf> {
    std::fs::read_dir(source)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("uproject")))
}

fn unreal_association(source: &Path) -> Option<String> {
    let text = std::fs::read_to_string(uproject(source)?).ok()?;
    let project: Value = serde_json::from_str(&text).ok()?;
    project["EngineAssociation"]
        .as_str()
        .map(str::to_owned)
        .filter(|version| !version.is_empty())
}

fn version_key(text: &str) -> Vec<u64> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_default()
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn env_dir(name: &str) -> Option<PathBuf> {
    env_path(name).filter(|path| path.is_dir())
}

fn home() -> Option<PathBuf> {
    env_path("USERPROFILE").or_else(|| env_path("HOME"))
}

fn program_files() -> PathBuf {
    env_path("ProgramFiles").unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default()
}

fn file_name_lower(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn find_godot() -> Option<Godot> {
    godot_candidates()
        .into_iter()
        .take(MAX_GODOT_PROBES)
        .filter_map(probe_godot)
        .find(|godot| version_key(&godot.version).first().copied().unwrap_or(0) >= u64::from(MIN_GODOT_MAJOR))
}

fn godot_candidates() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = ["GODOT", "GODOT_BIN", "GODOT4", "GODOT_PATH"]
        .iter()
        .filter_map(|name| env_path(name))
        .filter(|path| path.is_file())
        .collect();
    found.extend(["godot", "godot4"].iter().filter_map(|name| on_path(name)));
    if cfg!(target_os = "macos") {
        let applications = [PathBuf::from("/Applications"), home().unwrap_or_default().join("Applications")];
        for root in applications {
            for bundle in ["Godot.app", "Godot_mono.app"] {
                let exe = root.join(bundle).join("Contents").join("MacOS").join("Godot");
                if exe.is_file() {
                    found.push(exe);
                }
            }
        }
    }
    if cfg!(windows) {
        let mut roots = path_dirs();
        let local = env_path("LOCALAPPDATA").unwrap_or_default();
        let user = home().unwrap_or_default();
        roots.extend([
            local.join("Programs").join("Godot"),
            program_files().join("Godot"),
            program_files().join("Godot Engine"),
            user.join("Godot"),
            user.join("Desktop"),
            user.join("Downloads"),
            user.join("Documents"),
            user.join("scoop").join("apps").join("godot").join("current"),
            PathBuf::from(r"C:\Godot"),
            env_path("ProgramFiles(x86)")
                .unwrap_or_default()
                .join("Steam")
                .join("steamapps")
                .join("common")
                .join("Godot Engine"),
        ]);
        let winget = local.join("Microsoft").join("WinGet").join("Packages");
        roots.extend(
            subdirs(&winget)
                .into_iter()
                .filter(|dir| file_name_lower(dir).starts_with("godotengine")),
        );
        let nested: Vec<PathBuf> = roots
            .iter()
            .flat_map(|root| subdirs(root))
            .filter(|dir| file_name_lower(dir).starts_with(GODOT_PREFIX))
            .collect();
        roots.extend(nested);
        let mut scanned: Vec<PathBuf> = roots
            .iter()
            .flat_map(|root| std::fs::read_dir(root).into_iter().flatten().flatten())
            .map(|entry| entry.path())
            .filter(|path| {
                let name = file_name_lower(path);
                path.is_file() && name.starts_with(GODOT_PREFIX) && name.ends_with(".exe")
            })
            .collect();
        scanned.sort_by_key(|path| !file_name_lower(path).contains("console"));
        found.extend(scanned);
    }
    let mut unique = Vec::with_capacity(found.len());
    for path in found {
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    unique
}

fn probe_godot(exe: PathBuf) -> Option<Godot> {
    let output = process::capture(&exe, &["--version"], PROBE_TIMEOUT)?;
    let version = output
        .lines()
        .map(str::trim)
        .filter(|line| line.chars().next().is_some_and(|c| c.is_ascii_digit()) && line.contains('.'))
        .last()?
        .to_owned();
    let templates = godot_template_roots(&exe)
        .iter()
        .flat_map(|root| subdirs(root))
        .any(|dir| {
            let name = dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            let matches = version == name || version.starts_with(&format!("{name}."));
            matches && std::fs::read_dir(&dir).is_ok_and(|mut entries| entries.next().is_some())
        });
    Some(Godot {
        exe,
        version,
        templates,
    })
}

fn godot_template_roots(exe: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(dir) = exe.parent() {
        roots.push(dir.join("editor_data").join("export_templates"));
    }
    roots.extend(godot_user_dir("XDG_DATA_HOME", &[".local", "share"]).map(|dir| dir.join("export_templates")));
    roots
}

fn godot_settings_dir() -> Option<PathBuf> {
    godot_user_dir("XDG_CONFIG_HOME", &[".config"])
}

fn godot_user_dir(xdg_variable: &str, xdg_fallback: &[&str]) -> Option<PathBuf> {
    let user = home().unwrap_or_default();
    if cfg!(windows) {
        env_path("APPDATA").map(|dir| dir.join("Godot"))
    } else if cfg!(target_os = "macos") {
        Some(user.join("Library").join("Application Support").join("Godot"))
    } else {
        let base = env_path(xdg_variable)
            .unwrap_or_else(|| xdg_fallback.iter().fold(user, |path, part| path.join(part)));
        Some(base.join("godot"))
    }
}

fn find_unity() -> Vec<UnityEditor> {
    let mut editors = Vec::new();
    if cfg!(windows) {
        let mut roots = vec![program_files().join("Unity").join("Hub").join("Editor")];
        let secondary = env_path("APPDATA")
            .map(|dir| dir.join("UnityHub").join("secondaryInstallPath.json"))
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| serde_json::from_str::<String>(text.trim()).ok())
            .filter(|path| !path.is_empty());
        roots.extend(secondary.map(PathBuf::from));
        for root in roots {
            for dir in subdirs(&root) {
                let editor = dir.join("Editor");
                editors.push(UnityEditor {
                    exe: editor.join("Unity.exe"),
                    version: dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
                    engines: editor.join("Data").join("PlaybackEngines"),
                });
            }
        }
        let single = program_files().join("Unity").join("Editor");
        editors.push(UnityEditor {
            exe: single.join("Unity.exe"),
            version: "Unity".into(),
            engines: single.join("Data").join("PlaybackEngines"),
        });
    } else if cfg!(target_os = "macos") {
        for dir in subdirs(Path::new("/Applications/Unity/Hub/Editor")) {
            editors.push(UnityEditor {
                exe: dir.join("Unity.app").join("Contents").join("MacOS").join("Unity"),
                version: dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
                engines: dir.join("PlaybackEngines"),
            });
        }
    } else if let Some(user) = home() {
        for dir in subdirs(&user.join("Unity").join("Hub").join("Editor")) {
            let editor = dir.join("Editor");
            editors.push(UnityEditor {
                exe: editor.join("Unity"),
                version: dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
                engines: editor.join("Data").join("PlaybackEngines"),
            });
        }
    }
    editors.retain(|editor| editor.exe.is_file());
    editors.sort_by(|left, right| compare_versions(&right.version, &left.version));
    editors
}

fn find_unreal() -> Vec<UnrealEngine> {
    let mut engines = Vec::new();
    let manifest = if cfg!(windows) {
        env_path("ProgramData").map(|dir| dir.join("Epic").join("UnrealEngineLauncher").join("LauncherInstalled.dat"))
    } else {
        home().map(|user| {
            user.join("Library")
                .join("Application Support")
                .join("Epic")
                .join("UnrealEngineLauncher")
                .join("LauncherInstalled.dat")
        })
    };
    let installed = manifest
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    if let Some(list) = installed.as_ref().and_then(|value| value["InstallationList"].as_array()) {
        for entry in list {
            let (Some(name), Some(location)) = (entry["AppName"].as_str(), entry["InstallLocation"].as_str()) else {
                continue;
            };
            if let Some(version) = name.strip_prefix(UNREAL_PREFIX) {
                engines.push(UnrealEngine {
                    root: PathBuf::from(location),
                    version: version.to_owned(),
                });
            }
        }
    }
    let epic = if cfg!(windows) {
        program_files().join("Epic Games")
    } else {
        PathBuf::from("/Users/Shared/Epic Games")
    };
    for dir in subdirs(&epic) {
        let name = dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(version) = name.strip_prefix(UNREAL_PREFIX) {
            engines.push(UnrealEngine {
                root: dir.clone(),
                version: version.to_owned(),
            });
        }
    }
    for variable in ["UE_ROOT", "UNREAL_ENGINE_ROOT"] {
        if let Some(root) = env_dir(variable) {
            let version = unreal_build_version(&root).unwrap_or_else(|| "5".into());
            engines.push(UnrealEngine { root, version });
        }
    }
    engines.retain(|engine| engine.uat().is_file());
    engines.sort_by(|left, right| compare_versions(&right.version, &left.version));
    engines.dedup_by(|left, right| left.root == right.root);
    engines
}

fn unreal_build_version(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("Engine").join("Build").join("Build.version")).ok()?;
    let build: Value = serde_json::from_str(&text).ok()?;
    Some(format!(
        "{}.{}",
        build["MajorVersion"].as_u64()?,
        build["MinorVersion"].as_u64()?
    ))
}

fn installed_rust_targets() -> Vec<String> {
    cargo_tool("rustup")
        .and_then(|rustup| process::capture(&rustup, &["target", "list", "--installed"], PROBE_TIMEOUT))
        .map(|output| output.lines().map(|line| line.trim().to_owned()).filter(|line| !line.is_empty()).collect())
        .unwrap_or_default()
}

pub fn android_debug_keystore() -> Option<PathBuf> {
    home().map(|user| user.join(".android").join("debug.keystore"))
}

pub fn android_platforms(sdk: &Path) -> Vec<u32> {
    let mut levels: Vec<u32> = subdirs(&sdk.join("platforms"))
        .iter()
        .filter_map(|dir| {
            file_name_lower(dir)
                .strip_prefix("android-")
                .and_then(|level| level.parse().ok())
        })
        .collect();
    levels.sort_unstable();
    levels
}

fn ndk_max_platform(ndk: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(ndk.join("build").join("core").join("platforms.mk")).ok()?;
    text.lines()
        .find_map(|line| line.trim().strip_prefix("NDK_MAX_PLATFORM_LEVEL"))
        .and_then(|rest| rest.trim_start_matches([' ', ':', '=']).trim().parse().ok())
}

fn find_jdk() -> Option<Jdk> {
    let mut homes: Vec<PathBuf> = env_path("JAVA_HOME").into_iter().collect();
    homes.extend(home().map(|user| subdirs(&user.join(".jdks"))).unwrap_or_default());
    if cfg!(windows) {
        let local = env_path("LOCALAPPDATA").unwrap_or_default();
        homes.push(program_files().join("Android").join("Android Studio").join("jbr"));
        homes.push(local.join("Programs").join("Android Studio").join("jbr"));
        for vendor in ["Java", "Eclipse Adoptium", "Microsoft", "Zulu", "Amazon Corretto", "BellSoft"] {
            homes.extend(subdirs(&program_files().join(vendor)));
        }
    } else if cfg!(target_os = "macos") {
        homes.push(PathBuf::from("/Applications/Android Studio.app/Contents/jbr/Contents/Home"));
        homes.extend(
            subdirs(Path::new("/Library/Java/JavaVirtualMachines"))
                .into_iter()
                .map(|dir| dir.join("Contents").join("Home")),
        );
    } else {
        homes.extend(subdirs(Path::new("/usr/lib/jvm")));
        homes.extend(home().map(|user| user.join("android-studio").join("jbr")));
    }
    let found: Vec<Jdk> = homes
        .into_iter()
        .filter_map(|home| {
            let major = jdk_major(&home)?;
            let jdk = Jdk { home, major };
            jdk.java().is_file().then_some(jdk)
        })
        .collect();
    preferred_jdk(found)
}

fn preferred_jdk(found: Vec<Jdk>) -> Option<Jdk> {
    let gradle_friendly = found
        .iter()
        .filter(|jdk| (MIN_GRADLE_JDK..=MAX_GRADLE_JDK).contains(&jdk.major))
        .max_by_key(|jdk| jdk.major)
        .cloned();
    gradle_friendly.or_else(|| found.into_iter().max_by_key(|jdk| jdk.major))
}

fn jdk_major(home: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(home.join("release")).ok()?;
    let version = text
        .lines()
        .find_map(|line| line.strip_prefix("JAVA_VERSION="))?
        .trim_matches('"');
    let mut parts = version.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

fn android_sdk() -> Option<PathBuf> {
    let user = home().unwrap_or_default();
    let defaults = if cfg!(windows) {
        env_path("LOCALAPPDATA").map(|dir| dir.join("Android").join("Sdk"))
    } else if cfg!(target_os = "macos") {
        Some(user.join("Library").join("Android").join("sdk"))
    } else {
        Some(user.join("Android").join("Sdk"))
    };
    ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .filter_map(|name| env_path(name))
        .chain(defaults)
        .find(|dir| dir.join("platforms").is_dir() || dir.join("platform-tools").is_dir())
}

fn android_ndk(sdk: Option<&Path>) -> Option<PathBuf> {
    let from_env = ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT", "NDK_ROOT", "NDKROOT"]
        .iter()
        .find_map(|name| env_dir(name));
    from_env.or_else(|| {
        let sdk = sdk?;
        let mut versions = subdirs(&sdk.join("ndk"));
        versions.sort_by(|left, right| {
            compare_versions(&file_name_lower(right), &file_name_lower(left))
        });
        versions
            .into_iter()
            .next()
            .or_else(|| Some(sdk.join("ndk-bundle")).filter(|dir| dir.is_dir()))
    })
}

fn find_bob() -> Option<PathBuf> {
    let user = home().unwrap_or_default();
    ["DEFOLD_BOB", "BOB_JAR"]
        .iter()
        .filter_map(|name| env_path(name))
        .chain([
            user.join(".defold").join("bob.jar"),
            user.join("Defold").join("bob.jar"),
            user.join("Downloads").join("bob.jar"),
        ])
        .find(|jar| jar.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_engine_versions_numerically() {
        assert_eq!(compare_versions("2022.3.10f1", "2022.3.9f1"), Ordering::Greater);
        assert_eq!(compare_versions("5.4", "5.10"), Ordering::Less);
        assert_eq!(compare_versions("4.3.stable", "4.3.stable"), Ordering::Equal);
    }

    #[test]
    fn reads_project_versions() {
        let dir = std::env::temp_dir().join(format!("noctics-toolchains-{}", std::process::id()));
        let settings = dir.join("ProjectSettings");
        std::fs::create_dir_all(&settings).unwrap();
        std::fs::write(
            settings.join("ProjectVersion.txt"),
            "m_EditorVersion: 2022.3.10f1\nm_EditorVersionWithRevision: 2022.3.10f1 (ff3792e53c62)\n",
        )
        .unwrap();
        std::fs::write(dir.join("Heist.uproject"), r#"{"FileVersion":3,"EngineAssociation":"5.4"}"#).unwrap();
        assert_eq!(unity_project_version(&dir).as_deref(), Some("2022.3.10f1"));
        assert_eq!(unreal_association(&dir).as_deref(), Some("5.4"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefers_a_jdk_that_gradle_can_run() {
        let jdk = |major| Jdk {
            home: PathBuf::from(format!("jdk-{major}")),
            major,
        };
        assert_eq!(preferred_jdk(vec![jdk(25), jdk(21), jdk(17)]).unwrap().major, 21);
        assert_eq!(preferred_jdk(vec![jdk(25)]).unwrap().major, 25);
        assert_eq!(preferred_jdk(vec![jdk(11), jdk(17)]).unwrap().major, 17);
        assert!(preferred_jdk(Vec::new()).is_none());
    }

    #[test]
    fn reads_android_platform_levels() {
        let dir = std::env::temp_dir().join(format!("noctics-sdk-{}", std::process::id()));
        for platform in ["android-36", "android-35", "android-36.1", "android-VanillaIceCream"] {
            std::fs::create_dir_all(dir.join("platforms").join(platform)).unwrap();
        }
        let core = dir.join("ndk").join("build").join("core");
        std::fs::create_dir_all(&core).unwrap();
        std::fs::write(core.join("platforms.mk"), "NDK_MIN_PLATFORM_LEVEL := 21\nNDK_MAX_PLATFORM_LEVEL := 35\n").unwrap();
        assert_eq!(android_platforms(&dir), [35, 36]);
        assert_eq!(ndk_max_platform(&dir.join("ndk")), Some(35));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_jdk_release_files() {
        let dir = std::env::temp_dir().join(format!("noctics-jdk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("release"), "IMPLEMENTOR=\"JetBrains\"\nJAVA_VERSION=\"21.0.3\"\n").unwrap();
        assert_eq!(jdk_major(&dir), Some(21));
        std::fs::write(dir.join("release"), "JAVA_VERSION=\"1.8.0_402\"\n").unwrap();
        assert_eq!(jdk_major(&dir), Some(8));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
