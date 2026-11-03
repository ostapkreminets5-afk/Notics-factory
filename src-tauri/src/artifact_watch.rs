use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};

use chrono::DateTime;
use noctics_shared::{ARTIFACTS_CHANGED_EVENT, ArtifactError, artifact_link};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::artifacts::{ArtifactStore, FoundArtifact};

const STARTUP_DELAY: Duration = Duration::from_secs(3);
const SCAN_INTERVAL: Duration = Duration::from_secs(15);
const LOOKBACK: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const TRANSCRIPT_EXTENSION: &str = "jsonl";
const CONFIG_DIR_VARIABLE: &str = "CLAUDE_CONFIG_DIR";
const HOME_VARIABLES: [&str; 2] = ["USERPROFILE", "HOME"];
const CONFIG_DIR_NAME: &str = ".claude";
const PROJECTS_DIR_NAME: &str = "projects";
const ARTIFACT_MARKER: &str = "claude.ai/artifact";
const TOOL_RESULT_MARKER: &str = "\"tool_result\"";
const OWN_RELATION: &str = "mine";

pub fn watch(app: AppHandle, store: Arc<ArtifactStore>) {
    let Some(root) = transcripts_root() else {
        return;
    };
    thread::spawn(move || {
        thread::sleep(STARTUP_DELAY);
        let mut announce = || {
            let _ = app.emit(ARTIFACTS_CHANGED_EVENT, ());
        };
        loop {
            let _ = scan(&store, &root, SystemTime::now(), &mut announce);
            thread::sleep(SCAN_INTERVAL);
        }
    });
}

fn transcripts_root() -> Option<PathBuf> {
    let config = std::env::var_os(CONFIG_DIR_VARIABLE)
        .map(PathBuf::from)
        .or_else(|| {
            HOME_VARIABLES
                .iter()
                .find_map(std::env::var_os)
                .map(|home| PathBuf::from(home).join(CONFIG_DIR_NAME))
        })?;
    Some(config.join(PROJECTS_DIR_NAME))
}

fn scan(
    store: &ArtifactStore,
    root: &Path,
    now: SystemTime,
    announce: &mut impl FnMut(),
) -> Result<bool, ArtifactError> {
    let mut changed = false;
    for path in transcripts(root) {
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        let recent = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_none_or(|age| age <= LOOKBACK);
        if !recent {
            continue;
        }
        let key = path.to_string_lossy();
        let remembered = store.transcript_offset(&key)?;
        let start = if metadata.len() < remembered { 0 } else { remembered };
        if metadata.len() == start {
            continue;
        }
        let Ok((found, consumed)) = read_new_lines(&path, start) else {
            continue;
        };
        let mut file_changed = false;
        for artifact in &found {
            file_changed |= store.record_found(artifact)?;
        }
        if file_changed {
            changed = true;
            announce();
        }
        if consumed > 0 {
            store.set_transcript_offset(&key, start + consumed)?;
        }
    }
    Ok(changed)
}

fn transcripts(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut found = Vec::new();
    while let Some(folder) = pending.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(path),
                Ok(kind) if kind.is_file() && path.extension().is_some_and(|ext| ext == TRANSCRIPT_EXTENSION) => {
                    found.push(path);
                }
                _ => {}
            }
        }
    }
    found
}

fn read_new_lines(path: &Path, start: u64) -> std::io::Result<(Vec<FoundArtifact>, u64)> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut consumed = 0;
    let mut found = Vec::new();
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 || line.last() != Some(&b'\n') {
            break;
        }
        consumed += read as u64;
        if let Ok(text) = std::str::from_utf8(&line) {
            found.extend(artifacts_in_line(text));
        }
    }
    Ok((found, consumed))
}

fn artifacts_in_line(line: &str) -> Vec<FoundArtifact> {
    let mut found = Vec::new();
    if !line.contains(ARTIFACT_MARKER) || !line.contains(TOOL_RESULT_MARKER) {
        return found;
    }
    let Ok(entry) = serde_json::from_str::<Value>(line) else {
        return found;
    };
    let Some(blocks) = entry.pointer("/message/content").and_then(Value::as_array) else {
        return found;
    };
    for block in blocks.iter().filter(|block| block["type"] == "tool_result") {
        for text in result_texts(&block["content"]) {
            if let Ok(payload) = serde_json::from_str::<Value>(text) {
                collect(&payload, &mut found);
            }
        }
    }
    found
}

fn result_texts(content: &Value) -> Vec<&str> {
    match content {
        Value::String(text) => vec![text.as_str()],
        Value::Array(items) => items.iter().filter_map(|item| item["text"].as_str()).collect(),
        _ => Vec::new(),
    }
}

fn collect(value: &Value, found: &mut Vec<FoundArtifact>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| collect(item, found)),
        Value::Object(fields) => {
            let link = fields.get("url").and_then(Value::as_str).and_then(artifact_link);
            let foreign = fields
                .get("rel")
                .and_then(Value::as_str)
                .is_some_and(|relation| relation != OWN_RELATION);
            let deleted = fields.get("soft_deleted").and_then(Value::as_bool).unwrap_or(false);
            if let (Some(link), false, false) = (link, foreign, deleted) {
                found.push(FoundArtifact {
                    link,
                    title: fields.get("title").and_then(Value::as_str).map(str::to_owned),
                    created_at: ["created_at", "updated_at"]
                        .iter()
                        .filter_map(|key| fields.get(*key).and_then(Value::as_str))
                        .find_map(|stamp| DateTime::parse_from_rfc3339(stamp).ok())
                        .map(|stamp| stamp.timestamp()),
                });
            }
            fields.values().for_each(|nested| collect(nested, found));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST_LINE: &str = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_1","type":"tool_result","content":[{"type":"text","text":"<artifact-content-authored-by-others/>"},{"type":"text","text":"{\"artifacts\":[{\"slug\":\"8116871c\",\"title\":\"ВІДПУСТИ — Арт-дизайн\",\"rel\":\"mine\",\"updated_at\":\"2026-10-30T21:48:44Z\",\"url\":\"https://claude.ai/artifact/GwYHvofqunFu6ZSHy3aJPY\"},{\"slug\":\"x\",\"title\":\"Чужий\",\"rel\":\"shared\",\"url\":\"https://claude.ai/artifact/Foreign1\"},{\"slug\":\"y\",\"title\":\"Видалений\",\"rel\":\"mine\",\"soft_deleted\":true,\"url\":\"https://claude.ai/artifact/Deleted1\"}]}"}]}]}}"#;
    const PUBLISH_LINE: &str = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_2","type":"tool_result","content":"{\"slug\":\"e275\",\"version\":\"1793022101-97e7\",\"read\":\"owner\",\"url\":\"https://claude.ai/artifact/UxuweBPZSnhor1MAJw3vte\"}"}]}}"#;
    const ASSISTANT_LINE: &str = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Ось посилання https://claude.ai/artifact/Mentioned1 і \"tool_result\""}]}}"#;

    #[test]
    fn reads_own_artifacts_from_list_results() {
        let found = artifacts_in_line(LIST_LINE);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].link, "https://claude.ai/artifact/GwYHvofqunFu6ZSHy3aJPY");
        assert_eq!(found[0].title.as_deref(), Some("ВІДПУСТИ — Арт-дизайн"));
        assert_eq!(found[0].created_at, Some(1_793_396_924));
    }

    #[test]
    fn reads_published_artifacts() {
        let found = artifacts_in_line(PUBLISH_LINE);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].link, "https://claude.ai/artifact/UxuweBPZSnhor1MAJw3vte");
        assert_eq!(found[0].title, None);
    }

    #[test]
    fn ignores_links_outside_tool_results() {
        assert!(artifacts_in_line(ASSISTANT_LINE).is_empty());
        assert!(artifacts_in_line("not json claude.ai/artifact \"tool_result\"").is_empty());
    }

    #[test]
    fn scans_only_new_complete_lines() {
        let root = std::env::temp_dir().join(format!("noctics-artifact-watch-{}", std::process::id()));
        let project = root.join("c--Users-Owl-Game");
        fs::create_dir_all(&project).unwrap();
        let transcript = project.join("session.jsonl");
        fs::write(&transcript, format!("{LIST_LINE}\n{PUBLISH_LINE}")).unwrap();
        let store = ArtifactStore::open(&root.join("shelf.sqlite3")).unwrap();

        assert!(scan(&store, &root, SystemTime::now(), &mut || {}).unwrap());
        assert_eq!(store.list().unwrap().len(), 1);
        assert!(!scan(&store, &root, SystemTime::now(), &mut || {}).unwrap());

        fs::write(&transcript, format!("{LIST_LINE}\n{PUBLISH_LINE}\n")).unwrap();
        assert!(scan(&store, &root, SystemTime::now(), &mut || {}).unwrap());
        assert_eq!(store.list().unwrap().len(), 2);

        let far_future = SystemTime::now() + LOOKBACK + LOOKBACK;
        fs::write(project.join("old.jsonl"), format!("{LIST_LINE}\n")).unwrap();
        assert!(!scan(&store, &root, far_future, &mut || {}).unwrap());

        drop(store);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    #[ignore = "reads the real Claude Code transcripts on this machine"]
    fn live_scan_of_local_transcripts() {
        let root = transcripts_root().expect("no home folder");
        let shelf_file = std::env::temp_dir().join(format!("noctics-live-shelf-{}.sqlite3", std::process::id()));
        let store = ArtifactStore::open(&shelf_file).unwrap();
        let started = std::time::Instant::now();
        scan(&store, &root, SystemTime::now(), &mut || {}).unwrap();
        let first = started.elapsed();
        let resumed = std::time::Instant::now();
        scan(&store, &root, SystemTime::now(), &mut || {}).unwrap();
        let second = resumed.elapsed();
        for artifact in store.list().unwrap() {
            println!("{} | {} | {}", artifact.added_at, artifact.title, artifact.link);
        }
        println!("first scan {first:?}, next scan {second:?}");
        drop(store);
        let _ = fs::remove_file(&shelf_file);
    }
}
