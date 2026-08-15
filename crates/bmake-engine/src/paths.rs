use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct BMakePaths {
    pub root: PathBuf,
}

impl BMakePaths {
    pub fn new(project_dir: &Path) -> Self {
        Self {
            root: project_dir.join(".bmake"),
        }
    }

    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }
    pub fn dependencies(&self) -> PathBuf {
        self.root.join("dependencies")
    }
    pub fn tools(&self) -> PathBuf {
        self.root.join("tools")
    }
    pub fn engines(&self) -> PathBuf {
        self.root.join("engines")
    }
    pub fn sandbox(&self) -> PathBuf {
        self.root.join("sandbox")
    }
    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }
    pub fn credentials(&self) -> PathBuf {
        self.root.join("credentials.toml")
    }

    pub fn ensure_all(&self) -> std::io::Result<()> {
        for d in [
            self.cache(),
            self.dependencies(),
            self.tools(),
            self.engines(),
            self.sandbox(),
            self.tmp(),
        ] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }
}