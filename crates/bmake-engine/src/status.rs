#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildStatus {
    Pending,
    Running,
    Success,
    Failed,
    Cancelled,
    Timeout,
}

impl BuildStatus {
    pub fn exit_code(self) -> i32 {
        match self {
            BuildStatus::Success => 0,
            _ => 1,
        }
    }
}