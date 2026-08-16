use crate::paths::BMakePaths;
use anyhow::{bail, Result};
use bmake_ast::{Dependency, ToolReq};
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

pub fn ensure_dependencies(deps: &[Dependency]) -> Result<()> {
    for dep in deps {
        if which::which(&dep.need).is_ok() {
            println!(" Dependency OK: {} ({})", dep.name, dep.need);
            continue;
        }

        let Some(pm) = detect_package_manager() else {
            bail!(" No recognized package manager found to install '{}'", dep.need);
        };

        println!(" Installing dependency '{}' via {}...", dep.need, pm);
        let status = match pm {
            "pkg" => Command::new("pkg").args(["install", "-y", &dep.need]).status()?,
            "apt-get" | "apt" => Command::new("sudo").args([pm, "install", "-y", &dep.need]).status()?,
            "brew" => Command::new("brew").args(["install", &dep.need]).status()?,
            "dnf" => Command::new("sudo").args(["dnf", "install", "-y", &dep.need]).status()?,
            "pacman" => Command::new("sudo").args(["pacman", "-S", "--noconfirm", &dep.need]).status()?,
            _ => bail!("Unsupported package manager: {}", pm),
        };

        if !status.success() {
            bail!(" Failed to install dependency '{}'", dep.need);
        }
    }
    Ok(())
}

/// Maps a BMake `Tool:` name to the package name a given package manager
/// actually publishes it under, for the handful of tools where they diverge.
/// Anything not listed falls back to the lowercased tool name.
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
/// detected system package manager. Installed tools are also symlinked into
/// `.bmake/tools/bin` so the sandboxed PATH picks them up on later runs.
pub fn ensure_tools(tools: &[ToolReq], paths: &BMakePaths) -> Result<()> {
    for tool in tools {
        if let Ok(resolved) = which::which(&tool.name) {
            println!(" Tool OK: {} (requested {}) -> {}", tool.name, tool.need, resolved.display());
            check_tool_version(&tool.name, &tool.need);
            link_into_sandbox(&tool.name, &resolved, paths);
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
        link_into_sandbox(&tool.name, &resolved, paths);
    }
    Ok(())
}

/// Best-effort version confirmation by running `<tool> --version` and
/// checking whether the requested version string shows up in the output.
/// Not authoritative — output formats vary wildly between tools — so a
/// mismatch is only a warning, never a hard failure.
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

fn link_into_sandbox(name: &str, resolved: &std::path::Path, paths: &BMakePaths) {
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