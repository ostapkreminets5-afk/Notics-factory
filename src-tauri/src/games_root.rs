use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock};

use noctics_shared::{BuildError, GamesFolder, MAX_SOURCE_PATH_CHARS};

pub struct GamesRoot {
    file: PathBuf,
    fallback: PathBuf,
    chosen: RwLock<Option<PathBuf>>,
}

impl GamesRoot {
    pub fn open(file: PathBuf, fallback: PathBuf) -> Self {
        let chosen = fs::read_to_string(&file)
            .ok()
            .and_then(|text| parse(&text).ok())
            .filter(|path| path != &fallback);
        Self {
            file,
            fallback,
            chosen: RwLock::new(chosen),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.chosen().unwrap_or_else(|| self.fallback.clone())
    }

    pub fn snapshot(&self) -> GamesFolder {
        let chosen = self.chosen();
        GamesFolder {
            path: chosen
                .as_deref()
                .unwrap_or(&self.fallback)
                .to_string_lossy()
                .into_owned(),
            is_default: chosen.is_none(),
        }
    }

    pub fn choose(&self, raw: &str) -> Result<GamesFolder, BuildError> {
        let path = parse(raw)?;
        if path == self.fallback {
            return self.reset();
        }
        create(&path)?;
        fs::write(&self.file, path.to_string_lossy().as_bytes()).map_err(|error| BuildError::Storage(error.to_string()))?;
        self.remember(Some(path));
        Ok(self.snapshot())
    }

    pub fn reset(&self) -> Result<GamesFolder, BuildError> {
        match fs::remove_file(&self.file) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(BuildError::Storage(error.to_string())),
        }
        self.remember(None);
        Ok(self.snapshot())
    }

    pub fn ensure(&self) -> Result<PathBuf, BuildError> {
        let path = self.path();
        create(&path)?;
        Ok(path)
    }

    fn chosen(&self) -> Option<PathBuf> {
        self.chosen.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn remember(&self, path: Option<PathBuf>) {
        *self.chosen.write().unwrap_or_else(PoisonError::into_inner) = path;
    }
}

fn parse(raw: &str) -> Result<PathBuf, BuildError> {
    let cleaned = raw.trim().trim_matches('"').trim();
    if cleaned.is_empty() || cleaned.chars().count() > MAX_SOURCE_PATH_CHARS {
        return Err(BuildError::PathInvalid);
    }
    let path = PathBuf::from(cleaned);
    if !path.is_absolute() || (path.exists() && !path.is_dir()) {
        return Err(BuildError::PathInvalid);
    }
    Ok(path)
}

fn create(path: &Path) -> Result<(), BuildError> {
    fs::create_dir_all(path)
        .map_err(|error| BuildError::Process(format!("не вдалося створити {}: {error}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sandbox(PathBuf);

    impl Sandbox {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("noctics-games-root-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn root(&self) -> GamesRoot {
            GamesRoot::open(self.0.join("games-folder.txt"), self.0.join("default"))
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn starts_at_the_default_folder() {
        let sandbox = Sandbox::new("default");
        let root = sandbox.root();
        assert_eq!(root.path(), sandbox.0.join("default"));
        assert!(root.snapshot().is_default);
    }

    #[test]
    fn remembers_a_chosen_folder_across_restarts() {
        let sandbox = Sandbox::new("chosen");
        let picked = sandbox.0.join("My Games");
        let snapshot = sandbox.root().choose(&format!("  \"{}\" ", picked.display())).unwrap();
        assert!(!snapshot.is_default);
        assert!(picked.is_dir());
        assert_eq!(sandbox.root().path(), picked);
    }

    #[test]
    fn resets_back_to_the_default_folder() {
        let sandbox = Sandbox::new("reset");
        let root = sandbox.root();
        root.choose(&sandbox.0.join("elsewhere").to_string_lossy()).unwrap();
        assert!(root.reset().unwrap().is_default);
        assert_eq!(sandbox.root().path(), sandbox.0.join("default"));
    }

    #[test]
    fn rejects_relative_paths_and_files() {
        let sandbox = Sandbox::new("invalid");
        let root = sandbox.root();
        let file = sandbox.0.join("note.txt");
        fs::write(&file, "x").unwrap();
        assert_eq!(root.choose("games"), Err(BuildError::PathInvalid));
        assert_eq!(root.choose(""), Err(BuildError::PathInvalid));
        assert_eq!(root.choose(&file.to_string_lossy()), Err(BuildError::PathInvalid));
        assert!(root.snapshot().is_default);
    }
}
