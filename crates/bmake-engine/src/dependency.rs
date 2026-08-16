use crate::paths::BMakePaths;
use anyhow::{bail, Context, Result};
use bmake_ast::{Dependency, ToolReq};
use std::path::Path;
use std::process::Command;

pub fn check_require(name: &str) -> bool {
    which::which(name).is_ok()
}

pub fn ensure_requires(requires: &[String]) -> Result<()> {
    for r in requires {
        if !check_require(r) {
            bail!(" Required dependency not found: {}", r);
        }
        println!(" Require OK: {}", r);
    }
    Ok(())
}

fn detect_package_manager() -> Option<&'static str> {
    ["pkg", "apt-get", "apt", "brew", "dnf", "pacman"]
        .into_iter()
        .find(|pm| which::which(pm).is_ok())
}

/// Installs missing dependencies. For apt/pkg (Debian-family, including
/// Termux) this is fully sandboxed: the package is downloaded and extracted
/// into `.bmake/dependencies/<name>/` without touching the system, and its
/// binaries are symlinked into `.bmake/tools/bin`. Other package managers
/// don't have a safe rootless prefix-install path yet, so they fall back to
/// a real system-wide install (clearly labeled as such in the output).
pub fn ensure_dependencies(deps: &[Dependency], paths: &BMakePaths) -> Result<()> {
    for dep in deps {
        if which::which(&dep.need).is_ok() {
            println!(" Dependency OK: {} ({})", dep.name, dep.need);
            continue;
        }

        let Some(pm) = detect_package_manager() else {
            bail!(" No recognized package manager found to install '{}'", dep.need);
        };

        match pm {
            "pkg" | "apt-get" | "apt" => install_deb_sandboxed(&dep.need, paths)?,
            "brew" => {
                println!(" Installing '{}' via brew (system-wide — brew has no sandboxed prefix install yet)...", dep.need);
                let status = Command::new("brew").args(["install", &dep.need]).status()?;
                if !status.success() {
                    bail!(" Failed to install dependency '{}'", dep.need);
                }
            }
            "dnf" => {
                println!(" Installing '{}' via dnf (system-wide — sandboxed install not implemented for dnf yet)...", dep.need);
                let status = Command::new("sudo").args(["dnf", "install", "-y", &dep.need]).status()?;
                if !status.success() {
                    bail!(" Failed to install dependency '{}'", dep.need);
                }
            }
            "pacman" => {
                println!(" Installing '{}' via pacman (system-wide — sandboxed install not implemented for pacman yet)...", dep.need);
                let status = Command::new("sudo").args(["pacman", "-S", "--noconfirm", &dep.need]).status()?;
                if !status.success() {
                    bail!(" Failed to install dependency '{}'", dep.need);
                }
            }
            _ => bail!("Unsupported package manager: {}", pm),
        }
    }
    Ok(())
}

fn install_deb_sandboxed(package: &str, paths: &BMakePaths) -> Result<()> {
    let dep_dir = paths.dependencies().join(package);
    std::fs::create_dir_all(&dep_dir)?;

    let downloader = if which::which("apt-get").is_ok() { "apt-get" } else { "pkg" };
    println!(" Downloading '{}' via '{} download' (sandboxed, no system install)...", package, downloader);

    let download_dir = dep_dir.join("_download");
    std::fs::create_dir_all(&download_dir)?;

    let status = Command::new(downloader)
        .args(["download", package])
        .current_dir(&download_dir)
        .status()
        .with_context(|| format!("Failed to run '{} download {}'", downloader, package))?;

    if !status.success() {
        bail!(" Failed to download package '{}' via '{} download'", package, downloader);
    }

    let deb_file = std::fs::read_dir(&download_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().map(|e| e == "deb").unwrap_or(false));

    let Some(deb_file) = deb_file else {
        bail!(" No .deb file was produced for package '{}'", package);
    };

    if which::which("dpkg-deb").is_err() {
        bail!(" 'dpkg-deb' is required to extract sandboxed packages but was not found on PATH");
    }

    let extract_status = Command::new("dpkg-deb")
        .args(["-x", deb_file.to_str().unwrap_or_default(), dep_dir.to_str().unwrap_or_default()])
        .status()
        .with_context(|| "Failed to run dpkg-deb -x")?;

    if !extract_status.success() {
        bail!(" Failed to extract '{}'", deb_file.display());
    }

    let _ = std::fs::remove_dir_all(&download_dir);
    link_binaries_into_sandbox(&dep_dir, paths)?;

    println!(" Dependency '{}' installed to {} (sandboxed)", package, dep_dir.display());
    Ok(())
}

fn link_binaries_into_sandbox(extracted_root: &Path, paths: &BMakePaths) -> Result<()> {
    let bin_dir = paths.tools().join("bin");
    std::fs::create_dir_all(&bin_dir)?;

    for candidate in ["usr/bin", "usr/sbin", "bin", "sbin"] {
        let dir = extracted_root.join(candidate);
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(&dir)?.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name() else {
                continue;
            };
            let link = bin_dir.join(name);

            #[cfg(unix)]
            {
                use std::os::unix::fs::symlink;
                let _ = std::fs::remove_file(&link);
                let _ = symlink(&path, &link);
            }
            #[cfg(windows)]
            {
                let _ = std::fs::remove_file(&link);
                let _ = std::fs::copy(&path, &link);
            }
        }
    }
    Ok(())
}

/// Maps a BMake `Tool:` name to the package name a given package manager
/// actually publishes it under, for the handful of tools where they diverge.
fn tool_package_name(pm: &str, tool: &str) -> String {
    let lower = tool.to_lowercase();
    match (pm, lower.as_str()) {
        ("apt-get", "ninja") | ("apt", "ninja") => "ninja-build".to_string(),
        ("dnf", "ninja") => "ninja-build".to_string(),
        ("pkg", "ninja") | ("brew", "ninja") | ("pacman", "ninja") => "ninja".to_string(),
        _ => lower,
    }
}

/// Checks whether `tools` are available, installing missing ones via the
/// detected system package manager. Installed tools are symlinked into
/// `.bmake/tools/bin` so the sandboxed PATH picks them up on later runs.
pub fn ensure_tools(tools: &[ToolReq], paths: &BMakePaths) -> Result<()> {
    for tool in tools {
        if let Ok(resolved) = which::which(&tool.name) {
            println!(" Tool OK: {} (requested {}) -> {}", tool.name, tool.need, resolved.display());
            check_tool_version(&tool.name, &tool.need);
            link_tool_into_sandbox(&tool.name, &resolved, paths);
            continue;
        }

        let Some(pm) = detect_package_manager() else {
            bail!(
                " Required tool not found and no package manager available to install it: {} (need {})",
                tool.name,
                tool.need
            );
        };

        let package = tool_package_name(pm, &tool.name);
        println!(" Installing tool '{}' (package '{}') via {}...", tool.name, package, pm);

        let status = match pm {
            "pkg" => Command::new("pkg").args(["install", "-y", &package]).status()?,
            "apt-get" | "apt" => Command::new("sudo").args([pm, "install", "-y", &package]).status()?,
            "brew" => Command::new("brew").args(["install", &package]).status()?,
            "dnf" => Command::new("sudo").args(["dnf", "install", "-y", &package]).status()?,
            "pacman" => Command::new("sudo").args(["pacman", "-S", "--noconfirm", &package]).status()?,
            _ => bail!("Unsupported package manager: {}", pm),
        };

        let Ok(resolved) = which::which(&tool.name) else {
            bail!(" Failed to install tool '{}' (package '{}')", tool.name, package);
        };
        if !status.success() {
            bail!(" Failed to install tool '{}' (package '{}')", tool.name, package);
        }

        println!(" Tool installed: {} (need {})", tool.name, tool.need);
        check_tool_version(&tool.name, &tool.need);
        link_tool_into_sandbox(&tool.name, &resolved, paths);
    }
    Ok(())
}

/// Best-effort version confirmation by running `<tool> --version` and
/// checking whether the requested version string shows up in the output.
/// Not authoritative — output formats vary between tools — so a mismatch
/// is only a warning, never a hard failure.
fn check_tool_version(name: &str, need: &str) {
    let Ok(output) = Command::new(name).arg("--version").output() else {
        return;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = if stdout.trim().is_empty() {
        String::from_utf8_lossy(&output.stderr).to_string()
    } else {
        stdout.to_string()
    };

    if text.contains(need) {
        println!("   version matches requested '{}'", need);
    } else {
        let first_line = text.trim().lines().next().unwrap_or("");
        println!("   warning: could not confirm version '{}' — tool reports: {}", need, first_line);
    }
}

fn link_tool_into_sandbox(name: &str, resolved: &Path, paths: &BMakePaths) {
    let bin_dir = paths.tools().join("bin");
    if std::fs::create_dir_all(&bin_dir).is_err() {
        return;
    }
    let link = bin_dir.join(name);

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let _ = std::fs::remove_file(&link);
        let _ = symlink(resolved, &link);
    }
    #[cfg(windows)]
    {
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::copy(resolved, &link);
    }
}