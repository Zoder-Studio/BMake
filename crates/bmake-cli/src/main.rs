use anyhow::Result;
use bmake_engine::{dependency, executor, paths::BMakePaths, plugin, status::BuildStatus, version};
use clap::{Parser, Subcommand};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "bmake", version, about = "BMake — universal build orchestration system")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the build defined in a .bm file
    Run {
        file: Option<PathBuf>,
        #[arg(long)]
        verbose: bool,
        #[arg(long)]
        debug: bool,
        /// Ignore incremental build state and force every task to run
        #[arg(long)]
        force: bool,
    },
    /// Create a new BMake.bm in the current directory
    Init {
        #[arg(long)]
        kts: bool,
    },
    /// Validate a .bm file without running it
    Check { file: Option<PathBuf> },
    /// Remove the .bmake/ cache directory
    Clean {
        #[arg(long)]
        deep: bool,
    },
    /// Show migration notes between BMake versions
    Migrate,
    /// Authenticate with the BMake website
    Login,
    /// Remove stored BMake credentials
    Logout,
    /// Show the current BMake Engine version
    Version,
    /// Manage BMake Runners
    Runner {
        #[command(subcommand)]
        action: RunnerAction,
    },
}

#[derive(Subcommand)]
enum RunnerAction {
    /// Register a new Runner in the local registry
    Register,
    /// Mark a Runner as ONLINE
    Start { id: String },
    /// List registered Runners and their status
    Status,
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Run { file, verbose, debug, force } => cmd_run(file, verbose, debug, force),
        Commands::Init { kts } => cmd_init(kts),
        Commands::Check { file } => cmd_check(file),
        Commands::Clean { deep } => cmd_clean(deep),
        Commands::Migrate => cmd_migrate(),
        Commands::Login => cmd_login(),
        Commands::Logout => cmd_logout(),
        Commands::Version => cmd_version(),
        Commands::Runner { action } => match action {
            RunnerAction::Register => cmd_runner_register(),
            RunnerAction::Start { id } => cmd_runner_start(&id),
            RunnerAction::Status => cmd_runner_status(),
        },
    };

    if let Err(e) = result {
        eprintln!(" BMake Error:\n{:#}", e);
        std::process::exit(1);
    }
}

fn find_bm_file(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(f) = explicit {
        return Ok(f);
    }
    let cwd = std::env::current_dir()?;

    let default = cwd.join("BMake.bm");
    if default.exists() {
        return Ok(default);
    }
    let default_kts = cwd.join("BMake.bm.kts");
    if default_kts.exists() {
        return Ok(default_kts);
    }

    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&cwd)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_bm_file(p))
        .collect();

    match candidates.len() {
        0 => anyhow::bail!(
            "No '.bm' or '.bm.kts' file found in this directory. Run 'bmake init' to create one, or pass a path: bmake run <file>.bm"
        ),
        1 => Ok(candidates.remove(0)),
        _ => anyhow::bail!("Multiple BMake files found. Run: bmake run <file>.bm"),
    }
}

fn is_bm_file(p: &Path) -> bool {
    let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
    name.ends_with(".bm") || name.ends_with(".bm.kts")
}

fn parse_bm_or_kts(path: &Path) -> Result<bmake_ast::BMakeFile> {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if name.ends_with(".bm.kts") {
        bmake_engine::kts::parse_kts_file(path)
    } else {
        bmake_parser::parse_file(path)
    }
}

fn cmd_run(file: Option<PathBuf>, verbose: bool, debug: bool, force: bool) -> Result<()> {
    let bm_path = find_bm_file(file)?;
    let project_dir = bm_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut bmake_file = parse_bm_or_kts(&bm_path)?;

    if debug {
        bmake_file.log_level = bmake_ast::LogLevel::Debug;
    } else if verbose {
        bmake_file.log_level = bmake_ast::LogLevel::Verbose;
    }

    println!(" BMake {} — {}", bmake_file.version, bm_path.display());

    let paths = BMakePaths::new(&project_dir);
    paths.ensure_all()?;

    if bmake_file.version != version::CURRENT_ENGINE_VERSION {
        version::ensure_engine(&paths, &bmake_file.version)?;
        println!(" (Requested engine version differs from the running CLI — execution still uses this CLI's logic for now)");
    }

    if let Some(remote) = bmake_file.remote.clone() {
        handle_remote(&remote, &bmake_file)?;
    }

    dependency::ensure_requires(&bmake_file.requires)?;
    dependency::ensure_dependencies(&bmake_file.dependencies)?;
    dependency::ensure_tools(&bmake_file.tools, &paths)?;

    write_lockfile(&bmake_file, &paths)?;

    let start = bmake_engine::metadata::now_unix();
    let build_id = bmake_engine::metadata::new_build_id();

    let status = executor::run_all_tasks(&bmake_file, &project_dir, &paths, force)?;

    if status == BuildStatus::Success {
        plugin::run_after_build(&bmake_file, &project_dir)?;
    }

    let end = bmake_engine::metadata::now_unix();
    let meta = bmake_engine::metadata::BuildMetadata {
        bmake_version: version::CURRENT_ENGINE_VERSION.to_string(),
        build_id,
        status: format!("{:?}", status),
        start_time: start,
        end_time: end,
        duration_secs: end.saturating_sub(start),
        runs_on: bmake_file.runs_on.clone(),
        runs_on_version: bmake_file.runs_on_version.clone(),
        arch: bmake_file.arch.clone(),
        platform: bmake_file.platform.clone(),
        system: bmake_file.system.clone(),
        sub_system: bmake_file.sub_system.clone(),
        remote: bmake_file.remote.clone(),
        tasks: bmake_file
            .tasks
            .iter()
            .map(|t| bmake_engine::metadata::TaskMeta {
                name: t.name.clone(),
                status: "see summary above".to_string(),
            })
            .collect(),
        exit_code: status.exit_code(),
    };
    bmake_engine::metadata::write(&paths, &meta)?;

    match status {
        BuildStatus::Success => println!("\n BUILD SUCCESS"),
        BuildStatus::Failed => println!("\n BUILD FAILED"),
        other => println!("\n BUILD {:?}", other),
    }

    std::process::exit(status.exit_code());
}

fn handle_remote(remote: &str, bmake_file: &bmake_ast::BMakeFile) -> Result<()> {
    match remote {
        "Local" => {
            let runs_on = bmake_file
                .runs_on
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Remote: Local requires 'Runs-on:' to be set"))?;
            let matched = bmake_engine::runner::find_match(
                &runs_on,
                bmake_file.runs_on_version.as_deref(),
                bmake_file.arch.as_deref(),
            )?;
            match matched {
                Some(r) => println!(" Matched runner '{}' ({}) — executing here (no network dispatch yet)", r.name, r.id),
                None => anyhow::bail!(
                    "No online Runner found matching Runs-on: {} version: {:?} Arch: {:?}. Register one with 'bmake runner register' and bring it online with 'bmake runner start <id>'.",
                    runs_on,
                    bmake_file.runs_on_version,
                    bmake_file.arch
                ),
            }
        }
        "CI" => println!(" Remote: CI requested — the BMake control plane/website is not available yet; running locally as a fallback."),
        "Auto" => {}
        other => anyhow::bail!("Unknown Remote value '{}'", other),
    }
    Ok(())
}

fn write_lockfile(file: &bmake_ast::BMakeFile, paths: &BMakePaths) -> Result<()> {
    let mut lock = bmake_engine::lockfile::Lockfile {
        engine: version::CURRENT_ENGINE_VERSION.to_string(),
        environment: format!(
            "{}{}",
            file.runs_on.clone().unwrap_or_default(),
            file.runs_on_version.clone().map(|v| format!(" {}", v)).unwrap_or_default()
        ),
        arch: file.arch.clone().unwrap_or_else(|| version::target_arch().to_string()),
        ..Default::default()
    };
    for dep in &file.dependencies {
        lock.dependency.insert(dep.name.clone(), dep.need.clone());
    }
    for tool in &file.tools {
        lock.tool.insert(tool.name.clone(), tool.need.clone());
    }
    bmake_engine::lockfile::write(paths, &lock)
}

fn cmd_init(kts: bool) -> Result<()> {
    let filename = if kts { "BMake.bm.kts" } else { "BMake.bm" };
    let path = PathBuf::from(filename);
    if path.exists() {
        anyhow::bail!("{} already exists in this directory", filename);
    }
    let template = format!(
        "<Version: {}>\n\nStart\n\nLang: Kotlin\nSystem: Gradle\n\n<Task: Build>\n    Command: ./gradlew build\n</Task>\n\nStop",
        version::CURRENT_ENGINE_VERSION
    );
    std::fs::write(&path, template)?;
    println!(" Created: {}", filename);
    if kts {
        println!(" This file runs through the Kotlin scripting runtime — add val/if/for as needed, or leave it as plain BMake DSL.");
    }
    Ok(())
}

fn cmd_check(file: Option<PathBuf>) -> Result<()> {
    let bm_path = find_bm_file(file)?;
    let bmake_file = parse_bm_or_kts(&bm_path)?;
    println!(" {} is valid", bm_path.display());
    println!("   Version : {}", bmake_file.version);
    println!("   Lang    : {:?}", bmake_file.lang);
    println!("   System  : {:?}", bmake_file.system);
    println!("   Runs-on : {:?} {:?}", bmake_file.runs_on, bmake_file.runs_on_version);
    let task_names: Vec<_> = bmake_file.tasks.iter().map(|t| t.name.clone()).collect();
    println!("   Tasks   : {}", task_names.join(", "));
    let _ = bmake_engine::graph::topological_waves(&bmake_file.tasks)?;
    println!("   Task dependency graph: OK (no cycles)");
    Ok(())
}

fn cmd_clean(deep: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = BMakePaths::new(&cwd);

    if let Ok(bm_path) = find_bm_file(None) {
        if let Ok(bmake_file) = parse_bm_or_kts(&bm_path) {
            for p in &bmake_file.clean_paths {
                let target = cwd.join(p);
                if target.exists() {
                    if target.is_dir() {
                        std::fs::remove_dir_all(&target)?;
                    } else {
                        std::fs::remove_file(&target)?;
                    }
                    println!(" Cleaned: {}", target.display());
                }
            }
        }
    }

    for d in [paths.cache(), paths.sandbox(), paths.tmp()] {
        if d.exists() {
            std::fs::remove_dir_all(&d)?;
            println!(" Cleaned: {}", d.display());
        }
    }

    if deep {
        for d in [paths.engines(), paths.dependencies()] {
            if d.exists() {
                std::fs::remove_dir_all(&d)?;
                println!(" Cleaned (deep): {}", d.display());
            }
        }
    } else {
        println!(" Kept .bmake/engines/ and .bmake/dependencies/ (use 'bmake clean --deep' to remove them)");
    }

    paths.ensure_all()?;
    Ok(())
}

fn cmd_migrate() -> Result<()> {
    println!(" BMake Migration Notes");
    println!(" Currently running engine: {}", version::CURRENT_ENGINE_VERSION);
    println!(" Migration notes between versions will be added as new versions are released.");
    Ok(())
}

fn cmd_login() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = BMakePaths::new(&cwd);
    paths.ensure_all()?;
    println!(" Enter your BMake Account token (from https://Zoder-Studio.github.io/BMake/):");
    let mut token = String::new();
    std::io::stdin().read_line(&mut token)?;
    let token = token.trim();
    if token.is_empty() {
        anyhow::bail!("Token cannot be empty");
    }
    std::fs::write(paths.credentials(), format!("token = \"{}\"\n", token))?;
    println!(" Login successful. Token stored at {}", paths.credentials().display());
    Ok(())
}

fn cmd_logout() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = BMakePaths::new(&cwd);
    if paths.credentials().exists() {
        std::fs::remove_file(paths.credentials())?;
        println!(" Logged out");
    } else {
        println!(" Not logged in");
    }
    Ok(())
}

fn cmd_version() -> Result<()> {
    println!("BMake Engine {}", version::CURRENT_ENGINE_VERSION);
    println!("Platform: {} / Arch: {}", version::target_platform(), version::target_arch());
    Ok(())
}

fn cmd_runner_register() -> Result<()> {
    println!(" Register a new BMake Runner");
    let name = prompt("Runner name")?;
    let runs_on = prompt("Runs-on (e.g. ubuntu, debian, android)")?;
    let version = prompt("version (e.g. 24.04)")?;
    let arch = prompt("Arch (e.g. x86_64, arm64)")?;
    let runner = bmake_engine::runner::register(&name, &runs_on, &version, &arch)?;
    println!(" Registered runner '{}' with ID {}", runner.name, runner.id);
    println!(" Status: OFFLINE — run 'bmake runner start {}' to bring it online", runner.id);
    Ok(())
}

fn prompt(label: &str) -> Result<String> {
    print!(" {}: ", label);
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn cmd_runner_start(id: &str) -> Result<()> {
    bmake_engine::runner::set_status(id, bmake_engine::runner::RunnerStatus::Online)?;
    println!(" Runner '{}' is now ONLINE", id);
    Ok(())
}

fn cmd_runner_status() -> Result<()> {
    let runners = bmake_engine::runner::load_all()?;
    if runners.is_empty() {
        println!(" No runners registered. Use 'bmake runner register' to add one.");
        return Ok(());
    }
    for r in runners {
        println!(
            " {} [{}]  Runs-on: {} version: {} Arch: {}  Status: {:?}",
            r.id, r.name, r.runs_on, r.version, r.arch, r.status
        );
    }
    Ok(())
}