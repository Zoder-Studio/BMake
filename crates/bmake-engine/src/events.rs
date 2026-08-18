use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone)]
pub enum TaskEvent {
    TaskStarted { task: String },
    TaskSkipped { task: String, reason: String },
    CommandStarted { task: String, command: String },
    CommandOutput { task: String, stream: OutputStream, line: String },
    CommandRetry { task: String, attempt: u32, max_attempts: u32, error: String },
    TaskInfo { task: String, message: String },
    TaskSucceeded { task: String },
    TaskFailed { task: String, error: String },
    BuildFinished { results: Vec<(String, String)> },
    /// `source` is always a safe label ("secret.bm.locksys", "BMake Secret
    /// Store", "CI secret store") — never a value. `error` on
    /// SecretFailed is likewise constructed only from names/reasons
    /// throughout this codebase, never from the secret's plaintext.
    SecretResolving { name: String, source: String },
    SecretResolved { name: String },
    SecretFailed { name: String, error: String },
}

pub type EventSender = Sender<TaskEvent>;

/// Sends an event, silently ignoring a disconnected receiver — the UI
/// thread exiting early should never crash the build itself.
pub fn emit(tx: &EventSender, event: TaskEvent) {
    let _ = tx.send(event);
}