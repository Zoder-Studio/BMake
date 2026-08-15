use crate::paths::BMakePaths;
use std::collections::HashMap;
use std::io;

pub struct SandboxEnv {
    pub path: String,
    pub extra: HashMap<String, String>,
}

/// Prepends BMake-managed tool directories to PATH so dependencies BMake
/// installs are found before system-wide ones, and exposes
/// BMAKE_SANDBOX_HOME for tools that want to opt into an isolated home.
///
/// Deliberately does NOT override the real HOME/SSH/git credentials — most
/// build tools (npm private registries, git submodules, gradle signing)
/// need access to the user's real environment, so sandboxing here is
/// additive (PATH priority) rather than a full jail.
pub fn build_sandbox_env(paths: &BMakePaths) -> io::Result<SandboxEnv> {
    let tools_bin = paths.tools().join("bin");
    let sandbox_bin = paths.sandbox().join("bin");
    let sandbox_home = paths.sandbox().join("home");

    std::fs::create_dir_all(&tools_bin)?;
    std::fs::create_dir_all(&sandbox_bin)?;
    std::fs::create_dir_all(&sandbox_home)?;

    let existing_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{}:{}", sandbox_bin.display(), tools_bin.display(), existing_path);

    let mut extra = HashMap::new();
    extra.insert("BMAKE_SANDBOX_HOME".to_string(), sandbox_home.display().to_string());

    Ok(SandboxEnv { path, extra })
}