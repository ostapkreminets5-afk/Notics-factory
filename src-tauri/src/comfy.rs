use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};
use url::Url;

pub const COMFY_URL: &str = "http://127.0.0.1:8188";
const COMFY_HOST: &str = "127.0.0.1";
const COMFY_PORT: &str = "8188";
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_JSON_BYTES: u64 = 96 * 1024 * 1024;
const CLIENT_ID: &str = "noctics-factory";
const OUTPUT_TYPE: &str = "output";
const MAX_ERROR_CHARS: usize = 400;
const MODEL_EXTENSIONS: [&str; 7] = ["safetensors", "ckpt", "pt", "pth", "bin", "gguf", "sft"];
const MAIN_SCRIPT: &str = "main.py";
const MODELS_DIR: &str = "models";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Folder {
    Checkpoints,
    DiffusionModels,
    TextEncoders,
    Vae,
    ClipVision,
    BackgroundRemoval,
    Geometry,
    Loras,
    UltraTex,
}

impl Folder {
    const ALL: [Self; 9] = [
        Self::Checkpoints,
        Self::DiffusionModels,
        Self::TextEncoders,
        Self::Vae,
        Self::ClipVision,
        Self::BackgroundRemoval,
        Self::Geometry,
        Self::Loras,
        Self::UltraTex,
    ];

    fn loader(self) -> (&'static str, &'static str) {
        match self {
            Self::Checkpoints => ("CheckpointLoaderSimple", "ckpt_name"),
            Self::DiffusionModels => ("UNETLoader", "unet_name"),
            Self::TextEncoders => ("CLIPLoader", "clip_name"),
            Self::Vae => ("VAELoader", "vae_name"),
            Self::ClipVision => ("CLIPVisionLoader", "clip_name"),
            Self::BackgroundRemoval => ("LoadBackgroundRemovalModel", "bg_removal_name"),
            Self::Geometry => ("LoadMoGeModel", "model_name"),
            Self::Loras => ("LoraLoaderModelOnly", "lora_name"),
            Self::UltraTex => ("UltraTexForegroundDecoder", "decoder_name"),
        }
    }

    fn directories(self) -> &'static [&'static str] {
        match self {
            Self::Checkpoints => &["checkpoints"],
            Self::DiffusionModels => &["diffusion_models", "unet"],
            Self::TextEncoders => &["text_encoders", "clip"],
            Self::Vae => &["vae"],
            Self::ClipVision => &["clip_vision"],
            Self::BackgroundRemoval => &["background_removal"],
            Self::Geometry => &["geometry_estimation"],
            Self::Loras => &["loras"],
            Self::UltraTex => &["ultratex"],
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    pub files: BTreeMap<Folder, Vec<String>>,
    pub nodes: Option<BTreeSet<String>>,
}

impl Inventory {
    pub fn in_folder(&self, folder: Folder) -> &[String] {
        self.files.get(&folder).map_or(&[], Vec::as_slice)
    }

    pub fn has_node(&self, class: &str) -> bool {
        self.nodes.as_ref().is_none_or(|nodes| nodes.contains(class))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    pub version: Option<String>,
    pub gpu: Option<String>,
    pub vram_mb: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OutputRef {
    pub filename: String,
    pub subfolder: String,
}

impl OutputRef {
    pub fn annotated(&self) -> String {
        format!("{} [output]", self.relative())
    }

    pub fn relative(&self) -> String {
        if self.subfolder.is_empty() {
            self.filename.clone()
        } else {
            format!("{}/{}", self.subfolder, self.filename)
        }
    }

    pub fn extension(&self) -> String {
        Path::new(&self.filename)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default()
    }
}

pub enum Progress {
    Waiting,
    Finished(Vec<OutputRef>),
    Failed(String),
}

pub struct ComfyClient {
    base: Url,
    quick: ureq::Agent,
    agent: ureq::Agent,
    downloads: ureq::Agent,
}

impl ComfyClient {
    pub fn new(base: &str) -> Self {
        let build = |timeout: Duration| -> ureq::Agent {
            ureq::Agent::config_builder()
                .timeout_global(Some(timeout))
                .http_status_as_error(false)
                .build()
                .into()
        };
        Self {
            base: Url::parse(base).unwrap_or_else(|_| Url::parse(COMFY_URL).expect("valid ComfyUI url")),
            quick: build(PROBE_TIMEOUT),
            agent: build(REQUEST_TIMEOUT),
            downloads: build(DOWNLOAD_TIMEOUT),
        }
    }

    pub fn url(&self) -> String {
        self.base.as_str().trim_end_matches('/').to_owned()
    }

    fn endpoint(&self, path: &str) -> Url {
        let mut url = self.base.clone();
        url.set_path(path);
        url
    }

    fn get_json(&self, agent: &ureq::Agent, url: &Url) -> Option<Value> {
        let mut response = agent.get(url.as_str()).call().ok()?;
        if !response.status().is_success() {
            return None;
        }
        let text = response
            .body_mut()
            .with_config()
            .limit(MAX_JSON_BYTES)
            .read_to_string()
            .ok()?;
        serde_json::from_str(&text).ok()
    }

    fn post_json(&self, path: &str, body: &Value) -> Result<(u16, Value), String> {
        let mut response = self
            .agent
            .post(self.endpoint(path).as_str())
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .map_err(|error| error.to_string())?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .with_config()
            .limit(MAX_JSON_BYTES)
            .read_to_string()
            .unwrap_or_default();
        Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
    }

    pub fn probe(&self) -> Option<ServerInfo> {
        let stats = self.get_json(&self.quick, &self.endpoint("/system_stats"))?;
        let device = stats["devices"].as_array().and_then(|devices| devices.first());
        Some(ServerInfo {
            version: stats["system"]["comfyui_version"].as_str().map(str::to_owned),
            gpu: device
                .and_then(|device| device["name"].as_str())
                .map(clean_device_name),
            vram_mb: device
                .and_then(|device| device["vram_total"].as_u64())
                .map(|bytes| bytes / (1024 * 1024)),
        })
    }

    pub fn inventory(&self) -> Option<Inventory> {
        let info = self.get_json(&self.agent, &self.endpoint("/object_info"))?;
        let catalog = info.as_object()?;
        let nodes: BTreeSet<String> = catalog.keys().cloned().collect();
        let files = Folder::ALL
            .into_iter()
            .map(|folder| {
                let (class, input) = folder.loader();
                let options = catalog
                    .get(class)
                    .map(|node| combo_options(&node["input"]["required"][input]))
                    .unwrap_or_default();
                (folder, options)
            })
            .collect();
        Some(Inventory {
            files,
            nodes: Some(nodes),
        })
    }

    pub fn submit(&self, graph: &Value) -> Result<String, String> {
        let (status, body) = self.post_json(
            "/prompt",
            &json!({ "prompt": graph, "client_id": CLIENT_ID }),
        )?;
        if status == 200 {
            return body["prompt_id"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "ComfyUI не повернув id задачі".to_owned());
        }
        Err(describe_rejection(&body))
    }

    pub fn progress(&self, prompt_id: &str) -> Result<Progress, String> {
        let url = self.endpoint(&format!("/history/{prompt_id}"));
        let mut response = self
            .agent
            .get(url.as_str())
            .call()
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!("ComfyUI відповів {}", response.status().as_u16()));
        }
        let text = response
            .body_mut()
            .with_config()
            .limit(MAX_JSON_BYTES)
            .read_to_string()
            .map_err(|error| error.to_string())?;
        let history: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
        let entry = &history[prompt_id];
        if entry.is_null() {
            return Ok(Progress::Waiting);
        }
        if entry["status"]["status_str"].as_str() == Some("error") {
            return Ok(Progress::Failed(describe_failure(&entry["status"]["messages"])));
        }
        if entry["status"]["completed"].as_bool() == Some(false) {
            return Ok(Progress::Waiting);
        }
        Ok(Progress::Finished(collect_outputs(&entry["outputs"])))
    }

    pub fn download(&self, output: &OutputRef, destination: &Path) -> Result<u64, String> {
        let mut url = self.endpoint("/view");
        url.query_pairs_mut()
            .append_pair("filename", &output.filename)
            .append_pair("subfolder", &output.subfolder)
            .append_pair("type", OUTPUT_TYPE);
        let mut response = self
            .downloads
            .get(url.as_str())
            .call()
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!("ComfyUI не віддав файл ({})", response.status().as_u16()));
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let partial = destination.with_extension("part");
        let mut file = File::create(&partial).map_err(|error| error.to_string())?;
        let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
        let copied = std::io::copy(&mut reader, &mut file).map_err(|error| error.to_string())?;
        file.flush().map_err(|error| error.to_string())?;
        drop(file);
        std::fs::rename(&partial, destination).map_err(|error| error.to_string())?;
        Ok(copied)
    }

    pub fn queued(&self, prompt_id: &str) -> Option<bool> {
        let queue = self.get_json(&self.quick, &self.endpoint("/queue"))?;
        let listed = |key: &str| {
            queue[key].as_array().is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item[1].as_str() == Some(prompt_id))
            })
        };
        Some(listed("queue_running") || listed("queue_pending"))
    }

    pub fn free_memory(&self) {
        let _ = self.post_json("/free", &json!({ "unload_models": true, "free_memory": true }));
    }

    pub fn interrupt(&self, prompt_id: &str) {
        let _ = self.post_json("/queue", &json!({ "delete": [prompt_id] }));
        let _ = self.post_json("/interrupt", &json!({ "prompt_id": prompt_id }));
    }
}

fn clean_device_name(raw: &str) -> String {
    let name = raw.split(" : ").next().unwrap_or(raw);
    name.split_once(' ')
        .filter(|(prefix, _)| prefix.starts_with("cuda:"))
        .map_or(name, |(_, rest)| rest)
        .trim()
        .to_owned()
}

fn combo_options(spec: &Value) -> Vec<String> {
    let list = match &spec[0] {
        Value::Array(items) => Some(items),
        Value::String(kind) if kind == "COMBO" => spec[1]["options"].as_array(),
        _ => None,
    };
    list.map(|items| {
        items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    })
    .unwrap_or_default()
}

fn collect_outputs(outputs: &Value) -> Vec<OutputRef> {
    let Some(nodes) = outputs.as_object() else {
        return Vec::new();
    };
    nodes
        .values()
        .flat_map(|node| {
            ["images", "audio", "3d"]
                .into_iter()
                .filter_map(|key| node[key].as_array())
                .flatten()
        })
        .filter(|item| item["type"].as_str() == Some(OUTPUT_TYPE))
        .filter_map(|item| {
            Some(OutputRef {
                filename: item["filename"].as_str()?.to_owned(),
                subfolder: item["subfolder"].as_str().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

fn describe_rejection(body: &Value) -> String {
    let headline = body["error"]["message"]
        .as_str()
        .unwrap_or("ComfyUI відхилив граф")
        .to_owned();
    let details: Vec<String> = body["node_errors"]
        .as_object()
        .map(|nodes| {
            nodes
                .values()
                .flat_map(|node| {
                    let class = node["class_type"].as_str().unwrap_or("?").to_owned();
                    node["errors"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .map(move |error| {
                            let message = error["details"]
                                .as_str()
                                .filter(|text| !text.is_empty())
                                .or_else(|| error["message"].as_str())
                                .unwrap_or("помилка");
                            format!("{class}: {message}")
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    shorten(&if details.is_empty() {
        headline
    } else {
        format!("{headline} — {}", details.join("; "))
    })
}

fn describe_failure(messages: &Value) -> String {
    let failure = messages
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item[0].as_str() == Some("execution_error"))
        })
        .map(|item| &item[1]);
    let text = failure
        .map(|detail| {
            let node = detail["node_type"].as_str().unwrap_or("вузол");
            let message = detail["exception_message"]
                .as_str()
                .unwrap_or("невідома помилка")
                .trim();
            format!("{node}: {message}")
        })
        .unwrap_or_else(|| "ComfyUI зупинив задачу".to_owned());
    shorten(&text)
}

fn shorten(text: &str) -> String {
    let single = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if single.chars().count() <= MAX_ERROR_CHARS {
        return single;
    }
    let cut: String = single.chars().take(MAX_ERROR_CHARS).collect();
    format!("{cut}…")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    pub workspace: PathBuf,
    pub python: Option<PathBuf>,
}

impl Install {
    pub fn scan(&self) -> Inventory {
        let root = self.workspace.join(MODELS_DIR);
        let files = Folder::ALL
            .into_iter()
            .map(|folder| {
                let mut names: Vec<String> = folder
                    .directories()
                    .iter()
                    .flat_map(|directory| model_files(&root.join(directory)))
                    .collect();
                names.sort();
                names.dedup();
                (folder, names)
            })
            .collect();
        Inventory { files, nodes: None }
    }

    pub fn launch(&self, log: &Path) -> Result<(), String> {
        let python = self
            .python
            .as_ref()
            .ok_or_else(|| "не знайдено Python для ComfyUI".to_owned())?;
        if let Some(parent) = log.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let stdout = File::create(log).map_err(|error| error.to_string())?;
        let stderr = stdout.try_clone().map_err(|error| error.to_string())?;
        let mut command = Command::new(python);
        command
            .arg(MAIN_SCRIPT)
            .args(["--listen", COMFY_HOST, "--port", COMFY_PORT])
            .current_dir(&self.workspace)
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        hide_console(&mut command);
        command.spawn().map(drop).map_err(|error| error.to_string())
    }
}

pub fn locate_install() -> Option<Install> {
    candidate_workspaces()
        .into_iter()
        .find(|workspace| workspace.join(MAIN_SCRIPT).is_file() && workspace.join(MODELS_DIR).is_dir())
        .map(|workspace| Install {
            python: locate_python(&workspace),
            workspace,
        })
}

fn candidate_workspaces() -> Vec<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from);
    let mut candidates = Vec::new();
    if let Some(config) = local
        .as_ref()
        .map(|local| local.join("comfy-cli").join("config.ini"))
        .and_then(|path| std::fs::read_to_string(path).ok())
    {
        for key in ["default_workspace", "recent_workspace"] {
            if let Some(path) = ini_value(&config, key) {
                candidates.push(PathBuf::from(path));
            }
        }
    }
    if let Some(installs) = local
        .as_ref()
        .map(|local| local.join("Comfy-Desktop").join("ComfyUI-Installs"))
        .and_then(|path| std::fs::read_dir(path).ok())
    {
        candidates.extend(installs.flatten().map(|entry| entry.path().join("ComfyUI")));
    }
    if let Some(home) = home {
        candidates.push(home.join("ComfyUI"));
        candidates.push(home.join("Documents").join("ComfyUI"));
    }
    candidates.push(PathBuf::from(r"C:\ComfyUI_windows_portable\ComfyUI"));
    candidates.push(PathBuf::from(r"C:\ComfyUI"));
    candidates
}

fn ini_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key)
            .then(|| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn locate_python(workspace: &Path) -> Option<PathBuf> {
    let parent = workspace.parent().unwrap_or(workspace);
    [
        parent.join("standalone-env").join("python.exe"),
        workspace.join(".venv").join("Scripts").join("python.exe"),
        workspace.join("venv").join("Scripts").join("python.exe"),
        parent.join("python_embeded").join("python.exe"),
        workspace.join(".venv").join("bin").join("python"),
        workspace.join("venv").join("bin").join("python"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

fn model_files(directory: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let known = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| MODEL_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()));
            if !known {
                continue;
            }
            if let Ok(relative) = path.strip_prefix(directory) {
                found.push(relative.to_string_lossy().into_owned());
            }
        }
    }
    found
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_combo_formats() {
        let classic = json!([["a.safetensors", "b.safetensors"], {}]);
        assert_eq!(combo_options(&classic), vec!["a.safetensors", "b.safetensors"]);
        let modern = json!(["COMBO", { "options": ["birefnet.safetensors"] }]);
        assert_eq!(combo_options(&modern), vec!["birefnet.safetensors"]);
        assert!(combo_options(&json!(["INT", {}])).is_empty());
    }

    #[test]
    fn collects_only_saved_outputs() {
        let outputs = json!({
            "9": { "images": [
                { "filename": "a_00001_.png", "subfolder": "noctics/4", "type": "output" },
                { "filename": "preview.png", "subfolder": "", "type": "temp" }
            ]},
            "12": { "3d": [{ "filename": "m_00001_.glb", "subfolder": "noctics/4", "type": "output" }] },
            "14": { "audio": [{ "filename": "s_00001_.flac", "subfolder": "", "type": "output" }] }
        });
        let mut names: Vec<String> = collect_outputs(&outputs)
            .into_iter()
            .map(|output| output.filename)
            .collect();
        names.sort();
        assert_eq!(names, vec!["a_00001_.png", "m_00001_.glb", "s_00001_.flac"]);
    }

    #[test]
    fn explains_rejected_graphs() {
        let body = json!({
            "error": { "message": "Prompt outputs failed validation" },
            "node_errors": { "3": {
                "class_type": "UNETLoader",
                "errors": [{ "message": "Value not in list", "details": "unet_name: 'x' not in []" }]
            }}
        });
        assert_eq!(
            describe_rejection(&body),
            "Prompt outputs failed validation — UNETLoader: unet_name: 'x' not in []"
        );
        let messages = json!([
            ["execution_start", {}],
            ["execution_error", { "node_type": "KSampler", "exception_message": "CUDA out of memory\n" }]
        ]);
        assert_eq!(describe_failure(&messages), "KSampler: CUDA out of memory");
    }

    #[test]
    fn annotates_output_paths() {
        let nested = OutputRef { filename: "ref_00001_.png".into(), subfolder: "noctics/7".into() };
        assert_eq!(nested.annotated(), "noctics/7/ref_00001_.png [output]");
        assert_eq!(nested.relative(), "noctics/7/ref_00001_.png");
        assert_eq!(nested.extension(), "png");
        let flat = OutputRef { filename: "a.PNG".into(), subfolder: String::new() };
        assert_eq!(flat.annotated(), "a.PNG [output]");
        assert_eq!(flat.extension(), "png");
    }

    #[test]
    fn reads_ini_values_and_device_names() {
        let config = "[DEFAULT]\ndefault_workspace = C:\\Comfy\\ComfyUI\nrecent_workspace =\n";
        assert_eq!(ini_value(config, "default_workspace").as_deref(), Some("C:\\Comfy\\ComfyUI"));
        assert_eq!(ini_value(config, "recent_workspace"), None);
        assert_eq!(
            clean_device_name("cuda:0 NVIDIA GeForce RTX 5060 Laptop GPU : cudaMallocAsync"),
            "NVIDIA GeForce RTX 5060 Laptop GPU"
        );
    }
}
