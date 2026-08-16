use anyhow::{bail, Context, Result};
use bmake_ast::BMakeFile;
use std::path::Path;
use std::process::Command;

/// Locates a usable Kotlin script runner. Prefers the `kotlin` launcher
/// (ships with the Kotlin compiler distribution and runs `.kts` scripts
/// directly on the JVM); falls back to `kotlinc -script` if only the
/// compiler binary is present. No custom Kotlin interpreter is written —
/// per the spec, BMake must delegate to a real Kotlin scripting runtime.
fn find_kotlin_runner() -> Option<(&'static str, Vec<&'static str>)> {
    if which::which("kotlin").is_ok() {
        return Some(("kotlin", vec![]));
    }
    if which::which("kotlinc").is_ok() {
        return Some(("kotlinc", vec!["-script"]));
    }
    None
}

/// Runs a `.bm.kts` file: transpiles it to a plain Kotlin script that
/// prints the flattened BMake DSL, executes it on the JVM, then feeds the
/// resulting text through the exact same parser/model used for `.bm` — so
/// both formats end up on the same BMake Engine, as required.
pub fn parse_kts_file(path: &Path) -> Result<BMakeFile> {
    let Some((program, base_args)) = find_kotlin_runner() else {
        bail!(
            " No Kotlin scripting runtime found (looked for 'kotlin' and 'kotlinc' on PATH). \
Install the Kotlin compiler (e.g. 'pkg install kotlin' on Termux, or via SDKMAN) to run .bm.kts files."
        );
    };

    let source = std::fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let script = bmake_parser::transpile_kts_to_kotlin_script(&source);

    let tmp_dir = std::env::temp_dir();
    let script_path = tmp_dir.join(format!(
        "bmake-kts-{}.kts",
        path.file_stem().and_then(|s| s.to_str()).unwrap_or("script")
    ));
    std::fs::write(&script_path, &script)?;

    let output = Command::new(program)
        .args(&base_args)
        .arg(&script_path)
        .output()
        .with_context(|| format!("Failed to run {} on {}", program, script_path.display()))?;

    let _ = std::fs::remove_file(&script_path);

    if !output.status.success() {
        bail!(
            " Kotlin script evaluation failed for {}:\n{}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let flattened = String::from_utf8(output.stdout)
        .with_context(|| format!("Kotlin script output for {} was not valid UTF-8", path.display()))?;

    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    bmake_parser::parse_kts_output(&flattened, base_dir, path)
        .with_context(|| format!("Failed to parse the BMake DSL produced by {}", path.display()))
}