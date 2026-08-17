use crate::paths::BMakePaths;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskMeta {
    pub name: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BuildMetadata {
    pub bmake_version: String,
    pub build_id: String,
    pub status: String,
    pub start_time: u64,
    pub end_time: u64,
    pub duration_secs: u64,
    pub runs_on: Option<String>,
    pub runs_on_version: Option<String>,
    pub arch: Option<String>,
    pub platform: Option<String>,
    pub system: Option<String>,
    pub sub_system: Option<String>,
    pub remote: Option<String>,
    pub tasks: Vec<TaskMeta>,
    pub exit_code: i32,
}

pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

pub fn new_build_id() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("bmk_{}_{:05}", now.as_secs(), now.subsec_micros() % 100000)
}

pub fn write(paths: &BMakePaths, meta: &BuildMetadata) -> Result<()> {
    let path = paths.root.join("last-build.toml");
    std::fs::write(&path, toml::to_string_pretty(meta)?)?;
    Ok(())
}