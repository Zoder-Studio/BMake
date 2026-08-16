use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Runner {
    pub id: String,
    pub name: String,
    pub runs_on: String,
    pub version: String,
    pub arch: String,
    pub status: RunnerStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum RunnerStatus {
    Online,
    Offline,
    Busy,
    Error,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RunnerFile {
    #[serde(default)]
    runner: Vec<Runner>,
}

fn registry_path() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(".bmake").join("runners.toml")
}

pub fn load_all() -> Result<Vec<Runner>> {
    let path = registry_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    let parsed: RunnerFile = toml::from_str(&content)?;
    Ok(parsed.runner)
}

fn save_all(runners: &[Runner]) -> Result<()> {
    let path = registry_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = RunnerFile { runner: runners.to_vec() };
    std::fs::write(&path, toml::to_string_pretty(&file)?)?;
    Ok(())
}

pub fn register(name: &str, runs_on: &str, version: &str, arch: &str) -> Result<Runner> {
    let mut runners = load_all()?;
    let id = format!("runner-{:x}", stable_id(name, runs_on, version, arch));
    let runner = Runner {
        id: id.clone(),
        name: name.to_string(),
        runs_on: runs_on.to_string(),
        version: version.to_string(),
        arch: arch.to_string(),
        status: RunnerStatus::Offline,
    };
    runners.retain(|r| r.id != id);
    runners.push(runner.clone());
    save_all(&runners)?;
    Ok(runner)
}

pub fn set_status(id: &str, status: RunnerStatus) -> Result<()> {
    let mut runners = load_all()?;
    let Some(r) = runners.iter_mut().find(|r| r.id == id) else {
        bail!("Runner '{}' is not registered", id);
    };
    r.status = status;
    save_all(&runners)?;
    Ok(())
}

pub fn find_match(runs_on: &str, version: Option<&str>, arch: Option<&str>) -> Result<Option<Runner>> {
    let runners = load_all()?;
    Ok(runners.into_iter().find(|r| {
        r.status == RunnerStatus::Online
            && r.runs_on == runs_on
            && version.map(|v| v == r.version).unwrap_or(true)
            && arch.map(|a| a == r.arch).unwrap_or(true)
    }))
}

fn stable_id(a: &str, b: &str, c: &str, d: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    (a, b, c, d).hash(&mut hasher);
    hasher.finish()
}