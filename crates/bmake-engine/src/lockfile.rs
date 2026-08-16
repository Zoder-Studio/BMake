use crate::paths::BMakePaths;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Lockfile {
    #[serde(default)]
    pub dependency: HashMap<String, String>,
    #[serde(default)]
    pub tool: HashMap<String, String>,
    #[serde(default)]
    pub engine: String,
    #[serde(default)]
    pub environment: String,
    #[serde(default)]
    pub arch: String,
}

pub fn path(paths: &BMakePaths) -> PathBuf {
    paths.root.join("BMake.lock")
}

pub fn write(paths: &BMakePaths, lock: &Lockfile) -> Result<()> {
    std::fs::write(path(paths), toml::to_string_pretty(lock)?)?;
    Ok(())
}

pub fn read(paths: &BMakePaths) -> Result<Option<Lockfile>> {
    let p = path(paths);
    if !p.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(p)?;
    Ok(Some(toml::from_str(&content)?))
}