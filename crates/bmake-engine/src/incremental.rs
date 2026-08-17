use crate::paths::BMakePaths;
use anyhow::Result;
use bmake_ast::Task;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
struct IncrementalState {
    #[serde(default)]
    task: HashMap<String, String>,
}

fn state_path(paths: &BMakePaths) -> PathBuf {
    paths.cache().join("incremental.toml")
}

fn load_state(paths: &BMakePaths) -> IncrementalState {
    let path = state_path(paths);
    std::fs::read_to_string(&path).ok().and_then(|c| toml::from_str(&c).ok()).unwrap_or_default()
}

fn save_state(paths: &BMakePaths, state: &IncrementalState) -> Result<()> {
    let path = state_path(paths);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, toml::to_string_pretty(state)?)?;
    Ok(())
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheStats {
    #[serde(default)]
    hits: u64,
    #[serde(default)]
    misses: u64,
}

fn stats_path(paths: &BMakePaths) -> PathBuf {
    paths.cache().join("stats.toml")
}

fn load_stats(paths: &BMakePaths) -> CacheStats {
    std::fs::read_to_string(stats_path(paths)).ok().and_then(|c| toml::from_str(&c).ok()).unwrap_or_default()
}

fn save_stats(paths: &BMakePaths, stats: &CacheStats) {
    if let Some(parent) = stats_path(paths).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(stats_path(paths), toml::to_string_pretty(stats).unwrap_or_default());
}

/// Called by the executor whenever a Task is skipped because its Input
/// fingerprint hasn't changed.
pub fn record_hit(paths: &BMakePaths) {
    let mut s = load_stats(paths);
    s.hits += 1;
    save_stats(paths, &s);
}

fn record_miss(paths: &BMakePaths) {
    let mut s = load_stats(paths);
    s.misses += 1;
    save_stats(paths, &s);
}

pub fn stats(paths: &BMakePaths) -> (u64, u64) {
    let s = load_stats(paths);
    (s.hits, s.misses)
}

fn fingerprint(project_dir: &Path, inputs: &[String]) -> Option<String> {
    if inputs.is_empty() {
        return None;
    }

    let mut entries: Vec<(String, u64, u64)> = Vec::new();
    for input in inputs {
        collect_files(&project_dir.join(input), &mut entries);
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (path, size, mtime) in &entries {
        hasher.update(path.as_bytes());
        hasher.update(size.to_le_bytes());
        hasher.update(mtime.to_le_bytes());
    }
    let digest = hasher.finalize();
    Some(digest.iter().map(|b| format!("{:02x}", b)).collect())
}

fn collect_files(root: &Path, out: &mut Vec<(String, u64, u64)>) {
    let Ok(meta) = std::fs::metadata(root) else {
        return;
    };

    if meta.is_file() {
        let size = meta.len();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        out.push((root.display().to_string(), size, mtime));
        return;
    }

    if meta.is_dir() {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            collect_files(&entry.path(), out);
        }
    }
}

fn outputs_exist(project_dir: &Path, outputs: &[String]) -> bool {
    if outputs.is_empty() {
        return false;
    }
    outputs.iter().all(|o| project_dir.join(o).exists())
}

pub fn is_up_to_date(paths: &BMakePaths, project_dir: &Path, task: &Task) -> bool {
    if task.inputs.is_empty() || task.outputs.is_empty() {
        return false;
    }
    let Some(current) = fingerprint(project_dir, &task.inputs) else {
        return false;
    };
    if !outputs_exist(project_dir, &task.outputs) {
        return false;
    }
    let state = load_state(paths);
    state.task.get(&task.name).map(|f| f == &current).unwrap_or(false)
}

/// Records the current Input fingerprint after a task finishes successfully,
/// and counts it as a cache miss (it had to actually run).
pub fn record(paths: &BMakePaths, project_dir: &Path, task: &Task) -> Result<()> {
    record_miss(paths);
    let Some(current) = fingerprint(project_dir, &task.inputs) else {
        return Ok(());
    };
    let mut state = load_state(paths);
    state.task.insert(task.name.clone(), current);
    save_state(paths, &state)
}