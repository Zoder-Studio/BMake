use anyhow::{bail, Result};
use bmake_ast::BMakeFile;
use std::path::Path;

/// A BMake plugin adds optional post-build behavior without affecting the
/// core grammar or execution engine, per the spec's "Plugin" directive.
pub trait Plugin {
    fn name(&self) -> &'static str;
    fn after_build(&self, file: &BMakeFile, project_dir: &Path) -> Result<()>;
}

/// Built-in plugin: sanity-checks that an Android build produced an APK
/// artifact inside the declared `Output` directory.
pub struct AndroidPlugin;

impl Plugin for AndroidPlugin {
    fn name(&self) -> &'static str {
        "Android"
    }

    fn after_build(&self, file: &BMakeFile, project_dir: &Path) -> Result<()> {
        let Some(output) = &file.output else {
            return Ok(());
        };
        let output_dir = project_dir.join(output);
        if !output_dir.exists() {
            bail!(
                "Android plugin: output directory '{}' was not produced by the build",
                output_dir.display()
            );
        }

        let has_apk = std::fs::read_dir(&output_dir)?
            .filter_map(|e| e.ok())
            .any(|e| e.path().extension().map(|ext| ext == "apk").unwrap_or(false));

        if has_apk {
            println!(" Android plugin: APK artifact found in {}", output_dir.display());
        } else {
            println!(
                " Android plugin: no .apk found in {} — check your build task",
                output_dir.display()
            );
        }
        Ok(())
    }
}

pub fn resolve(name: &str) -> Option<Box<dyn Plugin>> {
    match name {
        "Android" => Some(Box::new(AndroidPlugin)),
        _ => None,
    }
}

pub fn run_after_build(file: &BMakeFile, project_dir: &Path) -> Result<()> {
    for name in &file.plugins {
        match resolve(name) {
            Some(plugin) => plugin.after_build(file, project_dir)?,
            None => println!(" Unknown plugin '{}' — skipped (no core dependency created)", name),
        }
    }
    Ok(())
}