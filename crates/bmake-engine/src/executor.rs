use crate::cache;
use crate::graph;
use crate::incremental;
use crate::paths::BMakePaths;
use crate::sandbox;
use crate::status::BuildStatus;
use anyhow::{bail, Result};
use bmake_ast::{BMakeFile, LogLevel, OnError, Task};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command as Proc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
enum TaskResult {
    Success,
    Failed,
    Skipped,
}

pub fn run_all_tasks(file: &BMakeFile, project_dir: &Path, paths: &BMakePaths, force: bool) -> Result<BuildStatus> {
    let waves = graph::topological_waves(&file.tasks)?;
    let name_to_idx: HashMap<&str, usize> = file.tasks.iter().enumerate().map(|(i, t)| (t.name.as_str(), i)).collect();
    let results: Arc<Mutex<HashMap<usize, TaskResult>>> = Arc::new(Mutex::new(HashMap::new()));

    for wave in waves {
        let mut handles = Vec::new();

        for idx in wave {
            let task = file.tasks[idx].clone();

            let deps_ok = task.depends_on.iter().all(|d| {
                let dep_idx = name_to_idx[d.as_str()];
                matches!(results.lock().unwrap().get(&dep_idx), Some(TaskResult::Success))
            });

            if !deps_ok && file.stop_on_error {
                println!("\n Task: {} — skipped (a dependency failed)", task.name);
                results.lock().unwrap().insert(idx, TaskResult::Skipped);
                continue;
            }

            if let Some(cond) = &task.condition {
                if !evaluate_condition(cond, file) {
                    println!("\n Task: {} — skipped (Condition: {} is false)", task.name, cond);
                    results.lock().unwrap().insert(idx, TaskResult::Skipped);
                    continue;
                }
            }

            if !force && incremental::is_up_to_date(paths, project_dir, &task) {
                println!("\n Task: {} — up to date (Input unchanged, Output present), skipping", task.name);
                results.lock().unwrap().insert(idx, TaskResult::Success);
                continue;
            }

            let file_owned = file.clone();
            let project_dir_owned = project_dir.to_path_buf();
            let paths_owned = BMakePaths { root: paths.root.clone() };
            let results_ref = Arc::clone(&results);

            let run_one = move || {
                let status = run_task(&file_owned, &task, &project_dir_owned, &paths_owned).unwrap_or(BuildStatus::Failed);
                let r = if status == BuildStatus::Success { TaskResult::Success } else { TaskResult::Failed };
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

    println!("\n Build summary:");
    for task in &file.tasks {
        let idx = name_to_idx[task.name.as_str()];
        let label = match final_results.get(&idx) {
            Some(TaskResult::Success) => "SUCCESS",
            Some(TaskResult::Failed) => {
                any_failed = true;
                "FAILED"
            }
            Some(TaskResult::Skipped) => "SKIPPED",
            None => "NOT RUN",
        };
        println!("   {} : {}", task.name, label);
    }

    Ok(if any_failed { BuildStatus::Failed } else { BuildStatus::Success })
}

fn evaluate_condition(cond: &str, file: &BMakeFile) -> bool {
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

pub fn run_task(file: &BMakeFile, task: &Task, project_dir: &Path, paths: &BMakePaths) -> Result<BuildStatus> {
    println!("\n Task: {}", task.name);

    for cmd in &task.before {
        run_shell(cmd, file, task, project_dir, None, paths)?;
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
            println!(" $ {}", step.command);
            match run_shell(&step.command, file, task, project_dir, effective_timeout, paths) {
                Ok(_) => {
                    succeeded = true;
                    break;
                }
                Err(e) => {
                    if attempt < max_attempts {
                        println!(" Retry {}/{}: {}", attempt, max_attempts, e);
                    }
                    last_err = Some(e);
                }
            }
        }

        if !succeeded {
            println!(" Command failed: {}", last_err.unwrap());
            return Ok(BuildStatus::Failed);
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
        println!(" Renamed {} -> {}", from, to);
    }

    for cmd in &task.after {
        run_shell(cmd, file, task, project_dir, None, paths)?;
    }

    for artifact in task.artifacts.iter().chain(file.artifacts.iter()) {
        report_artifact(project_dir, artifact);
    }

    if !task.inputs.is_empty() && !task.outputs.is_empty() {
        if let Err(e) = incremental::record(paths, project_dir, task) {
            println!(" Warning: failed to record incremental state for '{}': {}", task.name, e);
        }
    }

    println!(" Task '{}' finished", task.name);
    Ok(BuildStatus::Success)
}

fn report_artifact(project_dir: &Path, pattern: &str) {
    let full = project_dir.join(pattern);
    if full.exists() {
        println!(" Artifact: {}", full.display());
        return;
    }
    if let Some(parent) = full.parent() {
        if let (Some(name_pattern), Ok(entries)) = (full.file_name().and_then(|s| s.to_str()), std::fs::read_dir(parent)) {
            if let Some(prefix) = name_pattern.strip_suffix('*') {
                for entry in entries.flatten() {
                    if entry.file_name().to_string_lossy().starts_with(prefix) {
                        println!(" Artifact: {}", entry.path().display());
                    }
                }
            }
        }
    }
}

fn run_shell(cmd: &str, file: &BMakeFile, task: &Task, project_dir: &Path, timeout: Option<u64>, paths: &BMakePaths) -> Result<()> {
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
        println!("   workdir: {}", workdir.display());
        println!("   shell: {} {}", program, shell_flag);
    }
    if file.log_level == LogLevel::Debug {
        let mut keys: Vec<&String> = env.keys().collect();
        keys.sort();
        for k in keys {
            let upper = k.to_uppercase();
            let is_secret = ["TOKEN", "SECRET", "PASSWORD", "KEY"].iter().any(|s| upper.contains(s));
            let shown = if is_secret { "***".to_string() } else { env[k].clone() };
            println!("   env {}={}", k, shown);
        }
    }

    let mut child = Proc::new(program)
        .arg(shell_flag)
        .arg(cmd)
        .current_dir(&workdir)
        .envs(&env)
        .spawn()?;

    let status = match timeout {
        Some(secs) => wait_with_timeout(&mut child, Duration::from_secs(secs))?,
        None => child.wait()?,
    };

    if !status.success() {
        bail!("Command exited with status {:?}", status.code());
    }
    Ok(())
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