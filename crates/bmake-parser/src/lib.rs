use std::path::Path;
use anyhow::{bail, Result, Context};
use bmake_ast::*;

pub fn parse(input: &str) -> Result<BMakeFile> {
    let lines = bmake_lexer::to_lines(input);
    let n = lines.len();
    let mut i = 0usize;

    let mut version: Option<String> = None;
    while i < n {
        let t = lines[i].trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        version = parse_version_tag(t);
        if version.is_none() {
            bail!("Expected '<Version: ...>' before Start at line {}", i + 1);
        }
        i += 1;
        break;
    }
    let version = version.ok_or_else(|| anyhow::anyhow!("Missing '<Version: ...>' tag"))?;

    let mut found_start = false;
    while i < n {
        let t = lines[i].trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        if t == "Start" {
            found_start = true;
            i += 1;
        }
        break;
    }
    if !found_start {
        bail!("Missing 'Start' marker after Version tag");
    }

    let mut file = BMakeFile {
        version,
        ..Default::default()
    };
    let mut pending_dependency: Option<String> = None;

    while i < n {
        let t = lines[i].trim();

        if t.is_empty() {
            i += 1;
            continue;
        }
        if t == "Stop" {
            i += 1;
            break;
        }
        if t.starts_with("<Task:") {
            let (task, next_i) = parse_task(&lines, i)?;
            file.tasks.push(task);
            i = next_i;
            continue;
        }
        if let Some(rest) = t.strip_prefix("Dependency:") {
            pending_dependency = Some(rest.trim().to_string());
            i += 1;
            continue;
        }
        if let Some(rest) = t.strip_prefix("Need:") {
            let name = pending_dependency
                .take()
                .ok_or_else(|| anyhow::anyhow!("'Need' without preceding 'Dependency:' at line {}", i + 1))?;
            file.dependencies.push(Dependency {
                name,
                need: rest.trim().to_string(),
            });
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
                "Directory" => file.directory = Some(val),
                "Source" => file.source = Some(val),
                "Output" => file.output = Some(val),
                "Cache" => file.cache = val.eq_ignore_ascii_case("true"),
                "Parallel" => file.parallel = val.eq_ignore_ascii_case("true"),
                "Profile" => file.profile = Some(val),
                "Include" => file.includes.push(val),
                "Plugin" => file.plugins.push(val),
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

pub fn parse_file(path: &Path) -> Result<BMakeFile> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let mut file = parse(&content).with_context(|| format!("Failed to parse {}", path.display()))?;

    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let includes = std::mem::take(&mut file.includes);

    for inc in &includes {
        let inc_path = base_dir.join(inc);
        let included = parse_file(&inc_path).with_context(|| format!("Failed to include {}", inc_path.display()))?;
        merge_into(&mut file, included);
    }

    file.includes = includes;
    Ok(file)
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
    line.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
}

fn parse_task(lines: &[String], start: usize) -> Result<(Task, usize)> {
    let header = lines[start].trim();
    let inner = header.trim_start_matches("<Task:").trim_end_matches('>').trim();
    let mut task = Task {
        name: inner.to_string(),
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
            }
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
    let val = rest.strip_prefix('=')?;
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
    if target.directory.is_none() {
        target.directory = other.directory;
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
    target.dependencies.extend(other.dependencies);
    target.requires.extend(other.requires);
    target.plugins.extend(other.plugins);
    for (k, v) in other.env {
        target.env.entry(k).or_insert(v);
    }
    target.tasks.extend(other.tasks);
}