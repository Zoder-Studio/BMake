use anyhow::{bail, Context, Result};
use bmake_ast::*;
use std::path::Path;

pub fn parse(input: &str) -> Result<BMakeFile> {
    let lines = bmake_lexer::to_lines(input);
    let n = lines.len();

    let mut i = skip_blank(&lines, 0);
    if i >= n {
        bail!("Missing '<Version: ...>' tag");
    }
    let version = parse_version_tag(lines[i].trim())
        .ok_or_else(|| anyhow::anyhow!("Expected '<Version: ...>' before Start at line {}", i + 1))?;
    i += 1;

    i = skip_blank(&lines, i);
    if i >= n || lines[i].trim() != "Start" {
        bail!("Expected 'Start' after Version tag at line {}", i + 1);
    }
    i += 1;

    let mut file = BMakeFile {
        version,
        ..Default::default()
    };

    let mut pending_dependency: Option<String> = None;
    let mut pending_tool: Option<String> = None;

    while i < n {
        let raw_line = lines[i].clone();
        let t = raw_line.trim();

        if t.is_empty() {
            i += 1;
            continue;
        }
        if t == "Stop" {
            i += 1;
            break;
        }
        if t.starts_with("<Task:") {
            let (task, next_i) = parse_task(&lines, i, &file.env)?;
            file.tasks.push(task);
            i = next_i;
            continue;
        }
        if let Some(rest) = t.strip_prefix("import") {
            let rest = rest.trim_start();
            if let Some(v) = rest.strip_prefix('=') {
                file.imports.push(v.trim().to_string());
                i += 1;
                continue;
            }
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
                file.dependencies.push(Dependency {
                    name,
                    need: rest.trim().to_string(),
                });
            } else if let Some(name) = pending_tool.take() {
                file.tools.push(ToolReq {
                    name,
                    need: rest.trim().to_string(),
                });
            } else {
                bail!("'Need:' without preceding 'Dependency:' or 'Tool:' at line {}", i + 1);
            }
            i += 1;
            continue;
        }
        if let Some(rest) = t.strip_prefix("Require:") {
            file.requires.push(rest.trim().to_string());
            i += 1;
            continue;
        }

        if let Some((key, val)) = split_kv(t) {
            match key.as_str() {
                "Lang" => file.lang = Some(val),
                "System" => file.system = Some(val),
                "Sub-System" => file.sub_system = Some(val),
                "Platform" => file.platform = Some(val),
                "Arch" => file.arch = Some(val),
                "Shell" => file.shell = Some(val),
                "Runs-on" => file.runs_on = Some(val),
                "version" => file.runs_on_version = Some(val),
                "Remote" => file.remote = Some(val),
                "Workdir" => file.workdir = Some(val),
                "Directory" => file.workdir = Some(val),
                "Source" => file.source = Some(val),
                "Output" => file.output = Some(val),
                "Cache" => file.cache = val.eq_ignore_ascii_case("true"),
                "Parallel" => file.parallel = val.eq_ignore_ascii_case("true"),
                "Profile" => file.profile = Some(val),
                "Plugin" => file.plugins.push(val),
                "Artifact" => file.artifacts.push(val),
                "Clean" => file.clean_paths.push(val),
                "StopOnError" => file.stop_on_error = val.eq_ignore_ascii_case("true"),
                "Log-level" => file.log_level = parse_log_level(&val, i)?,
                "Env" => {
                    let Some((k, v)) = val.split_once('=') else {
                        bail!("Malformed Env at line {}: expected KEY=VALUE", i + 1);
                    };
                    file.env.insert(k.trim().to_string(), v.trim().to_string());
                }
                other => bail!("Unknown directive '{}' at line {}", other, i + 1),
            }
            i += 1;
            continue;
        }

        bail!("Unrecognized syntax at line {}: '{}'", i + 1, t);
    }

    Ok(file)
}

fn skip_blank(lines: &[String], mut i: usize) -> usize {
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }
    i
}

fn parse_log_level(v: &str, line: usize) -> Result<LogLevel> {
    match v {
        "normal" => Ok(LogLevel::Normal),
        "verbose" => Ok(LogLevel::Verbose),
        "debug" => Ok(LogLevel::Debug),
        other => bail!("Unknown Log-level value '{}' at line {}", other, line + 1),
    }
}

fn parse_version_tag(line: &str) -> Option<String> {
    let l = line.trim();
    if l.starts_with('<') && l.ends_with('>') {
        let inner = &l[1..l.len() - 1];
        if let Some(rest) = inner.strip_prefix("Version:") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

fn split_kv(line: &str) -> Option<(String, String)> {
    line.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
}

fn parse_task(lines: &[String], start: usize, global_env: &std::collections::HashMap<String, String>) -> Result<(Task, usize)> {
    let header = lines[start].trim();
    let inner = header.trim_start_matches("<Task:").trim_end_matches('>').trim();
    let mut task = Task {
        name: inner.to_string(),
        env: global_env.clone(),
        ..Default::default()
    };

    let mut i = start + 1;
    let n = lines.len();

    while i < n {
        let t = lines[i].trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        if t == "</Task>" {
            i += 1;
            return Ok((task, i));
        }

        if let Some(v) = strip_field(t, "Before") {
            task.before.push(v);
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "After") {
            task.after.push(v);
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Rename") {
            let Some((from, to)) = v.split_once("->") else {
                bail!("Malformed Rename at line {}: expected 'from -> to'", i + 1);
            };
            task.renames.push((from.trim().to_string(), to.trim().to_string()));
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "OnError") {
            let on_error = match v.as_str() {
                "stop" => OnError::Stop,
                "retry" => OnError::Retry,
                other => bail!("Unknown OnError value '{}' at line {}", other, i + 1),
            };
            if let Some(last) = task.commands.last_mut() {
                last.on_error = Some(on_error);
            }
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Retry") {
            let retry: u32 = v.parse()?;
            if let Some(last) = task.commands.last_mut() {
                last.retry = Some(retry);
            }
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Timeout") {
            let timeout: u64 = v.parse()?;
            if let Some(last) = task.commands.last_mut() {
                last.timeout = Some(timeout);
            } else {
                task.timeout = Some(timeout);
            }
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Depends-on") {
            for dep in v.split(',') {
                let dep = dep.trim();
                if !dep.is_empty() {
                    task.depends_on.push(dep.to_string());
                }
            }
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Input") {
            task.inputs.push(v);
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Output") {
            task.outputs.push(v);
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Artifact") {
            task.artifacts.push(v);
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Condition") {
            task.condition = Some(v);
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Workdir") {
            task.workdir = Some(v);
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Env") {
            let Some((k, val)) = v.split_once('=') else {
                bail!("Malformed Env at line {}: expected KEY=VALUE", i + 1);
            };
            task.env.insert(k.trim().to_string(), val.trim().to_string());
            i += 1;
            continue;
        }
        if let Some(v) = strip_field(t, "Command") {
            if v == "{" {
                let (cmd, next_i) = parse_multiline_command(lines, i + 1)?;
                task.commands.push(CommandStep {
                    command: cmd,
                    ..Default::default()
                });
                i = next_i;
            } else {
                task.commands.push(CommandStep {
                    command: v,
                    ..Default::default()
                });
                i += 1;
            }
            continue;
        }

        bail!("Unrecognized syntax inside Task '{}' at line {}: '{}'", task.name, i + 1, t);
    }

    bail!("Missing closing '</Task>' for task '{}'", task.name)
}

fn strip_field(line: &str, field: &str) -> Option<String> {
    let rest = line.strip_prefix(field)?.trim_start();
    let val = rest.strip_prefix(':')?;
    Some(val.trim().to_string())
}

fn parse_multiline_command(lines: &[String], start: usize) -> Result<(String, usize)> {
    let mut parts = Vec::new();
    let mut i = start;
    let n = lines.len();

    while i < n {
        let t = lines[i].trim();
        if t == "}" {
            return Ok((parts.join(" "), i + 1));
        }
        match t.strip_suffix("+/") {
            Some(stripped) => parts.push(stripped.trim().to_string()),
            None => parts.push(t.to_string()),
        }
        i += 1;
    }

    bail!("Unterminated multiline Command block")
}

pub fn parse_file(path: &Path) -> Result<BMakeFile> {
    let mut stack = Vec::new();
    parse_file_inner(path, &mut stack)
}

fn parse_file_inner(path: &Path, stack: &mut Vec<std::path::PathBuf>) -> Result<BMakeFile> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if stack.contains(&canonical) {
        let mut chain: Vec<String> = stack.iter().map(|p| p.display().to_string()).collect();
        chain.push(canonical.display().to_string());
        bail!("Circular import detected:\n{}", chain.join(" -> "));
    }
    stack.push(canonical);

    let content = std::fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
    let file = parse_content_with_imports(&content, &base_dir, stack)?;

    stack.pop();
    Ok(file)
}

fn parse_content_with_imports(content: &str, base_dir: &Path, stack: &mut Vec<std::path::PathBuf>) -> Result<BMakeFile> {
    let mut file = parse(content)?;
    let imports = std::mem::take(&mut file.imports);

    for imp in &imports {
        let imp_path = base_dir.join(imp);
        let imported =
            parse_file_inner(&imp_path, stack).with_context(|| format!("Failed to import {}", imp_path.display()))?;
        merge_into(&mut file, imported);
    }

    file.imports = imports;
    check_duplicate_tasks(&file)?;
    Ok(file)
}

fn check_duplicate_tasks(file: &BMakeFile) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for t in &file.tasks {
        if !seen.insert(t.name.as_str()) {
            bail!(
                "Duplicate Task '{}' found (likely declared in more than one imported .bm file)",
                t.name
            );
        }
    }
    Ok(())
}

pub fn parse_kts_output(flattened: &str, base_dir: &Path, origin_path: &Path) -> Result<BMakeFile> {
    let mut stack = vec![origin_path.canonicalize().unwrap_or_else(|_| origin_path.to_path_buf())];
    parse_content_with_imports(flattened, base_dir, &mut stack)
}

fn merge_into(target: &mut BMakeFile, other: BMakeFile) {
    if target.lang.is_none() {
        target.lang = other.lang;
    }
    if target.system.is_none() {
        target.system = other.system;
    }
    if target.sub_system.is_none() {
        target.sub_system = other.sub_system;
    }
    if target.platform.is_none() {
        target.platform = other.platform;
    }
    if target.arch.is_none() {
        target.arch = other.arch;
    }
    if target.shell.is_none() {
        target.shell = other.shell;
    }
    if target.runs_on.is_none() {
        target.runs_on = other.runs_on;
    }
    if target.runs_on_version.is_none() {
        target.runs_on_version = other.runs_on_version;
    }
    if target.remote.is_none() {
        target.remote = other.remote;
    }
    if target.workdir.is_none() {
        target.workdir = other.workdir;
    }
    if target.source.is_none() {
        target.source = other.source;
    }
    if target.output.is_none() {
        target.output = other.output;
    }
    if target.profile.is_none() {
        target.profile = other.profile;
    }
    target.cache = target.cache || other.cache;
    target.parallel = target.parallel || other.parallel;
    target.stop_on_error = target.stop_on_error && other.stop_on_error;
    target.dependencies.extend(other.dependencies);
    target.requires.extend(other.requires);
    target.tools.extend(other.tools);
    target.plugins.extend(other.plugins);
    target.artifacts.extend(other.artifacts);
    target.clean_paths.extend(other.clean_paths);
    for (k, v) in other.env {
        target.env.entry(k).or_insert(v);
    }
    target.tasks.extend(other.tasks);
}

pub fn transpile_kts_to_kotlin_script(source: &str) -> String {
    let mut out = String::new();
    for raw_line in source.lines() {
        let trimmed = raw_line.trim();
        if is_kotlin_control_line(trimmed) {
            out.push_str(raw_line);
            out.push('\n');
        } else if trimmed.is_empty() {
            out.push('\n');
        } else {
            out.push_str("println(\"\"\"");
            out.push_str(raw_line);
            out.push_str("\"\"\")\n");
        }
    }
    out
}

fn is_kotlin_control_line(t: &str) -> bool {
    if t.is_empty() || t == "{" || t == "}" || t.starts_with('}') {
        return true;
    }
    if let Some(rest) = t.strip_prefix("import ") {
        return !rest.trim_start().starts_with('=');
    }
    let prefixes = [
        "val ", "var ", "fun ", "class ", "object ", "if (", "if(", "if ", "else", "for (", "for(", "for ",
        "while (", "while(", "while ", "package ",
    ];
    prefixes.iter().any(|p| t.starts_with(p))
}