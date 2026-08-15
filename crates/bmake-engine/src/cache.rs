use crate::paths::BMakePaths;
use bmake_ast::BMakeFile;
use std::collections::HashMap;

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
            env.insert(
                "GRADLE_USER_HOME".to_string(),
                cache_root.join("gradle").display().to_string(),
            );
        }
        Some("Cargo") => {
            env.insert(
                "CARGO_HOME".to_string(),
                cache_root.join("cargo").display().to_string(),
            );
        }
        Some("CMake") | Some("Make") => {
            if which::which("ccache").is_ok() {
                env.insert(
                    "CCACHE_DIR".to_string(),
                    cache_root.join("ccache").display().to_string(),
                );
                env.insert("CC".to_string(), "ccache cc".to_string());
                env.insert("CXX".to_string(), "ccache c++".to_string());
            }
        }
        _ => {}
    }

    env
}