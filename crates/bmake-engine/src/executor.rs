use crate::cache;
use crate::paths::BMakePaths;
use crate::sandbox;
use crate::status::BuildStatus;
use anyhow::{bail, Result};
use bmake_ast::{BMakeFile, OnError, Task};
use std::path::{Path, PathBuf};
use std::process::Command as Proc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub fn run_all_tasks(file: &BMakeFile, project_dir: &Path, paths: &BMakePaths) -> Result<BuildStatus> {
    if file.parallel && file.tasks.len() > 1 {
        println!(" Parallel = true — running {} tasks concurrently", file.tasks.len());
        run_tasks_parallel(file, project_dir, paths)
    } else {
        run_tasks_sequential(file, project_dir, paths)
    }
}

fn run_tasks_sequential(file: &BMakeFile, project_dir: &Path, paths: &BMakePaths) -> Result<BuildStatus> {
    for task in &file.tasks {
        let status = run_task(file, task, project_dir, paths)?;
        if status != BuildStatus::Success {
            return Ok(status);
        }
    }
    Ok(BuildStatus::Success)
}

/// Runs every task on its own thread. Intended for tasks with no ordering
/// dependency between them — the grammar has no inter-task dependency
/// syntax, so the author of the `.bm` file is responsible for only marking
/// genuinely independent tasks as parallel-safe.
fn run_tasks_parallel(file: &BMakeFile, project_dir: &Path, paths: &BMakePaths) -> Result<BuildStatus> {
    let file = Arc::new(file.clone());
    let project_dir: Arc<PathBuf> = Arc::new(project_dir.to_path_buf());
    let paths = Arc::new(BMakePaths { root: paths.root.clone() });
    let mut handles = Vec::new();

    for task in file.tasks.clone() {
        let file = Arc::clone(&file);
        let project_dir = Arc::clone(&project_dir);
        let paths = Arc::clone(&paths);
        handles.push(thread::spawn(move || run_task(&file, &task, &project_dir, &paths)));
    }

    let mut overall = BuildStatus::Success;
    for handle in handles {
        match handle.join() {
            Ok(Ok(status)) if status != BuildStatus::Success => overall = status,
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                eprintln!(" Task error: {}", e);
                overall = BuildStatus::Failed;
            }
            Err(_) => overall = BuildStatus::Failed,
        }
    }
    Ok(overall)
}

pub fn run_task(file: &BMakeFile, task: &Task, project_dir: &Path, paths: &BMakePaths) -> Result<BuildStatus> {
    println!("\n Task: {}", task.name);

    for cmd in &task.before {
        run_shell(cmd, file, project_dir, None, paths)?;
    }

    for step in &task.commands {
        let max_attempts = if step.on_error == Some(OnError::Retry) {
            step.retry.unwrap_or(1).max(1)
        } else {
            1
        };

        let mut last_err: Option<anyhow::Error> = None;
        let mut succeeded = false;

        for attempt in 1..=max_attempts {
            println!(" $ {}", step.command);
            match run_shell(&step.command, file, project_dir, step.timeout, paths) {
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
        run_shell(cmd, file, project_dir, None, paths)?;
    }

    println!(" Task '{}' finished", task.name);
    Ok(BuildStatus::Success)
}

fn run_shell(cmd: &str, file: &BMakeFile, project_dir: &Path, timeout: Option<u64>, paths: &BMakePaths) -> Result<()> {
    let workdir = match &file.directory {
        Some(d) => project_dir.join(d),
        None => project_dir.to_path_buf(),
    };

    let mut env = file.env.clone();
    if let Ok(sandbox_env) = sandbox::build_sandbox_env(paths) {
        env.insert("PATH".to_string(), sandbox_env.path);
        env.extend(sandbox_env.extra);
    }
    env.extend(cache::cache_env(file, paths));

    #[cfg(unix)]
    let mut child = Proc::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(&workdir)
        .envs(&env)
        .spawn()?;

    #[cfg(windows)]
    let mut child = Proc::new("cmd")
        .arg("/C")
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