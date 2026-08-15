use anyhow::{bail, Result};
use bmake_ast::Dependency;
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