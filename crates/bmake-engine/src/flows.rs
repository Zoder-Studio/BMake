use anyhow::{bail, Context, Result};
use bmake_ast::{FlowDef, Task};
use std::path::{Path, PathBuf};

/// Resolves `Uses: <path>` to `.bmake/flows/<path>.bm`, rooted at the
/// project directory (never at the file that declared `Uses:`).
pub fn flow_file_path(project_dir: &Path, flow_path: &str) -> PathBuf {
    let mut p = project_dir.join(".bmake").join("flows");
    for part in flow_path.split('/') {
        p = p.join(part);
    }
    p.set_extension("bm");
    p
}

/// Loads and parses a Plugin Flow. Path safety (no "..", no absolute
/// paths, no control characters) is already enforced at parse time in
/// bmake-parser; this only resolves the (already-validated) path to disk.
pub fn resolve_flow(project_dir: &Path, flow_path: &str) -> Result<FlowDef> {
    let file_path = flow_file_path(project_dir, flow_path);
    if !file_path.exists() {
        bail!(
            "BMake Error:\n\nPlugin Flow not found:\n\n{}\n\nExpected:\n\n{}",
            flow_path,
            file_path.display()
        );
    }
    let content = std::fs::read_to_string(&file_path).with_context(|| format!("Failed to read {}", file_path.display()))?;
    bmake_parser::flow::parse_flow_body(&content, flow_path)
}

/// Converts a resolved Plugin Flow into a Task so it's scheduled by the
/// exact same dependency graph and executor as any other Task. The Task's
/// name is the flow's path (e.g. "android/build"), so an existing
/// `Depends-on: android/build` in a regular Task resolves to it with no
/// special case anywhere in the graph/executor code.
pub fn materialize_as_task(flow: &FlowDef) -> Task {
    Task {
        name: flow.path.clone(),
        flow_label: Some(flow.name.clone()),
        before: flow.before.clone(),
        after: flow.after.clone(),
        commands: flow.commands.clone(),
        renames: flow.renames.clone(),
        depends_on: Vec::new(),
        inputs: flow.inputs.clone(),
        outputs: flow.outputs.clone(),
        artifacts: flow.artifacts.clone(),
        condition: flow.condition.clone(),
        workdir: flow.workdir.clone(),
        env: flow.env.clone(),
        timeout: flow.timeout,
        continue_on_error: flow.continue_on_error,
    }
}

/// Recursively discovers every Plugin Flow under `.bmake/flows/`, for
/// `bmake list flows` / `bmake syntax`. A flow that fails to parse is
/// reported alongside the others rather than aborting discovery.
pub fn discover_flows(project_dir: &Path) -> Vec<(String, Result<FlowDef>)> {
    let root = project_dir.join(".bmake").join("flows");
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Result<FlowDef>)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out);
            continue;
        }
        if path.extension().map(|e| e == "bm").unwrap_or(false) {
            let Ok(rel) = path.strip_prefix(root) else { continue };
            let flow_path = rel.with_extension("").to_string_lossy().replace('\\', "/");
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let parsed = bmake_parser::flow::parse_flow_body(&content, &flow_path);
            out.push((flow_path, parsed));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_nested_flow_and_materializes_as_task() {
        let dir = tempfile::tempdir().unwrap();
        let flow_dir = dir.path().join(".bmake/flows/android/release");
        std::fs::create_dir_all(&flow_dir).unwrap();
        std::fs::write(
            flow_dir.join("build.bm"),
            "Name: Android Release Build\nDescription: Build a release APK\n\nCommand: echo building",
        )
        .unwrap();

        let flow = resolve_flow(dir.path(), "android/release/build").unwrap();
        assert_eq!(flow.name, "Android Release Build");

        let task = materialize_as_task(&flow);
        assert_eq!(task.name, "android/release/build");
        assert_eq!(task.flow_label.as_deref(), Some("Android Release Build"));
        assert_eq!(task.commands[0].command, "echo building");
    }

    #[test]
    fn missing_flow_reports_expected_path() {
        let dir = tempfile::tempdir().unwrap();
        let err = resolve_flow(dir.path(), "android/build").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Plugin Flow not found"));
        assert!(msg.contains("android/build"));
        assert!(msg.contains(".bmake"));
    }
}