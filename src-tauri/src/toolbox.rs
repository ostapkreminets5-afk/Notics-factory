use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use noctics_shared::{ActiveTool, Tool, ToolDraft, ToolError, ToolSlot, Toolbox};
use rusqlite::{Connection, params};

use crate::accounts::unix_now;

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS custom_tools (
        id INTEGER PRIMARY KEY,
        slot TEXT NOT NULL,
        name TEXT NOT NULL,
        note TEXT NOT NULL,
        url TEXT,
        created_at INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS tool_choices (
        slot TEXT PRIMARY KEY,
        tool_id TEXT NOT NULL
    );
";
const CUSTOM_PREFIX: &str = "custom-";

struct Preset {
    id: &'static str,
    slot: ToolSlot,
    name: &'static str,
    note: &'static str,
    url: Option<&'static str>,
    integrated: bool,
}

const PRESETS: [Preset; 23] = [
    Preset {
        id: "claude",
        slot: ToolSlot::Core,
        name: "AI-ядро",
        note: "Claude за підпискою · конвеєр і навички",
        url: Some("https://claude.ai"),
        integrated: true,
    },
    Preset {
        id: "comfyui",
        slot: ToolSlot::Images,
        name: "ComfyUI",
        note: "Усі моделі на одну задачу",
        url: Some("http://127.0.0.1:8188"),
        integrated: true,
    },
    Preset {
        id: "swarmui",
        slot: ToolSlot::Images,
        name: "SwarmUI",
        note: "Локальний інтерфейс над ComfyUI",
        url: Some("http://127.0.0.1:7801"),
        integrated: false,
    },
    Preset {
        id: "midjourney",
        slot: ToolSlot::Images,
        name: "Midjourney",
        note: "Концепт-арт за промптом",
        url: Some("https://www.midjourney.com"),
        integrated: false,
    },
    Preset {
        id: "pixellab",
        slot: ToolSlot::Images,
        name: "PixelLab",
        note: "Піксель-арт, спрайти й тайли",
        url: Some("https://www.pixellab.ai"),
        integrated: false,
    },
    Preset {
        id: "krea",
        slot: ToolSlot::Images,
        name: "Krea",
        note: "Генерація й апскейл у реальному часі",
        url: Some("https://www.krea.ai"),
        integrated: false,
    },
    Preset {
        id: "claude-design",
        slot: ToolSlot::Interface,
        name: "Claude Design",
        note: "Екрани гри від Claude",
        url: None,
        integrated: true,
    },
    Preset {
        id: "open-design",
        slot: ToolSlot::Interface,
        name: "Open Design",
        note: "Відкрита local-first альтернатива Claude Design",
        url: Some("https://open-design.ai"),
        integrated: false,
    },
    Preset {
        id: "figma",
        slot: ToolSlot::Interface,
        name: "Figma",
        note: "Макети інтерфейсу",
        url: Some("https://www.figma.com"),
        integrated: false,
    },
    Preset {
        id: "penpot",
        slot: ToolSlot::Interface,
        name: "Penpot",
        note: "Відкритий редактор макетів",
        url: Some("https://penpot.app"),
        integrated: false,
    },
    Preset {
        id: "pinterest",
        slot: ToolSlot::References,
        name: "Pinterest",
        note: "Дошки референсів",
        url: Some("https://www.pinterest.com"),
        integrated: false,
    },
    Preset {
        id: "artstation",
        slot: ToolSlot::References,
        name: "ArtStation",
        note: "Концепт-арт професіоналів",
        url: Some("https://www.artstation.com"),
        integrated: false,
    },
    Preset {
        id: "arena",
        slot: ToolSlot::References,
        name: "Are.na",
        note: "Колекції ідей і настрою",
        url: Some("https://www.are.na"),
        integrated: false,
    },
    Preset {
        id: "blender",
        slot: ToolSlot::Models,
        name: "3D Models",
        note: "Blender · моделі й сцени",
        url: Some("https://www.blender.org"),
        integrated: false,
    },
    Preset {
        id: "meshy",
        slot: ToolSlot::Models,
        name: "Meshy",
        note: "3D-моделі з тексту й картинок",
        url: Some("https://www.meshy.ai"),
        integrated: false,
    },
    Preset {
        id: "sketchfab",
        slot: ToolSlot::Models,
        name: "Sketchfab",
        note: "Бібліотека готових моделей",
        url: Some("https://sketchfab.com"),
        integrated: false,
    },
    Preset {
        id: "polyhaven",
        slot: ToolSlot::Models,
        name: "Poly Haven",
        note: "Безкоштовні моделі й текстури",
        url: Some("https://polyhaven.com"),
        integrated: false,
    },
    Preset {
        id: "build",
        slot: ToolSlot::Build,
        name: "Збірка",
        note: "Білди для ПК, вебу й телефонів",
        url: None,
        integrated: true,
    },
    Preset {
        id: "godot",
        slot: ToolSlot::Build,
        name: "Godot",
        note: "Відкритий рушій 2D і 3D",
        url: Some("https://godotengine.org"),
        integrated: false,
    },
    Preset {
        id: "unity",
        slot: ToolSlot::Build,
        name: "Unity",
        note: "Рушій для ПК і мобільних",
        url: Some("https://unity.com"),
        integrated: false,
    },
    Preset {
        id: "unreal",
        slot: ToolSlot::Build,
        name: "Unreal Engine",
        note: "Рушій для 3D-ігор",
        url: Some("https://www.unrealengine.com"),
        integrated: false,
    },
    Preset {
        id: "defold",
        slot: ToolSlot::Build,
        name: "Defold",
        note: "Легкий 2D-рушій",
        url: Some("https://defold.com"),
        integrated: false,
    },
    Preset {
        id: "phaser",
        slot: ToolSlot::Build,
        name: "Phaser",
        note: "Браузерні 2D-ігри",
        url: Some("https://phaser.io"),
        integrated: false,
    },
];

pub struct ToolboxStore {
    connection: Mutex<Connection>,
}

impl ToolboxStore {
    pub fn open(path: &Path) -> Result<Self, ToolError> {
        Self::prepare(Connection::open(path).map_err(storage)?)
    }

    fn prepare(connection: Connection) -> Result<Self, ToolError> {
        connection.execute_batch(SCHEMA).map_err(storage)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn snapshot(&self) -> Result<Toolbox, ToolError> {
        snapshot(&*self.lock()?)
    }

    pub fn choose(&self, slot: ToolSlot, tool_id: &str) -> Result<Toolbox, ToolError> {
        let connection = self.lock()?;
        let current = snapshot(&connection)?;
        if !current.tools_for(slot).any(|tool| tool.id == tool_id) {
            return Err(ToolError::NotFound);
        }
        connection
            .execute(
                "INSERT INTO tool_choices (slot, tool_id) VALUES (?1, ?2)
                 ON CONFLICT(slot) DO UPDATE SET tool_id = excluded.tool_id",
                params![slot.code(), tool_id],
            )
            .map_err(storage)?;
        snapshot(&connection)
    }

    pub fn add(&self, draft: &ToolDraft) -> Result<Toolbox, ToolError> {
        let draft = draft.normalized()?;
        let connection = self.lock()?;
        let url = (!draft.url.is_empty()).then_some(draft.url.as_str());
        connection
            .execute(
                "INSERT INTO custom_tools (slot, name, note, url, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![draft.slot.code(), draft.name, draft.note, url, unix_now()],
            )
            .map_err(storage)?;
        let tool_id = format!("{CUSTOM_PREFIX}{}", connection.last_insert_rowid());
        connection
            .execute(
                "INSERT INTO tool_choices (slot, tool_id) VALUES (?1, ?2)
                 ON CONFLICT(slot) DO UPDATE SET tool_id = excluded.tool_id",
                params![draft.slot.code(), tool_id],
            )
            .map_err(storage)?;
        snapshot(&connection)
    }

    pub fn remove(&self, tool_id: &str) -> Result<Toolbox, ToolError> {
        let row_id = custom_row_id(tool_id).ok_or(if is_preset(tool_id) {
            ToolError::BuiltIn
        } else {
            ToolError::NotFound
        })?;
        let connection = self.lock()?;
        let removed = connection
            .execute("DELETE FROM custom_tools WHERE id = ?1", params![row_id])
            .map_err(storage)?;
        if removed == 0 {
            return Err(ToolError::NotFound);
        }
        connection
            .execute(
                "DELETE FROM tool_choices WHERE tool_id = ?1",
                params![tool_id],
            )
            .map_err(storage)?;
        snapshot(&connection)
    }

    pub fn url_of(&self, tool_id: &str) -> Result<String, ToolError> {
        let toolbox = self.snapshot()?;
        let tool = toolbox
            .tools
            .iter()
            .find(|tool| tool.id == tool_id)
            .ok_or(ToolError::NotFound)?;
        tool.url.clone().ok_or(ToolError::NoUrl)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, ToolError> {
        self.connection
            .lock()
            .map_err(|_| ToolError::Storage("toolbox lock is poisoned".into()))
    }
}

fn snapshot(connection: &Connection) -> Result<Toolbox, ToolError> {
    let mut tools: Vec<Tool> = PRESETS
        .iter()
        .map(|preset| Tool {
            id: preset.id.to_owned(),
            slot: preset.slot,
            name: preset.name.to_owned(),
            note: preset.note.to_owned(),
            url: preset.url.map(str::to_owned),
            custom: false,
            integrated: preset.integrated,
        })
        .collect();
    let mut statement = connection
        .prepare("SELECT id, slot, name, note, url FROM custom_tools ORDER BY id")
        .map_err(storage)?;
    let custom = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(storage)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage)?;
    tools.extend(
        custom
            .into_iter()
            .filter_map(|(id, slot, name, note, url)| {
                Some(Tool {
                    id: format!("{CUSTOM_PREFIX}{id}"),
                    slot: ToolSlot::from_code(&slot)?,
                    name,
                    note,
                    url,
                    custom: true,
                    integrated: false,
                })
            }),
    );
    let mut choices = connection
        .prepare("SELECT slot, tool_id FROM tool_choices")
        .map_err(storage)?;
    let active = choices
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage)?
        .into_iter()
        .filter_map(|(slot, tool_id)| {
            Some(ActiveTool {
                slot: ToolSlot::from_code(&slot)?,
                tool_id,
            })
        })
        .collect();
    Ok(Toolbox { tools, active })
}

fn custom_row_id(tool_id: &str) -> Option<i64> {
    tool_id.strip_prefix(CUSTOM_PREFIX)?.parse().ok()
}

fn is_preset(tool_id: &str) -> bool {
    PRESETS.iter().any(|preset| preset.id == tool_id)
}

fn storage(error: rusqlite::Error) -> ToolError {
    ToolError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory() -> ToolboxStore {
        ToolboxStore::prepare(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn draft(name: &str) -> ToolDraft {
        ToolDraft {
            slot: ToolSlot::Images,
            name: name.into(),
            note: "Локальна SDXL".into(),
            url: "http://127.0.0.1:7860".into(),
        }
    }

    #[test]
    fn every_slot_starts_with_its_default_tool() {
        let toolbox = in_memory().snapshot().unwrap();
        let defaults: Vec<&str> = ToolSlot::ALL
            .iter()
            .map(|slot| toolbox.active_tool(*slot).unwrap().id.as_str())
            .collect();
        assert_eq!(
            defaults,
            [
                "claude",
                "comfyui",
                "claude-design",
                "pinterest",
                "blender",
                "build"
            ]
        );
        let mut ids: Vec<&str> = PRESETS.iter().map(|preset| preset.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), PRESETS.len());
    }

    #[test]
    fn chooses_only_tools_of_the_same_slot() {
        let store = in_memory();
        let toolbox = store.choose(ToolSlot::Images, "pixellab").unwrap();
        assert_eq!(
            toolbox.active_tool(ToolSlot::Images).unwrap().name,
            "PixelLab"
        );
        assert_eq!(
            store.choose(ToolSlot::Images, "figma"),
            Err(ToolError::NotFound)
        );
        let design = store.choose(ToolSlot::Interface, "open-design").unwrap();
        assert_eq!(
            design.active_tool(ToolSlot::Interface).unwrap().name,
            "Open Design"
        );
        assert_eq!(store.url_of("open-design").unwrap(), "https://open-design.ai");
        assert_eq!(
            store.choose(ToolSlot::Images, "unknown"),
            Err(ToolError::NotFound)
        );
    }

    #[test]
    fn adds_selects_and_removes_custom_tools() {
        let store = in_memory();
        let added = store.add(&draft("  Automatic1111 ")).unwrap();
        let custom = added.active_tool(ToolSlot::Images).unwrap().clone();
        assert!(custom.custom && custom.id.starts_with(CUSTOM_PREFIX));
        assert_eq!(custom.name, "Automatic1111");
        assert_eq!(store.url_of(&custom.id).unwrap(), "http://127.0.0.1:7860");

        let removed = store.remove(&custom.id).unwrap();
        assert_eq!(removed.active_tool(ToolSlot::Images).unwrap().id, "comfyui");
        assert_eq!(store.remove(&custom.id), Err(ToolError::NotFound));
        assert_eq!(store.remove("figma"), Err(ToolError::BuiltIn));
        assert_eq!(store.url_of("claude-design"), Err(ToolError::NoUrl));
    }

    #[test]
    fn rejects_invalid_custom_tools() {
        let store = in_memory();
        let mut bad = draft("Сайт");
        bad.url = "file:///C:/Windows".into();
        assert_eq!(store.add(&bad), Err(ToolError::InvalidUrl));
        assert_eq!(store.add(&draft(" ")), Err(ToolError::NameMissing));
        assert!(
            store
                .snapshot()
                .unwrap()
                .tools
                .iter()
                .all(|tool| !tool.custom)
        );
    }
}
