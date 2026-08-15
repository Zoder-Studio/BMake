use anyhow::{Context, Result};
use bmake_engine::{dependency, executor, paths::BMakePaths, status::BuildStatus, version};
use clap::{Parser, Subcommand};
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
    Run { file: Option<PathBuf> },
    /// Create a new BMake.bm in the current directory
    Init,
    /// Validate a .bm file without running it
    Check { file: Option<PathBuf> },
    /// Remove the .bmake/ cache directory
    Clean,
    /// Show migration notes between BMake versions
    Migrate,
    /// Authenticate with the BMake website
    Login,
    /// Remove stored BMake credentials
    Logout,
    /// Show the current BMake Engine version
    Version,
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Run { file } => cmd_run(file),
        Commands::Init => cmd_init(),
        Commands::Check { file } => cmd_check(file),
        Commands::Clean => cmd_clean(),
        Commands::Migrate => cmd_migrate(),
        Commands::Login => cmd_login(),
        Commands::Logout => cmd_logout(),
        Commands::Version => cmd_version(),
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

    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&cwd)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|ext| ext == "bm").unwrap_or(false))
        .collect();

    match candidates.len() {
        0 => anyhow::bail!("No '.bm' files were found in this directory."),
        1 => Ok(candidates.remove(0)),
        _ => anyhow::bail!("More than one '.bm' file was found. Run: bmake run <file>.bm"),
    }
}

fn cmd_run(file: Option<PathBuf>) -> Result<()> {
    let bm_path = find_bm_file(file)?;
    let project_dir = bm_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let content = std::fs::read_to_string(&bm_path).with_context(|| format!("Failed to Read {}", bm_path.display()))?;
    let bmake_file = bmake_parser::parse(&content).with_context(|| format!("Parsing failed {}", bm_path.display()))?;

    println!(" BMake {} — {}", bmake_file.version, bm_path.display());

    let paths = BMakePaths::new(&project_dir);
    paths.ensure_all()?;

    if bmake_file.version != version::CURRENT_ENGINE_VERSION {
        version::ensure_engine(&paths, &bmake_file.version)?;
        println!(" (Engine version is different from active CLI — in this phase the execution still uses CLI logic running)");
    }

    dependency::ensure_requires(&bmake_file.requires)?;
    dependency::ensure_dependencies(&bmake_file.dependencies)?;

    let status = executor::run_all_tasks(&bmake_file, &project_dir)?;

    match status {
        BuildStatus::Success => println!("\n BUILD SUCCESS"),
        BuildStatus::Failed => println!("\n BUILD FAILED"),
        other => println!("\n BUILD {:?}", other),
    }

    std::process::exit(status.exit_code());
}

fn cmd_init() -> Result<()> {
    let path = PathBuf::from("BMake.bm");
    if path.exists() {
        anyhow::bail!(" BMake.bm already exists in this directory");
    }
    let template = format!(
        "<Version: {}>\n\nStart\n\nLang = Kotlin\nSystem = Gradle\n\n<Task: Build>\n    Command = ./gradlew build\n</Task>\n\nStop",
        version::CURRENT_ENGINE_VERSION
    );
    std::fs::write(&path, template)?;
    println!(" Created: BMake.bm");
    Ok(())
}

fn cmd_check(file: Option<PathBuf>) -> Result<()> {
    let bm_path = find_bm_file(file)?;
    let content = std::fs::read_to_string(&bm_path)?;
    let bmake_file = bmake_parser::parse(&content)?;
    println!(" {} valid", bm_path.display());
    println!("   Version : {}", bmake_file.version);
    println!("   Lang    : {:?}", bmake_file.lang);
    println!("   System  : {:?}", bmake_file.system);
    let task_names: Vec<_> = bmake_file.tasks.iter().map(|t| t.name.clone()).collect();
    println!("   Tasks   : {}", task_names.join(", "));
    Ok(())
}

fn cmd_clean() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = BMakePaths::new(&cwd);
    if paths.root.exists() {
        std::fs::remove_dir_all(&paths.root)?;
        println!(" Cleaned: {}", paths.root.display());
    } else {
        println!(" There is no .bmake/ to clean up.");
    }
    Ok(())
}

fn cmd_migrate() -> Result<()> {
    println!(" BMake Migration Notes");
    println!(" Engine currently active: {}", version::CURRENT_ENGINE_VERSION);
    println!(" Migration notes between versions will be added as new versions are released.");
    Ok(())
}

fn cmd_login() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = BMakePaths::new(&cwd);
    paths.ensure_all()?;
    println!(" Enter BMake Account token (from https://Zoder-Studio.github.io/BMake/):");
    let mut token = String::new();
    std::io::stdin().read_line(&mut token)?;
    let token = token.trim();
    if token.is_empty() {
        anyhow::bail!("Token cannot be empty");
    }
    std::fs::write(paths.credentials(), format!("token = \"{}\"\n", token))?;
    println!(" Login successful. Token is stored in {}", paths.credentials().display());
    Ok(())
}

fn cmd_logout() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = BMakePaths::new(&cwd);
    if paths.credentials().exists() {
        std::fs::remove_file(paths.credentials())?;
        println!(" Logout successful");
    } else {
        println!(" Not logged in yet");
    }
    Ok(())
}

fn cmd_version() -> Result<()> {
    println!("BMake Engine {}", version::CURRENT_ENGINE_VERSION);
    println!("Platform: {} / Arch: {}", version::target_platform(), version::target_arch());
    Ok(())
}