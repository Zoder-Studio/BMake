use crate::cache;
use crate::events::{emit, EventSender, OutputStream, TaskEvent};
use crate::graph;
use crate::incremental;
use crate::logstore;
use crate::paths::BMakePaths;
use crate::sandbox;
use crate::status::BuildStatus;
use anyhow::{bail, Result};
use bmake_ast::{BMakeFile, LogLevel, OnError, Task};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command as Proc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
enum TaskResult {
    Success,
    Failed,
    Timeout,
    Skipped,
}

#[allow(clippy::too_many_arguments)]
pub fn run_all_tasks(
    file: &BMakeFile,
    project_dir: &Path,
    paths: &BMakePaths,
    force: bool,
    build_id: &str,
    events: &EventSender,
    secrets_to_mask: &HashSet<String>,
) -> Result<BuildStatus> {
    let waves = graph::topological_waves(&file.tasks)?;
    let name_to_idx: HashMap<&str, usize> = file.tasks.iter().enumerate().map(|(i, t)| (t.name.as_str(), i)).collect();
    let results: Arc<Mutex<HashMap<usize, TaskResult>>> = Arc::new(Mutex::new(HashMap::new()));

    for wave in waves {
        let mut handles = Vec::new();

        for idx in wave {
            let task = file.tasks[idx].clone();

            let deps_ok = task.depends_on.iter().all(|d| {
                let dep_idx = name_to_idx[d.as_str()];
                match results.lock().unwrap().get(&dep_idx).copied() {
                    Some(TaskResult::Success) => true,
                    // Task-level ContinueOnError overrides global StopOnError
                    // for whether Tasks depending on THIS failed/timed-out
                    // Task still run.
                    Some(TaskResult::Failed) | Some(TaskResult::Timeout) => {
                        file.tasks[dep_idx].continue_on_error.unwrap_or(!file.stop_on_error)
                    }
                    _ => false,
                }
            });

            if !deps_ok {
                emit(events, TaskEvent::TaskSkipped { task: task.name.clone(), reason: "a dependency failed".to_string() });
                results.lock().unwrap().insert(idx, TaskResult::Skipped);
                continue;
            }

            if let Some(cond) = &task.condition {
                if !evaluate_condition(cond, file) {
                    emit(
                        events,
                        TaskEvent::TaskSkipped { task: task.name.clone(), reason: format!("Condition: {} is false", cond) },
                    );
                    results.lock().unwrap().insert(idx, TaskResult::Skipped);
                    continue;
                }
            }

            if !force && incremental::is_up_to_date(paths, project_dir, &task) {
                incremental::record_hit(paths);
                emit(
                    events,
                    TaskEvent::TaskSkipped {
                        task: task.name.clone(),
                        reason: "up to date (Input unchanged, Output present)".to_string(),
                    },
                );
                results.lock().unwrap().insert(idx, TaskResult::Success);
                continue;
            }

            let file_owned = file.clone();
            let project_dir_owned = project_dir.to_path_buf();
            let paths_owned = BMakePaths { root: paths.root.clone() };
            let build_id_owned = build_id.to_string();
            let events_owned = events.clone();
            let secrets_owned = secrets_to_mask.clone();
            let results_ref = Arc::clone(&results);

            let run_one = move || {
                let status = run_task(&file_owned, &task, &project_dir_owned, &paths_owned, &build_id_owned, &events_owned, &secrets_owned)
                    .unwrap_or(BuildStatus::Failed);
                let r = match status {
                    BuildStatus::Success => TaskResult::Success,
                    BuildStatus::Timeout => TaskResult::Timeout,
                    _ => TaskResult::Failed,
                };
                results_ref.lock().unwrap().insert(idx, r);
            };

            if file.parallel {
                handles.push(thread::spawn(run_one));
            } else {
                run_one();
            }
        }

        for h in handles {
            let _ = h.join();
        }
    }

    let final_results = results.lock().unwrap();
    let mut any_failed = false;
    let mut summary = Vec::new();

    for task in &file.tasks {
        let idx = name_to_idx[task.name.as_str()];
        let label = match final_results.get(&idx) {
            Some(TaskResult::Success) => "SUCCESS",
            Some(TaskResult::Failed) => {
                any_failed = true;
                "FAILED"
            }
            Some(TaskResult::Timeout) => {
                any_failed = true;
                "TIMEOUT"
            }
            Some(TaskResult::Skipped) => "SKIPPED",
            None => "NOT RUN",
        };
        summary.push((task.name.clone(), label.to_string()));
    }
    emit(events, TaskEvent::BuildFinished { results: summary });

    Ok(if any_failed { BuildStatus::Failed } else { BuildStatus::Success })
}

pub fn evaluate_condition(cond: &str, file: &BMakeFile) -> bool {
    let Some((field, value)) = cond.split_once("==") else {
        return true;
    };
    let field = field.trim();
    let value = value.trim();
    let actual = match field {
        "Profile" => file.profile.as_deref(),
        "Platform" => file.platform.as_deref(),
        "Arch" => file.arch.as_deref(),
        "Lang" => file.lang.as_deref(),
        "System" => file.system.as_deref(),
        "Runs-on" => file.runs_on.as_deref(),
        _ => None,
    };
    actual == Some(value)
}

#[allow(clippy::too_many_arguments)]
pub fn run_task(
    file: &BMakeFile,
    task: &Task,
    project_dir: &Path,
    paths: &BMakePaths,
    build_id: &str,
    events: &EventSender,
    secrets_to_mask: &HashSet<String>,
) -> Result<BuildStatus> {
    emit(events, TaskEvent::TaskStarted { task: task.name.clone(), label: task.flow_label.clone() });

    for cmd in &task.before {
        run_shell(cmd, file, task, project_dir, None, paths, build_id, events, secrets_to_mask)?;
    }

    for step in &task.commands {
        let effective_timeout = step.timeout.or(task.timeout);
        let max_attempts = if step.on_error == Some(OnError::Retry) {
            step.retry.unwrap_or(1).max(1)
        } else {
            1
        };

        let mut last_err: Option<anyhow::Error> = None;
        let mut succeeded = false;

        for attempt in 1..=max_attempts {
            if attempt == 1 {
                emit(events, TaskEvent::CommandStarted { task: task.name.clone(), command: step.command.clone() });
            }
            match run_shell(&step.command, file, task, project_dir, effective_timeout, paths, build_id, events, secrets_to_mask) {
                Ok(_) => {
                    succeeded = true;
                    break;
                }
                Err(e) => {
                    if attempt < max_attempts {
                        emit(
                            events,
                            TaskEvent::CommandRetry {
                                task: task.name.clone(),
                                attempt,
                                max_attempts,
                                error: e.to_string(),
                            },
                        );
                    }
                    last_err = Some(e);
                }
            }
        }

        if !succeeded {
            let error = last_err.unwrap().to_string();
            let is_timeout = error == "TIMEOUT";
            emit(events, TaskEvent::TaskFailed { task: task.name.clone(), error, label: task.flow_label.clone() });
            return Ok(if is_timeout { BuildStatus::Timeout } else { BuildStatus::Failed });
        }
    }

    for (from, to) in &task.renames {
        let from_path = project_dir.join(from);
        let to_path = project_dir.join(to);
        if let Some(parent) = to_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&from_path, &to_path)
            .map_err(|e| anyhow::anyhow!("Rename failed '{}' -> '{}': {}", from, to, e))?;
        emit(events, TaskEvent::TaskInfo { task: task.name.clone(), message: format!("renamed {} -> {}", from, to) });
    }

    for cmd in &task.after {
        run_shell(cmd, file, task, project_dir, None, paths, build_id, events, secrets_to_mask)?;
    }

    for artifact in task.artifacts.iter().chain(file.artifacts.iter()) {
        if let Some(found) = resolve_artifact(project_dir, artifact) {
            emit(events, TaskEvent::TaskInfo { task: task.name.clone(), message: format!("artifact: {}", found) });
        }
    }

    // Only Tasks materialized from `Uses:` enforce strict Output
    // verification — a regular Task's Output: is only used for
    // incremental-build caching and shouldn't suddenly become a hard
    // failure for projects that predate this feature.
    if task.flow_label.is_some() {
        for output in &task.outputs {
            if resolve_artifact(project_dir, output).is_none() {
                let msg = format!(
                    "Plugin Flow completed successfully,\nbut declared output was not produced:\n\n{}",
                    output
                );
                emit(events, TaskEvent::TaskFailed { task: task.name.clone(), error: msg, label: task.flow_label.clone() });
                return Ok(BuildStatus::Failed);
            }
        }
    }

    if !task.inputs.is_empty() && !task.outputs.is_empty() {
        if let Err(e) = incremental::record(paths, project_dir, task) {
            emit(
                events,
                TaskEvent::TaskInfo {
                    task: task.name.clone(),
                    message: format!("warning: failed to record incremental state: {}", e),
                },
            );
        }
    }

    emit(events, TaskEvent::TaskSucceeded { task: task.name.clone(), label: task.flow_label.clone() });
    Ok(BuildStatus::Success)
}

fn resolve_artifact(project_dir: &Path, pattern: &str) -> Option<String> {
    let full = project_dir.join(pattern);
    if full.exists() {
        return Some(full.display().to_string());
    }
    let parent = full.parent()?;
    let name_pattern = full.file_name()?.to_str()?;
    let prefix = name_pattern.strip_suffix('*')?;
    let entries = std::fs::read_dir(parent).ok()?;
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(prefix) {
            return Some(entry.path().display().to_string());
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn run_shell(
    cmd: &str,
    file: &BMakeFile,
    task: &Task,
    project_dir: &Path,
    timeout: Option<u64>,
    paths: &BMakePaths,
    build_id: &str,
    events: &EventSender,
    secrets_to_mask: &HashSet<String>,
) -> Result<()> {
    let workdir_rel = task.workdir.as_ref().or(file.workdir.as_ref());
    let workdir = match workdir_rel {
        Some(d) => {
            let p = Path::new(d);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                project_dir.join(p)
            }
        }
        None => project_dir.to_path_buf(),
    };

    let mut env = task.env.clone();
    if let Ok(sandbox_env) = sandbox::build_sandbox_env(paths) {
        env.insert("PATH".to_string(), sandbox_env.path);
        env.extend(sandbox_env.extra);
    }
    env.extend(cache::cache_env(file, paths));

    let shell = file.shell.as_deref().unwrap_or(default_shell());
    let (program, shell_flag) = shell_command(shell);

    if file.log_level != LogLevel::Normal {
        emit(
            events,
            TaskEvent::TaskInfo {
                task: task.name.clone(),
                message: format!("workdir: {}  shell: {} {}", workdir.display(), program, shell_flag),
            },
        );
    }
    if file.log_level == LogLevel::Debug {
        let mut keys: Vec<&String> = env.keys().collect();
        keys.sort();
        for k in keys {
            let upper = k.to_uppercase();
            let is_secret_key = ["TOKEN", "SECRET", "PASSWORD", "KEY"].iter().any(|s| upper.contains(s));
            let shown = if is_secret_key { "***".to_string() } else { mask_secrets(&env[k], secrets_to_mask) };
            emit(events, TaskEvent::TaskInfo { task: task.name.clone(), message: format!("env {}={}", k, shown) });
        }
    }

    let mut child = Proc::new(program)
        .arg(shell_flag)
        .arg(cmd)
        .current_dir(&workdir)
        .envs(&env)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let out_handle = child.stdout.take().map(|s| {
        let paths = BMakePaths { root: paths.root.clone() };
        let build_id = build_id.to_string();
        let task_name = task.name.clone();
        let events = events.clone();
        let secrets = secrets_to_mask.clone();
        thread::spawn(move || forward(s, &paths, &build_id, &task_name, OutputStream::Stdout, &events, &secrets))
    });
    let err_handle = child.stderr.take().map(|s| {
        let paths = BMakePaths { root: paths.root.clone() };
        let build_id = build_id.to_string();
        let task_name = task.name.clone();
        let events = events.clone();
        let secrets = secrets_to_mask.clone();
        thread::spawn(move || forward(s, &paths, &build_id, &task_name, OutputStream::Stderr, &events, &secrets))
    });

    let status = match timeout {
        Some(secs) => wait_with_timeout(&mut child, Duration::from_secs(secs))?,
        None => child.wait()?,
    };

    if let Some(h) = out_handle {
        let _ = h.join();
    }
    if let Some(h) = err_handle {
        let _ = h.join();
    }

    if !status.success() {
        bail!("Command exited with status {:?}", status.code());
    }
    Ok(())
}

fn mask_secrets(line: &str, secrets: &HashSet<String>) -> String {
    let mut out = line.to_string();
    for s in secrets {
        if !s.is_empty() {
            out = out.replace(s.as_str(), "********");
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn forward(
    reader: impl std::io::Read,
    paths: &BMakePaths,
    build_id: &str,
    task: &str,
    stream: OutputStream,
    events: &EventSender,
    secrets: &HashSet<String>,
) {
    use std::io::{BufRead, BufReader};
    for line in BufReader::new(reader).lines().flatten() {
        let masked = mask_secrets(&line, secrets);
        let _ = logstore::append_line(paths, build_id, &masked);
        emit(events, TaskEvent::CommandOutput { task: task.to_string(), stream, line: masked });
    }
}

fn default_shell() -> &'static str {
    if cfg!(windows) {
        "cmd"
    } else {
        "sh"
    }
}

fn shell_command(shell: &str) -> (&str, &str) {
    match shell {
        "cmd" => ("cmd", "/C"),
        "powershell" => ("powershell", "-Command"),
        other => (other, "-c"),
    }
}

fn wait_with_timeout(child: &mut std::process::Child, timeout: Duration) -> Result<std::process::ExitStatus> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if start.elapsed() >= timeout {
            child.kill()?;
            bail!("TIMEOUT");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_with(profile: Option<&str>) -> BMakeFile {
        BMakeFile { profile: profile.map(|s| s.to_string()), ..Default::default() }
    }

    #[test]
    fn condition_true_when_field_matches() {
        assert!(evaluate_condition("Profile == Release", &file_with(Some("Release"))));
    }

    #[test]
    fn condition_false_when_field_differs() {
        assert!(!evaluate_condition("Profile == Release", &file_with(Some("Debug"))));
    }

    #[test]
    fn mask_secrets_redacts_known_values() {
        let mut secrets = HashSet::new();
        secrets.insert("abc123".to_string());
        assert_eq!(mask_secrets("Deploying with token abc123", &secrets), "Deploying with token ********");
    }

    #[test]
    fn mask_secrets_leaves_unrelated_text_alone() {
        let secrets = HashSet::new();
        assert_eq!(mask_secrets("hello world", &secrets), "hello world");
    }
}