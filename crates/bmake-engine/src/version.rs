use crate::paths::BMakePaths;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;

/// Version format follows the BMake spec (e.g. "1.0", "1.8.4-maintenance"),
/// not strict SemVer — decoupled from the Cargo package version.
pub const CURRENT_ENGINE_VERSION: &str = "1.0";

const GITHUB_OWNER: &str = "BMake";
const GITHUB_REPO: &str = "BMake";

pub fn engine_dir_for(paths: &BMakePaths, version: &str) -> PathBuf {
    paths.engines().join(version)
}

pub fn resolve_tag(version: &str) -> String {
    format!("v{}", version)
}

/// Ensures the requested engine version is available locally, downloading
/// it from GitHub Releases if necessary. If the requested version matches
/// the currently running CLI, no download happens.
pub fn ensure_engine(paths: &BMakePaths, version: &str) -> Result<PathBuf> {
    if version == CURRENT_ENGINE_VERSION {
        return Ok(std::env::current_exe()?);
    }

    let dir = engine_dir_for(paths, version);
    let bin = dir.join("bmake-engine");
    if bin.exists() {
        return Ok(bin);
    }

    std::fs::create_dir_all(&dir)?;
    let tag = resolve_tag(version);
    let asset_name = format!("bmake-engine-{}-{}", target_platform(), target_arch());
    let url = format!(
        "https://github.com/{}/{}/releases/download/{}/{}",
        GITHUB_OWNER, GITHUB_REPO, tag, asset_name
    );

    println!(" Downloading BMake Engine {} from GitHub...", version);
    let resp = reqwest::blocking::get(&url).with_context(|| format!("Failed to Called {}", url))?;

    if !resp.status().is_success() {
        bail!(
            " BMake Engine {} was not found in GitHub Releases ({}). Make sure the '{}' tag is released in {}/{}.",
            version,
            resp.status(),
            tag,
            GITHUB_OWNER,
            GITHUB_REPO
        );
    }

    let bytes = resp.bytes()?;
    std::fs::write(&bin, &bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&bin)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms)?;
    }

    println!(" BMake Engine {} successfully downloaded to {}", version, dir.display());
    Ok(bin)
}

pub fn target_platform() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unknown"
    }
}

pub fn target_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "arm") {
        "arm"
    } else if cfg!(target_arch = "x86") {
        "x86"
    } else {
        "unknown"
    }
}