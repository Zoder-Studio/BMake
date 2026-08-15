use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum OnError {
    Stop,
    Retry,
}

#[derive(Debug, Clone, Default)]
pub struct CommandStep {
    pub command: String,
    pub on_error: Option<OnError>,
    pub retry: Option<u32>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct Task {
    pub name: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub commands: Vec<CommandStep>,
    pub renames: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct Dependency {
    pub name: String,
    pub need: String,
}

#[derive(Debug, Clone, Default)]
pub struct BMakeFile {
    pub version: String,
    pub lang: Option<String>,
    pub system: Option<String>,
    pub sub_system: Option<String>,
    pub platform: Option<String>,
    pub arch: Option<String>,
    pub directory: Option<String>,
    pub source: Option<String>,
    pub output: Option<String>,
    pub dependencies: Vec<Dependency>,
    pub requires: Vec<String>,
    pub cache: bool,
    pub parallel: bool,
    pub profile: Option<String>,
    pub env: HashMap<String, String>,
    pub includes: Vec<String>,
    pub plugins: Vec<String>,
    pub tasks: Vec<Task>,
}