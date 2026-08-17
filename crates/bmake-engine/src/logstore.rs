use crate::metadata::BuildMetadata;
use crate::paths::BMakePaths;
use anyhow::Result;
use std::io::Write;
use std::path::PathBuf;

pub fn log_dir(paths: &BMakePaths) -> PathBuf {
    paths.root.join("logs")
}

pub fn log_path(paths: &BMakePaths, build_id: &str) -> PathBuf {
    log_dir(paths).join(format!("{}.log", build_id))
}

pub fn meta_path(paths: &BMakePaths, build_id: &str) -> PathBuf {
    log_dir(paths).join(format!("{}.meta.toml", build_id))
}

/// Appends one line to a build's persisted log — called from the executor
/// for every line of real command stdout/stderr, so `bmake logs` reflects
/// what actually happened, not a summary reconstructed after the fact.
pub fn append_line(paths: &BMakePaths, build_id: &str, line: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(log_dir(paths))?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(log_path(paths, build_id))?;
    writeln!(f, "{}", line)
}

pub fn write_meta(paths: &BMakePaths, build_id: &str, meta: &BuildMetadata) -> Result<()> {
    std::fs::create_dir_all(log_dir(paths))?;
    std::fs::write(meta_path(paths, build_id), toml::to_string_pretty(meta)?)?;
    Ok(())
}

pub fn read_meta(paths: &BMakePaths, build_id: &str) -> Option<BuildMetadata> {
    let content = std::fs::read_to_string(meta_path(paths, build_id)).ok()?;
    toml::from_str(&content).ok()
}

pub fn read_log(paths: &BMakePaths, build_id: &str) -> std::io::Result<String> {
    std::fs::read_to_string(log_path(paths, build_id))
}

/// Lists known build IDs, oldest first (based on log file mtime).
pub fn list_builds(paths: &BMakePaths) -> std::io::Result<Vec<String>> {
    let dir = log_dir(paths);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut ids: Vec<(String, std::time::SystemTime)> = std::fs::read_dir(dir)?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let id = name.strip_suffix(".log")?.to_string();
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((id, modified))
        })
        .collect();
    ids.sort_by_key(|(_, t)| *t);
    Ok(ids.into_iter().map(|(id, _)| id).collect())
}