use crate::paths::BMakePaths;
use bmake_ast::BMakeFile;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Returns build-tool-specific cache environment variables when
/// `Cache = true` is set. Nothing is returned (and nothing overridden)
/// when caching is off.
pub fn cache_env(file: &BMakeFile, paths: &BMakePaths) -> HashMap<String, String> {
    let mut env = HashMap::new();
    if !file.cache {
        return env;
    }

    let cache_root = paths.cache();

    match file.system.as_deref() {
        Some("Gradle") => {
            env.insert("GRADLE_USER_HOME".to_string(), cache_root.join("gradle").display().to_string());
        }
        Some("Cargo") => {
            env.insert("CARGO_HOME".to_string(), cache_root.join("cargo").display().to_string());
        }
        Some("CMake") | Some("Make") => {
            if which::which("ccache").is_ok() {
                env.insert("CCACHE_DIR".to_string(), cache_root.join("ccache").display().to_string());
                env.insert("CC".to_string(), "ccache cc".to_string());
                env.insert("CXX".to_string(), "ccache c++".to_string());
            }
        }
        _ => {}
    }

    env
}

pub struct CacheInfo {
    pub location: PathBuf,
    pub size_bytes: u64,
    pub entries: usize,
    pub hits: u64,
    pub misses: u64,
}

pub fn info(paths: &BMakePaths) -> std::io::Result<CacheInfo> {
    let location = paths.cache();
    let (size_bytes, entries) = dir_stats(&location);
    let (hits, misses) = crate::incremental::stats(paths);
    Ok(CacheInfo { location, size_bytes, entries, hits, misses })
}

fn dir_stats(dir: &Path) -> (u64, usize) {
    let mut size = 0u64;
    let mut count = 0usize;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let (s, c) = dir_stats(&path);
                size += s;
                count += c;
            } else if let Ok(meta) = entry.metadata() {
                size += meta.len();
                count += 1;
            }
        }
    }
    (size, count)
}

/// Removes only `.bmake/cache/` — never `.bmake/engines/` or
/// `.bmake/dependencies/`, which are protected by design.
pub fn clear(paths: &BMakePaths) -> std::io::Result<()> {
    let dir = paths.cache();
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(())
}