use crate::{parse_multiline_command, strip_field};
use anyhow::{bail, Result};
use bmake_ast::*;
use crate::parse;

/// Parses a Plugin Flow file's body: a flat directive list (no
/// `<Version:>`, `Start`/`Stop`, or `<Task:>` wrapper — a flow IS the
/// execution unit, not a container of them). Reuses the exact same
/// directive tokens (`Command:`, `Env:`, `Depends-on` is deliberately NOT
/// supported here — flows are depended-on by name from regular Tasks, not
/// the other way around) as the rest of BMake.
pub fn parse_flow_body(content: &str, flow_path: &str) -> Result<FlowDef> {
    let lines = bmake_lexer::to_lines(content);
    let mut flow = FlowDef {
        path: flow_path.to_string(),
        ..Default::default()
    };
    let mut pending_dependency: Option<String> = None;
    let mut pending_tool: Option<String> = None;

    let mut i = 0;
    let n = lines.len();
    while i < n {
        let t = lines[i].trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        if t == "Start" || t == "Stop" || t.starts_with("<Version:") || t.starts_with("<Task:") {
            bail!(
                "Invalid Plugin Flow '{}': flow files are a flat directive body — they must not contain <Version:>, Start, Stop, or <Task:> (line {})",
                flow_path,
                i + 1
            );
        }
        if t == "Value:" {
            bail!(
                "Invalid Plugin Flow '{}': 'Value:' must be declared in the main project file, not inside a flow (line {})",
                flow_path,
                i + 1
            );
        }
        if t.starts_with("Uses:") {
            bail!("Invalid Plugin Flow '{}': a flow cannot itself contain 'Uses:' (line {})", flow_path, i + 1);
        }

        if let Some(rest) = t.strip_prefix("Name:") {
            flow.name = rest.trim().to_string();
            i += 1;
            continue;
        }
        if let Some(rest) = t.strip_prefix("Description:") {
            flow.description = rest.trim().to_string();
            i += 1;
            continue;
        }
        if let Some(rest) = t.strip_prefix("Dependency:") {
            pending_dependency = Some(rest.trim().to_string());
            pending_tool = None;
            i += 1;
            continue;
        }
        if let Some(rest) = t.strip_prefix("Tool:") {
            pending_tool = Some(rest.trim().to_string());
            pending_dependency = None;
            i += 1;
            continue;
        }
        if let Some(rest) = t.strip_prefix("Need:") {
            if let Some(name) = pending_dependency.take() {
                flow.dependencies.push(Dependency { name, need: rest.trim().to_string() });
            } else if let Some(name) = pending_tool.take() {
                flow.tools.push(ToolReq { name, need: rest.trim().to_string() });
            } else {
                bail!("'Need:' without preceding 'Dependency:' or 'Tool:' in flow '{}' at line {}", flow_path, i + 1);
            }
            i += 1;
            continue;
        }
        if let Some(rest) = t.strip_prefix("Require:") {
            flow.requires.push(rest.trim().to_string());
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Before") {
            flow.before.push(v);
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "After") {
            flow.after.push(v);
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Rename") {
            let Some((from, to)) = v.split_once("->") else {
                bail!("Malformed Rename in flow '{}' at line {}: expected 'from -> to'", flow_path, i + 1);
            };
            flow.renames.push((from.trim().to_string(), to.trim().to_string()));
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "OnError") {
            let on_error = match v.as_str() {
                "stop" => OnError::Stop,
                "retry" => OnError::Retry,
                other => bail!("Unknown OnError value '{}' in flow '{}' at line {}", other, flow_path, i + 1),
            };
            if let Some(last) = flow.commands.last_mut() {
                last.on_error = Some(on_error);
            }
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Retry") {
            let retry: u32 = v.parse()?;
            if let Some(last) = flow.commands.last_mut() {
                last.retry = Some(retry);
            }
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Timeout") {
            let timeout: u64 = v.parse()?;
            if let Some(last) = flow.commands.last_mut() {
                last.timeout = Some(timeout);
            } else {
                flow.timeout = Some(timeout);
            }
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "ContinueOnError") {
            flow.continue_on_error = Some(v.eq_ignore_ascii_case("true"));
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Input") {
            flow.inputs.push(v);
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Output") {
            flow.outputs.push(v);
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Artifact") {
            flow.artifacts.push(v);
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Condition") {
            flow.condition = Some(v);
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Workdir") {
            flow.workdir = Some(v);
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Env") {
            let Some((k, val)) = v.split_once('=') else {
                bail!("Malformed Env in flow '{}' at line {}: expected KEY=VALUE", flow_path, i + 1);
            };
            flow.env.insert(k.trim().to_string(), val.trim().to_string());
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Command") {
            if v == "{" {
                let (cmd, next_i) = parse_multiline_command(&lines, i + 1)?;
                flow.commands.push(CommandStep { command: cmd, ..Default::default() });
                i = next_i;
            } else {
                flow.commands.push(CommandStep { command: v, ..Default::default() });
                i += 1;
            }
            continue;
        }

        if let Some((key, val)) = t.split_once(':') {
            let (key, val) = (key.trim().to_string(), val.trim().to_string());
            match key.as_str() {
                "Runs-on" => flow.runs_on = Some(val),
                "version" => flow.runs_on_version = Some(val),
                "Arch" => flow.arch = Some(val),
                "Platform" => flow.platform = Some(val),
                "Shell" => flow.shell = Some(val),
                other => bail!("Unknown directive '{}' in Plugin Flow '{}' at line {}", other, flow_path, i + 1),
            }
            i += 1;
            continue;
        }

        bail!("Unrecognized syntax in Plugin Flow '{}' at line {}: '{}'", flow_path, i + 1, t);
    }

    if flow.name.trim().is_empty() {
        bail!("BMake Error:\n\nInvalid Plugin Flow:\n\n{}\n\nMissing required metadata:\n\nName:", flow_path);
    }
    if flow.description.trim().is_empty() {
        bail!(
            "BMake Error:\n\nInvalid Plugin Flow:\n\n{}\n\nMissing required metadata:\n\nDescription:",
            flow_path
        );
    }

    Ok(flow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_flow_parses() {
        let src = "Name: Android Build\nDescription: Build Android application\n\nRuns-on: android-builder\nversion: 1.0\n\nCommand: ./gradlew assembleRelease\nOutput: build/*.apk";
        let flow = parse_flow_body(src, "android/build").unwrap();
        assert_eq!(flow.name, "Android Build");
        assert_eq!(flow.runs_on.as_deref(), Some("android-builder"));
        assert_eq!(flow.commands[0].command, "./gradlew assembleRelease");
        assert_eq!(flow.outputs, vec!["build/*.apk".to_string()]);
    }

    #[test]
    fn missing_name_is_rejected() {
        let src = "Description: Build Android application\n\nCommand: echo hi";
        let err = parse_flow_body(src, "android/build").unwrap_err();
        assert!(err.to_string().contains("Missing required metadata:\n\nName:"));
    }

    #[test]
    fn missing_description_is_rejected() {
        let src = "Name: Android Build\n\nCommand: echo hi";
        let err = parse_flow_body(src, "android/build").unwrap_err();
        assert!(err.to_string().contains("Missing required metadata:\n\nDescription:"));
    }

    #[test]
    fn version_start_stop_and_task_are_rejected_inside_flow() {
        let src = "Name: X\nDescription: Y\n\nStart\n";
        let err = parse_flow_body(src, "x").unwrap_err();
        assert!(err.to_string().contains("must not contain"));
    }

    #[test]
    fn nested_uses_in_flow_is_rejected() {
        let src = "Name: X\nDescription: Y\n\nUses: other/flow";
        let err = parse_flow_body(src, "x").unwrap_err();
        assert!(err.to_string().contains("cannot itself contain 'Uses:'"));
    }

    #[test]
    fn uses_directive_parses_valid_path() {
        let src = "<Version: 1.0>\n\nStart\n\nUses: android/build\n\nStop";
        let file = parse(src).unwrap();
        assert_eq!(file.uses, vec!["android/build".to_string()]);
    }

    #[test]
    fn uses_rejects_path_traversal() {
        let src = "<Version: 1.0>\n\nStart\n\nUses: ../../secret\n\nStop";
        let err = parse(src).unwrap_err();
        assert!(err.to_string().contains("path traversal"));
    }

    #[test]
    fn uses_rejects_absolute_path() {
        let src = "<Version: 1.0>\n\nStart\n\nUses: /etc/passwd\n\nStop";
        let err = parse(src).unwrap_err();
        assert!(err.to_string().contains("absolute paths"));
    }

    #[test]
    fn uses_inside_task_is_rejected() {
        let src = "<Version: 1.0>\n\nStart\n\n<Task: Build>\n    Uses: android/build\n    Command: echo hi\n</Task>\n\nStop";
        let err = parse(src).unwrap_err();
        assert!(err.to_string().contains("global scope"));
    }

    #[test]
    fn uses_with_bm_extension_is_rejected() {
        let src = "<Version: 1.0>\n\nStart\n\nUses: android/build.bm\n\nStop";
        let err = parse(src).unwrap_err();
        assert!(err.to_string().contains("should not include"));
    }
}