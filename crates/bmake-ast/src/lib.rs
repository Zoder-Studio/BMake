use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum OnError {
    Stop,
    Retry,
}

#[derive(Debug, Clone)]
pub enum ValueNode {
    Scalar(String),
    Map(std::collections::BTreeMap<String, ValueNode>),
}

#[derive(Debug, Clone, Default)]
pub struct CommandStep {
    pub command: String,
    pub on_error: Option<OnError>,
    pub retry: Option<u32>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolReq {
    pub name: String,
    pub need: String,
}

#[derive(Debug, Clone, Default)]
pub struct Dependency {
    pub name: String,
    pub need: String,
}

#[derive(Debug, Clone, Default)]
pub struct Task {
    pub name: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub commands: Vec<CommandStep>,
    pub renames: Vec<(String, String)>,
    pub depends_on: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub artifacts: Vec<String>,
    pub condition: Option<String>,
    pub workdir: Option<String>,
    pub env: HashMap<String, String>,
    pub timeout: Option<u64>,
    pub continue_on_error: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogLevel {
    Normal,
    Verbose,
    Debug,
}

impl Default for LogLevel {
    fn default() -> Self {
        LogLevel::Normal
    }
}

#[derive(Debug, Clone)]
pub struct BMakeFile {
    pub version: String,
    pub lang: Option<String>,
    pub system: Option<String>,
    pub sub_system: Option<String>,
    pub platform: Option<String>,
    pub arch: Option<String>,
    pub shell: Option<String>,
    pub runs_on: Option<String>,
    pub runs_on_version: Option<String>,
    pub remote: Option<String>,
    pub workdir: Option<String>,
    pub source: Option<String>,
    pub output: Option<String>,
    pub dependencies: Vec<Dependency>,
    pub requires: Vec<String>,
    pub tools: Vec<ToolReq>,
    pub cache: bool,
    pub parallel: bool,
    pub profile: Option<String>,
    pub env: HashMap<String, String>,
    pub imports: Vec<String>,
    pub plugins: Vec<String>,
    pub artifacts: Vec<String>,
    pub clean_paths: Vec<String>,
    pub stop_on_error: bool,
    pub log_level: LogLevel,
    pub tasks: Vec<Task>,
    pub values: std::collections::BTreeMap<String, ValueNode>,
    pub secrets: Vec<String>,
}

impl Default for BMakeFile {
    fn default() -> Self {
        Self {
            version: String::new(),
            lang: None,
            system: None,
            sub_system: None,
            platform: None,
            arch: None,
            shell: None,
            runs_on: None,
            runs_on_version: None,
            remote: None,
            workdir: None,
            source: None,
            output: None,
            dependencies: Vec::new(),
            requires: Vec::new(),
            tools: Vec::new(),
            cache: false,
            parallel: false,
            profile: None,
            env: HashMap::new(),
            imports: Vec::new(),
            plugins: Vec::new(),
            artifacts: Vec::new(),
            clean_paths: Vec::new(),
            stop_on_error: true,
            log_level: LogLevel::Normal,
            tasks: Vec::new(),
            values: std::collections::BTreeMap::new(),
            secrets: Vec::new(),
        }
    }
}