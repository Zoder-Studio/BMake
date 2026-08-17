use bmake_engine::events::{OutputStream, TaskEvent};
use crossterm::style::{Color, Stylize};
use std::io::{stdout, IsTerminal, Write};
use std::sync::mpsc::Receiver;

#[derive(Debug, Clone, Copy)]
pub struct UiOptions {
    pub color: bool,
    pub animate: bool,
}

impl UiOptions {
    pub fn detect(no_color_flag: bool, no_animation_flag: bool) -> Self {
        let is_tty = stdout().is_terminal();
        let no_color_env = std::env::var_os("NO_COLOR").is_some();
        let is_ci = detect_ci();

        Self {
            color: is_tty && !no_color_flag && !no_color_env,
            animate: is_tty && !no_animation_flag && !is_ci,
        }
    }
}

fn detect_ci() -> bool {
    if std::env::var_os("CI").is_some() {
        return true;
    }
    ["GITHUB_ACTIONS", "GITLAB_CI", "BUILDKITE", "CIRCLECI", "TRAVIS", "TEAMCITY_VERSION"]
        .iter()
        .any(|k| std::env::var_os(k).is_some())
}

/// Consumes Task Events and writes the terminal-facing progress. This is
/// the only place in the CLI that prints build progress — the execution
/// engine only emits events, never touches stdout directly.
pub fn render_loop(rx: Receiver<TaskEvent>, opts: &UiOptions) {
    let mut spinner_frame: usize = 0;
    const SPINNER: [&str; 4] = ["-", "\\", "|", "/"];

    for event in rx {
        match event {
            TaskEvent::TaskStarted { task } => {
                println!("{}", styled(opts, &format!("▶ {}", task), Color::Cyan));
            }
            TaskEvent::TaskSkipped { task, reason } => {
                println!("{}", styled(opts, &format!("○ {} — skipped ({})", task, reason), Color::DarkGrey));
            }
            TaskEvent::CommandStarted { task, command } => {
                let marker = if opts.animate {
                    let frame = SPINNER[spinner_frame % SPINNER.len()];
                    spinner_frame += 1;
                    frame
                } else {
                    "$"
                };
                println!("  {} [{}] {}", marker, task, command);
            }
            TaskEvent::CommandOutput { task, stream, line } => {
                let color = if stream == OutputStream::Stderr { Color::Yellow } else { Color::Reset };
                println!("  {}", styled(opts, &format!("[{}] {}", task, line), color));
            }
            TaskEvent::CommandRetry { task, attempt, max_attempts, error } => {
                println!(
                    "{}",
                    styled(opts, &format!("↻ [{}] retry {}/{}: {}", task, attempt, max_attempts, error), Color::Yellow)
                );
            }
            TaskEvent::TaskInfo { task, message } => {
                println!("  [{}] {}", task, message);
            }
            TaskEvent::TaskSucceeded { task } => {
                println!("{}", styled(opts, &format!("✓ {} finished", task), Color::Green));
            }
            TaskEvent::TaskFailed { task, error } => {
                println!("{}", styled(opts, &format!("✗ {} failed: {}", task, error), Color::Red));
            }
            TaskEvent::BuildFinished { results } => {
                println!("\nBuild summary:");
                for (name, status) in &results {
                    let color = match status.as_str() {
                        "SUCCESS" => Color::Green,
                        "FAILED" => Color::Red,
                        "SKIPPED" => Color::DarkGrey,
                        _ => Color::Reset,
                    };
                    println!("  {}", styled(opts, &format!("{} : {}", name, status), color));
                }
            }
        }
        let _ = stdout().flush();
    }
}

fn styled(opts: &UiOptions, text: &str, color: Color) -> String {
    if !opts.color || color == Color::Reset {
        return text.to_string();
    }
    text.with(color).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // std::env::set_var/remove_var mutate global process state, so these
    // tests serialize via a lock instead of running in parallel threads.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn ci_detection_respects_ci_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("CI", "true");
        assert!(detect_ci());
        std::env::remove_var("CI");
    }

    #[test]
    fn ci_detection_respects_provider_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("CI");
        std::env::set_var("GITHUB_ACTIONS", "true");
        assert!(detect_ci());
        std::env::remove_var("GITHUB_ACTIONS");
    }

    #[test]
    fn no_ci_env_means_no_ci() {
        let _guard = ENV_LOCK.lock().unwrap();
        for k in ["CI", "GITHUB_ACTIONS", "GITLAB_CI", "BUILDKITE", "CIRCLECI", "TRAVIS", "TEAMCITY_VERSION"] {
            std::env::remove_var(k);
        }
        assert!(!detect_ci());
    }

    #[test]
    fn plain_line_has_no_ansi_when_color_disabled() {
        let opts = UiOptions { color: false, animate: false };
        let line = styled(&opts, "hello", Color::Green);
        assert_eq!(line, "hello");
        assert!(!line.contains('\u{1b}'));
    }

    #[test]
    fn colored_line_contains_ansi_when_color_enabled() {
        let opts = UiOptions { color: true, animate: true };
        let line = styled(&opts, "hello", Color::Green);
        assert!(line.contains('\u{1b}'));
    }
}