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
}

pub type EventSender = Sender<TaskEvent>;

/// Sends an event, silently ignoring a disconnected receiver — the UI
/// thread exiting early should never crash the build itself.
pub fn emit(tx: &EventSender, event: TaskEvent) {
    let _ = tx.send(event);
}