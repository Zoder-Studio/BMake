use crate::status::BuildStatus;
use anyhow::{bail, Result};
use bmake_ast::{BMakeFile, OnError, Task};
use std::path::Path;
use std::process::Command as Proc;
use std::time::{Duration, Instant};

pub fn run_all_tasks(file: &BMakeFile, project_dir: &Path) -> Result<BuildStatus> {
    for task in &file.tasks {
        let status = run_task(file, task, project_dir)?;
        if status != BuildStatus::Success {
            return Ok(status);
        }
    }
    Ok(BuildStatus::Success)
}

pub fn run_task(file: &BMakeFile, task: &Task, project_dir: &Path) -> Result<BuildStatus> {
    println!("\n Task: {}", task.name);

    for cmd in &task.before {
        run_shell(cmd, file, project_dir, None)?;
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
            match run_shell(&step.command, file, project_dir, step.timeout) {
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
            .map_err(|e| anyhow::anyhow!("Failed to Rename '{}' -> '{}': {}", from, to, e))?;
        println!(" Renamed {} -> {}", from, to);
    }

    for cmd in &task.after {
        run_shell(cmd, file, project_dir, None)?;
    }

    println!(" Task '{}' Finished", task.name);
    Ok(BuildStatus::Success)
}

fn run_shell(cmd: &str, file: &BMakeFile, project_dir: &Path, timeout: Option<u64>) -> Result<()> {
    let workdir = match &file.directory {
        Some(d) => project_dir.join(d),
        None => project_dir.to_path_buf(),
    };

    #[cfg(unix)]
    let mut child = Proc::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(&workdir)
        .envs(&file.env)
        .spawn()?;

    #[cfg(windows)]
    let mut child = Proc::new("cmd")
        .arg("/C")
        .arg(cmd)
        .current_dir(&workdir)
        .envs(&file.env)
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