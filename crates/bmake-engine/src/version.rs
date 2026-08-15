use crate::paths::BMakePaths;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Version format follows the BMake spec (e.g. "1.0", "1.8.4-maintenance"),
/// not strict SemVer — decoupled from the Cargo package version.
pub const CURRENT_ENGINE_VERSION: &str = "1.0";

const GITHUB_OWNER: &str = "Zoder-Studio";
const GITHUB_REPO: &str = "BMake";

pub fn engine_dir_for(paths: &BMakePaths, version: &str) -> PathBuf {
    paths.engines().join(version)
}

pub fn resolve_tag(version: &str) -> String {
    format!("v{}", version)
}

/// Ensures the requested engine version is available locally, downloading
/// it from GitHub Releases (with optional sha256 verification) if needed.
/// If the requested version matches the running CLI, no download happens.
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
    let resp = reqwest::blocking::get(&url).with_context(|| format!("Failed to reach {}", url))?;

    if !resp.status().is_success() {
        bail!(
            " BMake Engine {} was not found on GitHub Releases ({}). Make sure tag '{}' exists in {}/{}.",
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

    if let Some(expected) = fetch_checksum(&url) {
        verify_checksum(&bin, &expected)?;
        println!(" Checksum verified for engine {}", version);
    } else {
        println!(" No checksum published for this asset — skipping verification");
    }

    println!(" BMake Engine {} downloaded to {}", version, dir.display());
    Ok(bin)
}

fn fetch_checksum(asset_url: &str) -> Option<String> {
    let checksum_url = format!("{}.sha256", asset_url);
    let resp = reqwest::blocking::get(&checksum_url).ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let text = resp.text().ok()?;
    text.split_whitespace().next().map(|s| s.to_lowercase())
}

fn verify_checksum(bin: &Path, expected_hex: &str) -> Result<()> {
    let data = std::fs::read(bin)?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let digest = hasher.finalize();
    let actual_hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();

    if actual_hex != expected_hex {
        std::fs::remove_file(bin).ok();
        bail!(
            "Checksum mismatch for {}: expected {}, got {}",
            bin.display(),
            expected_hex,
            actual_hex
        );
    }
    Ok(())
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