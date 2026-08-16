mod editor_setup;
use anyhow::Result;
use bmake_engine::{dependency, executor, paths::BMakePaths, plugin, status::BuildStatus, version};
use clap::{Parser, Subcommand};
use serde_json::json;
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
    Run {
        file: Option<PathBuf>,
        #[arg(long)]
        verbose: bool,
        #[arg(long)]
        debug: bool,
        #[arg(long)]
        force: bool,
    },
    Init {
        #[arg(long)]
        kts: bool,
    },
    Check {
        file: Option<PathBuf>,
    },
    Clean {
        #[arg(long)]
        deep: bool,
    },
    Migrate,
    Login,
    Logout,
    Version,
    Runner {
        #[command(subcommand)]
        action: RunnerAction,
    },
}

#[derive(Subcommand)]
enum RunnerAction {
    Register,
    Start { id: String },
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

    if bmake_file.remote.as_deref() == Some("Local") {
        if let Ok(Some(session)) = bmake_engine::cloud::load_session() {
            let runs_on = bmake_file
                .runs_on
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Remote: Local requires 'Runs-on:' to be set"))?;
            let matched = bmake_engine::cloud::find_matching_runner(
                &session,
                &runs_on,
                bmake_file.runs_on_version.as_deref(),
                bmake_file.arch.as_deref(),
            )?;
            if let Some(runner) = matched {
                let raw_content = std::fs::read_to_string(&bm_path)?;
                let exit_code = dispatch_cloud_job(&session, &runner, &raw_content)?;
                std::process::exit(exit_code);
            }
            anyhow::bail!(
                "No online cloud Runner found matching Runs-on: {} version: {:?} Arch: {:?}. Register one with 'bmake runner register' and bring it online with 'bmake runner start <id>'.",
                runs_on,
                bmake_file.runs_on_version,
                bmake_file.arch
            );
        }
        handle_remote_local_fallback(&bmake_file)?;
    } else if let Some(remote) = bmake_file.remote.clone() {
        handle_remote(&remote, &bmake_file)?;
    }

    let paths = BMakePaths::new(&project_dir);
    paths.ensure_all()?;

    if bmake_file.version != version::CURRENT_ENGINE_VERSION {
        version::ensure_engine(&paths, &bmake_file.version)?;
        println!(" (Requested engine version differs from the running CLI — execution still uses this CLI's logic for now)");
    }

    dependency::ensure_requires(&bmake_file.requires)?;
    dependency::ensure_dependencies(&bmake_file.dependencies, &paths)?;
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

fn handle_remote(remote: &str, _bmake_file: &bmake_ast::BMakeFile) -> Result<()> {
    match remote {
        "CI" => println!(" Remote: CI requested — the control plane doesn't run CI jobs directly yet (only Runner dispatch is implemented); running locally as a fallback."),
        "Auto" => {}
        other => anyhow::bail!("Unknown Remote value '{}'", other),
    }
    Ok(())
}

fn handle_remote_local_fallback(bmake_file: &bmake_ast::BMakeFile) -> Result<()> {
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
        Some(r) => println!(
            " Matched local runner '{}' ({}) — not logged in to BMake cloud, executing here (single-machine mode)",
            r.name, r.id
        ),
        None => anyhow::bail!(
            "No online Runner found matching Runs-on: {} version: {:?} Arch: {:?}. Register one with 'bmake runner register', or 'bmake login' to use cloud Runners.",
            runs_on,
            bmake_file.runs_on_version,
            bmake_file.arch
        ),
    }
    Ok(())
}

fn dispatch_cloud_job(session: &bmake_engine::cloud::Session, runner: &serde_json::Value, bm_content: &str) -> Result<i32> {
    let runner_id = runner["id"].as_str().unwrap_or_default().to_string();
    let runner_name = runner["name"].as_str().unwrap_or_default().to_string();
    println!(" Matched cloud runner '{}' ({}) — dispatching job...", runner_name, runner_id);

    let build_id = bmake_engine::cloud::create_build(session, Some(&runner_id))?;
    let job = bmake_engine::cloud::create_job(session, &runner_id, bm_content, Some(&build_id))?;
    let job_id = job["id"].as_str().unwrap_or_default().to_string();
    if job_id.is_empty() {
        anyhow::bail!("Job creation did not return an id");
    }
    println!(" Job {} queued for build {} — waiting for the runner to pick it up...", job_id, build_id);

    let mut seen = 0usize;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3));

        let logs = bmake_engine::cloud::fetch_logs_since(session, &build_id, seen)?;
        for line in &logs {
            println!("{}", line);
        }
        seen += logs.len();

        let Some(job_status) = bmake_engine::cloud::get_job(session, &job_id)? else {
            anyhow::bail!("Job {} disappeared", job_id);
        };
        match job_status["status"].as_str().unwrap_or("PENDING") {
            "SUCCESS" => {
                println!("\n BUILD SUCCESS (remote runner '{}')", runner_name);
                return Ok(0);
            }
            "FAILED" | "CANCELLED" => {
                println!("\n BUILD FAILED (remote runner '{}')", runner_name);
                return Ok(1);
            }
            _ => {}
        }
    }
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
        "<Version: {}>\n\nStart\n\nLang: Kotlin\nSystem: Gradle\n\n<Task: Build>\n    Command: ./gradlew build\n</Task>\n\nStop\n",
        version::CURRENT_ENGINE_VERSION
    );
    std::fs::write(&path, template)?;
    println!(" Created: {}", filename);
    if kts {
        println!(" This file runs through the Kotlin scripting runtime — add val/if/for as needed, or leave it as plain BMake DSL.");
    }
    editor_setup::detect_and_setup();
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
    let supabase_url = std::env::var("BMAKE_SUPABASE_URL")
        .map_err(|_| anyhow::anyhow!("BMAKE_SUPABASE_URL is not set. Point it at your BMake control-plane Supabase project."))?;
    let anon_key = std::env::var("BMAKE_SUPABASE_ANON_KEY")
        .map_err(|_| anyhow::anyhow!("BMAKE_SUPABASE_ANON_KEY is not set. Point it at your BMake control-plane Supabase project."))?;

    let email = prompt("Email")?;
    print!(" Password: ");
    std::io::stdout().flush()?;
    let mut password = String::new();
    std::io::stdin().read_line(&mut password)?;
    let password = password.trim();

    let session = bmake_engine::cloud::login(&supabase_url, &anon_key, &email, password)?;
    println!(" Logged in as {} — session stored at ~/.bmake/credentials.toml", session.email);
    Ok(())
}

fn cmd_logout() -> Result<()> {
    bmake_engine::cloud::clear_session()?;
    println!(" Logged out");
    Ok(())
}

fn cmd_version() -> Result<()> {
    println!("BMake Engine {}", version::CURRENT_ENGINE_VERSION);
    println!("Platform: {} / Arch: {}", version::target_platform(), version::target_arch());
    Ok(())
}

fn prompt(label: &str) -> Result<String> {
    print!(" {}: ", label);
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn cmd_runner_register() -> Result<()> {
    println!(" Register a new BMake Runner");
    let name = prompt("Runner name")?;
    let runs_on = prompt("Runs-on (e.g. ubuntu, debian, android)")?;
    let version = prompt("version (e.g. 24.04)")?;
    let arch = prompt("Arch (e.g. x86_64, arm64)")?;

    if let Some(session) = bmake_engine::cloud::load_session()? {
        let runner = bmake_engine::cloud::register_runner(&session, &name, &runs_on, &version, &arch)?;
        let id = runner["id"].as_str().unwrap_or_default();
        println!(" Registered cloud runner '{}' with ID {}", name, id);
        println!(" Status: OFFLINE — run 'bmake runner start {}' to bring it online", id);
    } else {
        let runner = bmake_engine::runner::register(&name, &runs_on, &version, &arch)?;
        println!(" Registered local runner '{}' with ID {}", runner.name, runner.id);
        println!(" (Not logged in — this Runner only works in single-machine mode. Run 'bmake login' for cross-machine dispatch.)");
        println!(" Status: OFFLINE — run 'bmake runner start {}' to bring it online", runner.id);
    }
    Ok(())
}

fn cmd_runner_start(id: &str) -> Result<()> {
    let Some(session) = bmake_engine::cloud::load_session()? else {
        bmake_engine::runner::set_status(id, bmake_engine::runner::RunnerStatus::Online)?;
        println!(" Runner '{}' is now ONLINE (local-only — not logged in, so it can't receive jobs from other machines)", id);
        return Ok(());
    };

    bmake_engine::cloud::set_runner_status(&session, id, "ONLINE")?;
    println!(" Runner '{}' is now ONLINE and polling for jobs (Ctrl+C to stop)", id);

    loop {
        let jobs = bmake_engine::cloud::list_pending_jobs(&session, id)?;
        for job in jobs {
            let job_id = job["id"].as_str().unwrap_or_default().to_string();
            if job_id.is_empty() {
                continue;
            }
            if let Some(claimed) = bmake_engine::cloud::claim_job(&session, &job_id)? {
                run_claimed_job(&session, id, &claimed)?;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
}

fn run_claimed_job(session: &bmake_engine::cloud::Session, runner_id: &str, job: &serde_json::Value) -> Result<()> {
    let job_id = job["id"].as_str().unwrap_or_default().to_string();
    let build_id = job["build_id"].as_str().map(|s| s.to_string());
    let bm_content = job["bm_content"].as_str().unwrap_or_default().to_string();

    println!(" Claimed job {}", job_id);
    bmake_engine::cloud::set_runner_status(session, runner_id, "BUSY")?;
    bmake_engine::cloud::update_job_status(session, &job_id, "RUNNING")?;
    if let Some(bid) = &build_id {
        let _ = bmake_engine::cloud::update_build(session, bid, json!({ "status": "RUNNING" }));
    }

    let job_file = std::env::temp_dir().join(format!("bmake-job-{}.bm", job_id));
    std::fs::write(&job_file, &bm_content)?;

    let exe = std::env::current_exe()?;
    let mut child = std::process::Command::new(&exe)
        .args(["run", "--force"])
        .arg(&job_file)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let stdout_handle = child.stdout.take().map(|s| {
        let session = session.clone();
        let build_id = build_id.clone();
        std::thread::spawn(move || stream_and_upload(s, session, build_id))
    });
    let stderr_handle = child.stderr.take().map(|s| {
        let session = session.clone();
        let build_id = build_id.clone();
        std::thread::spawn(move || stream_and_upload(s, session, build_id))
    });

    let status = child.wait()?;
    if let Some(h) = stdout_handle {
        let _ = h.join();
    }
    if let Some(h) = stderr_handle {
        let _ = h.join();
    }
    let _ = std::fs::remove_file(&job_file);

    let final_status = if status.success() { "SUCCESS" } else { "FAILED" };
    bmake_engine::cloud::update_job_status(session, &job_id, final_status)?;
    if let Some(bid) = &build_id {
        let _ = bmake_engine::cloud::update_build(
            session,
            bid,
            json!({ "status": final_status, "exit_code": status.code().unwrap_or(-1) }),
        );
    }
    bmake_engine::cloud::set_runner_status(session, runner_id, "ONLINE")?;
    println!(" Job {} finished: {}", job_id, final_status);
    Ok(())
}

fn stream_and_upload(reader: impl std::io::Read, session: bmake_engine::cloud::Session, build_id: Option<String>) {
    use std::io::{BufRead, BufReader};
    let mut buffered: Vec<String> = Vec::new();
    for line in BufReader::new(reader).lines().flatten() {
        println!("{}", line);
        buffered.push(line);
        if buffered.len() >= 20 {
            if let Some(bid) = &build_id {
                let _ = bmake_engine::cloud::append_log(&session, bid, &buffered);
            }
            buffered.clear();
        }
    }
    if !buffered.is_empty() {
        if let Some(bid) = &build_id {
            let _ = bmake_engine::cloud::append_log(&session, bid, &buffered);
        }
    }
}

fn cmd_runner_status() -> Result<()> {
    if let Some(session) = bmake_engine::cloud::load_session()? {
        let runners = bmake_engine::cloud::list_runners(&session)?;
        if runners.is_empty() {
            println!(" No cloud runners registered. Use 'bmake runner register' to add one.");
            return Ok(());
        }
        for r in runners {
            println!(
                " {} [{}]  Runs-on: {} version: {} Arch: {}  Status: {}",
                r["id"].as_str().unwrap_or(""),
                r["name"].as_str().unwrap_or(""),
                r["runs_on"].as_str().unwrap_or(""),
                r["version"].as_str().unwrap_or(""),
                r["arch"].as_str().unwrap_or(""),
                r["status"].as_str().unwrap_or(""),
            );
        }
        return Ok(());
    }

    let runners = bmake_engine::runner::load_all()?;
    if runners.is_empty() {
        println!(" No local runners registered. Use 'bmake runner register' to add one.");
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