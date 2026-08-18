mod ui;
use anyhow::Result;
use bmake_engine::{dependency, executor, paths::BMakePaths, plugin, status::BuildStatus, version};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
mod pager;
mod syntax_docs;
mod editor_setup;

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
        task: Option<String>,
        #[arg(long)]
        verbose: bool,
        #[arg(long)]
        debug: bool,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        no_color: bool,
        #[arg(long)]
        no_animation: bool,
    },
    /// Create a new BMake.bm (or BMake.bm.kts) in the current directory
    Init {
        #[arg(long)]
        kts: bool,
    },
    /// Validate a .bm file without running it
    Check { file: Option<PathBuf> },
    /// Validate syntax, task graph, dependencies, and Runner config without running
    Validate { file: Option<PathBuf> },
    /// List tasks, tools, dependencies, or artifacts without running
    List { what: Option<String> },
    /// Show the Task dependency graph
    Graph {
        #[arg(long)]
        dot: bool,
    },
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
    /// Show the actual local build environment
    Env,
    /// Show or clear the BMake build cache
    Cache {
        #[command(subcommand)]
        action: Option<CacheAction>,
    },
    /// Show past build logs
    Logs {
        build_id: Option<String>,
        #[arg(long)]
        last: bool,
    },
    /// Explain why a Task would run (dependencies, condition, environment, command)
    Explain { task: String },
    /// Open the interactive BMake syntax reference
    Syntax {
        #[arg(long)]
        version: Option<String>,
    },
    /// Manage BMake Runners
    Runner {
        #[command(subcommand)]
        action: RunnerAction,
    },
    /// Add a secret to the local vault (secret.bm.locksys)
    Add {
        #[command(subcommand)]
        what: AddKind,
    },
    /// Remove a secret from the local vault
    Remove {
        #[command(subcommand)]
        what: RemoveKind,
    },
    /// Alias group for secret management (same implementation as add/remove)
    Secret {
        #[command(subcommand)]
        action: SecretAction,
    },
}

#[derive(Subcommand)]
enum RunnerAction {
    Register,
    Start { id: String },
    Status,
}

#[derive(Subcommand)]
enum AddKind {
    Secret { name: String },
}

#[derive(Subcommand)]
enum RemoveKind {
    Secret { name: String },
}

#[derive(Subcommand)]
enum SecretAction {
    Add { name: String },
    List,
    Remove { name: String },
    /// Add/update a secret in the BMake Online Secret Store (cloud, requires login)
    AddOnline { name: String },
    /// List secret names in the BMake Online Secret Store
    ListOnline,
    /// Grant a Runner permission to use a cloud secret
    Grant { name: String, runner_id: String },
}

#[derive(Subcommand)]
enum CacheAction {
    Info,
    Clear,
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Run { file, task, verbose, debug, force, no_color, no_animation } => {
            cmd_run(file, task, verbose, debug, force, no_color, no_animation)
        }
        Commands::Init { kts } => cmd_init(kts),
        Commands::Check { file } => cmd_check(file),
        Commands::Validate { file } => cmd_validate(file),
        Commands::List { what } => cmd_list(what),
        Commands::Graph { dot } => cmd_graph(dot),
        Commands::Clean { deep } => cmd_clean(deep),
        Commands::Migrate => cmd_migrate(),
        Commands::Login => cmd_login(),
        Commands::Logout => cmd_logout(),
        Commands::Version => cmd_version(),
        Commands::Env => cmd_env(),
        Commands::Cache { action } => cmd_cache(action),
        Commands::Logs { build_id, last } => cmd_logs(build_id, last),
        Commands::Explain { task } => cmd_explain(task),
        Commands::Syntax { version } => pager::run(version.as_deref()),
        Commands::Runner { action } => match action {
            RunnerAction::Register => cmd_runner_register(),
            RunnerAction::Start { id } => cmd_runner_start(&id),
            RunnerAction::Status => cmd_runner_status(),
        },
        Commands::Add { what: AddKind::Secret { name } } => cmd_secret_add(&name),
        Commands::Remove { what: RemoveKind::Secret { name } } => cmd_secret_remove(&name),
        Commands::Secret { action } => match action {
            SecretAction::Add { name } => cmd_secret_add(&name),
            SecretAction::List => cmd_secret_list(),
            SecretAction::Remove { name } => cmd_secret_remove(&name),
            SecretAction::AddOnline { name } => cmd_secret_add_online(&name),
            SecretAction::ListOnline => cmd_secret_list_online(),
            SecretAction::Grant { name, runner_id } => cmd_secret_grant(&name, &runner_id),
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

fn cmd_run(
    file: Option<PathBuf>,
    task: Option<String>,
    verbose: bool,
    debug: bool,
    force: bool,
    no_color: bool,
    no_animation: bool,
) -> Result<()> {
    let bm_path = find_bm_file(file)?;
    let project_dir = bm_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut bmake_file = parse_bm_or_kts(&bm_path)?;

    if debug {
        bmake_file.log_level = bmake_ast::LogLevel::Debug;
    } else if verbose {
        bmake_file.log_level = bmake_ast::LogLevel::Verbose;
    }

    println!(" BMake {} — {}", bmake_file.version, bm_path.display());

    if let Some(task_name) = &task {
        if bmake_file.remote.as_deref() == Some("Local") {
            anyhow::bail!("--task is not yet supported together with 'Remote: Local' cloud dispatch");
        }
        let filtered = bmake_engine::graph::transitive_closure(&bmake_file.tasks, task_name)?;
        println!(
            " --task {}: running {} task(s): {}",
            task_name,
            filtered.len(),
            filtered.iter().map(|t| t.name.clone()).collect::<Vec<_>>().join(", ")
        );
        bmake_file.tasks = filtered;
    }

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
                // Ships the raw .bm text as-is — the Runner machine resolves
                // its own Secret references against its OWN local vault, so
                // decrypted secret values never leave this machine.
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

    // Resolve Secret before Tool/Dependency, per the pre-flight order:
    // Parse -> Validate -> Resolve Secret -> Resolve Tool -> Resolve Runner
    // -> Build Dependency Graph -> Execute. Only the specific secrets this
    // file references are ever decrypted.
    let secret_names = bmake_engine::values::referenced_secret_names(&bmake_file);
    let mut secret_values: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if !secret_names.is_empty() {
        let is_ci = std::env::var_os("CI").is_some() || std::env::var_os("GITHUB_ACTIONS").is_some();
        let remote_ci = bmake_file.remote.as_deref() == Some("CI");

        if remote_ci || is_ci {
            // Never uploads Local secrets to CI automatically — only reads
            // whatever the workflow author already exposed as env vars.
            println!("SECRET: adding secret from CI secret store");
            for name in &secret_names {
                let Ok(value) = std::env::var(name) else {
                    anyhow::bail!(
                        "Secret \"{}\" was not found.\n\nFor CI, expose it as an environment variable in your workflow, e.g.:\n\n    env:\n      {}: ${{{{ secrets.{} }}}}",
                        name,
                        name,
                        name
                    );
                };
                secret_values.insert(name.clone(), value);
            }
        } else if let Ok(Some(session)) = bmake_engine::cloud::load_session() {
            println!("SECRET: adding secret from BMake Secret Store");
            let runner_id = std::env::var("BMAKE_RUNNER_ID").map_err(|_| {
                anyhow::anyhow!(
                    "BMake Online Secret Store requires the build to run through a registered Runner. Start one with 'bmake runner start <id>', or use a local secret.bm.locksys vault instead."
                )
            })?;
            for name in &secret_names {
                let value = bmake_engine::cloud::get_online_secret(&session, name, &runner_id)?;
                secret_values.insert(name.clone(), value);
            }
        } else {
            println!("SECRET: adding secret from secret.bm.locksys");
            let vault = bmake_engine::vault::Vault::at(&project_dir);
            if !vault.exists() {
                let first = secret_names.iter().next().cloned().unwrap_or_default();
                anyhow::bail!("Secret \"{}\" was not found.\n\nCreate it with:\n\n    bmake add secret {}", first, first);
            }
            let passphrase = rpassword::prompt_password(" vault passphrase: ")?;
            let names_vec: Vec<String> = secret_names.iter().cloned().collect();
            secret_values = vault.get_secrets(&passphrase, &names_vec)?;
        }
    }
    let (_, secret_values_used) = bmake_engine::values::resolve(&mut bmake_file, &secret_values)?;

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
    println!(" Build ID: {}", build_id);

    let (tx, rx) = std::sync::mpsc::channel::<bmake_engine::events::TaskEvent>();
    let ui_opts = ui::UiOptions::detect(no_color, no_animation);

    let file_for_exec = bmake_file.clone();
    let project_dir_for_exec = project_dir.clone();
    let paths_for_exec = BMakePaths { root: paths.root.clone() };
    let build_id_for_exec = build_id.clone();
    let secrets_for_exec = secret_values_used;
    let exec_handle = std::thread::spawn(move || {
        executor::run_all_tasks(
            &file_for_exec,
            &project_dir_for_exec,
            &paths_for_exec,
            force,
            &build_id_for_exec,
            &tx,
            &secrets_for_exec,
        )
    });

    ui::render_loop(rx, &ui_opts);

    let status = exec_handle.join().map_err(|_| anyhow::anyhow!("Build executor thread panicked"))??;

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
    bmake_engine::logstore::write_meta(&paths, &meta.build_id, &meta)?;

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
    // No trailing newline, no trailing whitespace of any kind after "Stop" —
    // the file ends exactly at the last byte of "Stop".
    let template = format!(
        "<Version: {}>\n\nStart\n\nLang: Kotlin\nSystem: Gradle\n\n<Task: Build>\n    Command: ./gradlew build\n</Task>\n\nStop",
        version::CURRENT_ENGINE_VERSION
    );
    std::fs::write(&path, template.as_bytes())?;
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

fn cmd_validate(file: Option<PathBuf>) -> Result<()> {
    let bm_path = find_bm_file(file)?;
    println!("BMake Validation\n");

    let mut bmake_file = match parse_bm_or_kts(&bm_path) {
        Ok(f) => f,
        Err(e) => {
            println!("✗ Syntax invalid\n");
            return Err(e);
        }
    };
    println!("✓ Syntax valid");

    match bmake_engine::values::resolve_values_only(&mut bmake_file) {
        Ok(report) => {
            println!("✓ Value references valid");
            let all = bmake_engine::values::all_paths(&bmake_file.values);
            let unused: Vec<&String> = all.difference(&report.used_paths).collect();
            if !unused.is_empty() {
                let mut sorted = unused.clone();
                sorted.sort();
                for path in sorted {
                    println!("⚠ Unused value detected:\n\n    Value.{}\n", path);
                    warnings += 1;
                }
            }
        }
        Err(e) => {
            println!("✗ Value reference invalid\n");
            return Err(e);
        }
    }

    let referenced_secrets = bmake_engine::values::referenced_secret_names(&bmake_file);
    let declared_secrets: std::collections::HashSet<&str> = bmake_file.secrets.iter().map(|s| s.as_str()).collect();
    for name in &referenced_secrets {
        if !declared_secrets.contains(name.as_str()) {
            println!("⚠ Secret '{}' is referenced but not declared with 'Secret: {}'", name, name);
            warnings += 1;
        }
    }
    if !bmake_file.secrets.is_empty() {
        println!("✓ Secret declarations present ({})", bmake_file.secrets.join(", "));
    }

    if bmake_file.version == version::CURRENT_ENGINE_VERSION {
        println!("✓ Version supported ({})", bmake_file.version);
    } else {
        println!(
            "• Version {} requested (running engine is {}) — resolved from .bmake/engines/ or GitHub on 'bmake run'",
            bmake_file.version,
            version::CURRENT_ENGINE_VERSION
        );
    }

    let mut warnings = 0u32;

    for dep in &bmake_file.dependencies {
        if which::which(&dep.need).is_ok() {
            println!("✓ Dependency '{}' found ({})", dep.name, dep.need);
        } else {
            println!("• Dependency '{}' not found locally ({}) — will be installed on 'bmake run'", dep.name, dep.need);
        }
    }
    for tool in &bmake_file.tools {
        if which::which(&tool.name).is_ok() {
            println!("✓ Tool '{}' found", tool.name);
        } else {
            println!("• Tool '{}' not found locally — will be installed on 'bmake run'", tool.name);
        }
    }
    println!("✓ Dependencies valid");

    match bmake_engine::graph::topological_waves(&bmake_file.tasks) {
        Ok(_) => println!("✓ Task graph valid ({} task(s))", bmake_file.tasks.len()),
        Err(e) => {
            println!("✗ Task graph invalid\n");
            return Err(e);
        }
    }

    for task in &bmake_file.tasks {
        if let Some(cond) = &task.condition {
            if cond.split_once("==").is_none() {
                println!("⚠ Task '{}' has a Condition that doesn't match 'Field == Value': {}", task.name, cond);
                warnings += 1;
            }
        }
        if let Some(w) = &task.workdir {
            if w.trim().is_empty() {
                println!("⚠ Task '{}' has an empty Workdir", task.name);
                warnings += 1;
            }
        }
    }

    if bmake_file.remote.as_deref() == Some("Local") && bmake_file.runs_on.is_none() {
        println!("⚠ Remote: Local is set but Runs-on: is missing");
        warnings += 1;
    } else if bmake_file.remote.is_some() {
        println!(
            "✓ Runner configuration valid (Remote: {:?}, Runs-on: {:?}, version: {:?}, Arch: {:?})",
            bmake_file.remote, bmake_file.runs_on, bmake_file.runs_on_version, bmake_file.arch
        );
    }

    if bmake_file.sub_system.is_some() && bmake_file.system.is_none() {
        println!("⚠ Sub-System is set but System is missing — Sub-System has nothing to attach to");
        warnings += 1;
    }
    println!();
    if warnings == 0 {
        println!("BMake file is valid.");
    } else {
        println!("BMake file is valid, with {} warning(s).", warnings);
    }
    Ok(())
}

fn cmd_list(what: Option<String>) -> Result<()> {
    let bm_path = find_bm_file(None)?;
    let bmake_file = parse_bm_or_kts(&bm_path)?;

    match what.as_deref() {
        None | Some("tasks") => {
            println!("BMake Tasks\n");
            for t in &bmake_file.tasks {
                println!("  {}", t.name);
            }
        }
        Some("tools") => {
            println!("BMake Tools\n");
            for t in &bmake_file.tools {
                println!("  {} (need {})", t.name, t.need);
            }
        }
        Some("dependencies") => {
            println!("BMake Dependencies\n");
            for d in &bmake_file.dependencies {
                println!("  {} (need {})", d.name, d.need);
            }
        }
        Some("artifacts") => {
            println!("BMake Artifacts\n");
            for a in bmake_file.artifacts.iter().chain(bmake_file.tasks.iter().flat_map(|t| t.artifacts.iter())) {
                println!("  {}", a);
            }
        }
        Some("secrets") => return cmd_secret_list(),
        Some(other) => anyhow::bail!("Unknown 'bmake list {}'. Try: tasks, tools, dependencies, artifacts", other),
    }
    Ok(())
}

fn cmd_graph(dot: bool) -> Result<()> {
    let bm_path = find_bm_file(None)?;
    let bmake_file = parse_bm_or_kts(&bm_path)?;

    // Validates first — this is where a circular dependency surfaces with
    // the exact "A -> B -> C -> A" chain, and it never loops forever.
    bmake_engine::graph::topological_waves(&bmake_file.tasks)?;

    if dot {
        println!("digraph BMake {{");
        for t in &bmake_file.tasks {
            if t.depends_on.is_empty() {
                println!("  \"{}\";", t.name);
            }
            for dep in &t.depends_on {
                println!("  \"{}\" -> \"{}\";", dep, t.name);
            }
        }
        println!("}}");
        return Ok(());
    }

    let mut dependents: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for t in &bmake_file.tasks {
        for dep in &t.depends_on {
            dependents.entry(dep.as_str()).or_default().push(t.name.as_str());
        }
    }
    let has_dep: std::collections::HashSet<&str> =
        bmake_file.tasks.iter().filter(|t| !t.depends_on.is_empty()).map(|t| t.name.as_str()).collect();
    let roots: Vec<&str> = bmake_file.tasks.iter().map(|t| t.name.as_str()).filter(|n| !has_dep.contains(n)).collect();

    for root in &roots {
        println!("{}", root);
        print_children(root, &dependents, "");
    }
    Ok(())
}

fn print_children(name: &str, dependents: &std::collections::HashMap<&str, Vec<&str>>, prefix: &str) {
    if let Some(children) = dependents.get(name) {
        for (i, child) in children.iter().enumerate() {
            let last = i == children.len() - 1;
            let connector = if last { "└── " } else { "├── " };
            println!("{}{}{}", prefix, connector, child);
            let new_prefix = format!("{}{}", prefix, if last { "    " } else { "│   " });
            print_children(child, dependents, &new_prefix);
        }
    }
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
    let password = rpassword::prompt_password(" Password: ")?;

    let session = bmake_engine::cloud::login(&supabase_url, &anon_key, &email, &password)?;
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

fn cmd_env() -> Result<()> {
    println!("BMake Environment\n");
    println!("OS:");
    println!("  {}", version::target_platform());
    println!("Architecture:");
    println!("  {}", version::target_arch());
    println!("Engine:");
    println!("  {}", version::CURRENT_ENGINE_VERSION);

    if let Ok(shell) = std::env::var("SHELL") {
        println!("Shell:");
        println!("  {}", shell);
    }

    println!("Cloud account:");
    match bmake_engine::cloud::load_session() {
        Ok(Some(session)) => println!("  {}", session.email),
        _ => println!("  not logged in"),
    }

    println!("Detected tools:");
    let mut any = false;
    for tool in ["java", "gradle", "cmake", "ninja", "cargo", "kotlin", "kotlinc"] {
        if dependency::check_require(tool) {
            println!("  {}: found", tool);
            any = true;
        }
    }
    if !any {
        println!("  (none of the common build tools were found on PATH)");
    }
    Ok(())
}

fn cmd_cache(action: Option<CacheAction>) -> Result<()> {
    match action.unwrap_or(CacheAction::Info) {
        CacheAction::Info => cmd_cache_info(),
        CacheAction::Clear => cmd_cache_clear(),
    }
}

fn cmd_cache_info() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = BMakePaths::new(&cwd);
    let info = bmake_engine::cache::info(&paths)?;
    println!("BMake Cache\n");
    println!("Location: {}", info.location.display());
    println!("Size: {}", human_size(info.size_bytes));
    println!("Entries: {}", info.entries);
    println!("Incremental build hits/misses: {}/{}", info.hits, info.misses);
    Ok(())
}

fn cmd_cache_clear() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = BMakePaths::new(&cwd);
    bmake_engine::cache::clear(&paths)?;
    println!(" Cleared {} (engines/ and dependencies/ were not touched)", paths.cache().display());
    Ok(())
}

fn cmd_secret_add(name: &str) -> Result<()> {
    bmake_engine::vault::validate_secret_name(name)?;
    let cwd = std::env::current_dir()?;
    let vault = bmake_engine::vault::Vault::at(&cwd);

    println!("BMake — Secret Vault\n");
    println!("Secret: {}\n", name);

    let passphrase = if vault.exists() {
        rpassword::prompt_password(" vault passphrase: ")?
    } else {
        println!(" No vault found — creating a new one at secret.bm.locksys");
        let p1 = rpassword::prompt_password(" set a new vault passphrase: ")?;
        let p2 = rpassword::prompt_password(" confirm passphrase: ")?;
        if p1 != p2 {
            anyhow::bail!("Passphrases did not match");
        }
        vault.create(&p1)?;
        p1
    };

    let value = rpassword::prompt_password(" add value: ")?;
    vault.add_secret(&passphrase, name, &value)?;
    println!("\n✓ Secret {} created", name);
    Ok(())
}

fn cmd_secret_list() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let vault = bmake_engine::vault::Vault::at(&cwd);
    if !vault.exists() {
        println!("Secrets:\n\n  (no vault found — use 'bmake add secret <name>' to create one)");
        return Ok(());
    }
    let passphrase = rpassword::prompt_password(" vault passphrase: ")?;
    let names = vault.list_names(&passphrase)?;
    println!("Secrets:\n");
    if names.is_empty() {
        println!("  (none)");
    }
    for n in names {
        println!("  {}", n);
    }
    Ok(())
}

fn cmd_secret_remove(name: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let vault = bmake_engine::vault::Vault::at(&cwd);
    if !vault.exists() {
        anyhow::bail!("No vault found at secret.bm.locksys");
    }
    let confirm = prompt(&format!("Remove secret '{}'? Type 'yes' to confirm", name))?;
    if confirm.trim() != "yes" {
        println!(" Cancelled");
        return Ok(());
    }
    let passphrase = rpassword::prompt_password(" vault passphrase: ")?;
    let removed = vault.remove_secret(&passphrase, name)?;
    if removed {
        println!("✓ Secret {} removed", name);
    } else {
        println!(" Secret {} was not found in the vault", name);
    }
    Ok(())
}

fn cmd_secret_add_online(name: &str) -> Result<()> {
    bmake_engine::vault::validate_secret_name(name)?;
    let Some(session) = bmake_engine::cloud::load_session()? else {
        anyhow::bail!("Not logged in. Run 'bmake login' first.");
    };
    println!("BMake — Online Secret Store\n");
    println!("Secret: {}\n", name);
    let value = rpassword::prompt_password(" add value: ")?;
    bmake_engine::cloud::add_online_secret(&session, name, &value)?;
    println!("\n✓ Secret {} created in the BMake Online Secret Store", name);
    println!(" Grant it to a Runner with: bmake secret grant {} <runner-id>", name);
    Ok(())
}

fn cmd_secret_list_online() -> Result<()> {
    let Some(session) = bmake_engine::cloud::load_session()? else {
        anyhow::bail!("Not logged in. Run 'bmake login' first.");
    };
    let names = bmake_engine::cloud::list_online_secrets(&session)?;
    println!("Secrets (BMake Online Secret Store):\n");
    if names.is_empty() {
        println!("  (none)");
    }
    for n in names {
        println!("  {}", n);
    }
    Ok(())
}

fn cmd_secret_grant(name: &str, runner_id: &str) -> Result<()> {
    let Some(session) = bmake_engine::cloud::load_session()? else {
        anyhow::bail!("Not logged in. Run 'bmake login' first.");
    };
    bmake_engine::cloud::grant_secret_to_runner(&session, name, runner_id)?;
    println!("✓ Runner {} can now request secret {}", runner_id, name);
    Ok(())
}

fn cmd_explain(task_name: String) -> Result<()> {
    let bm_path = find_bm_file(None)?;
    let bmake_file = parse_bm_or_kts(&bm_path)?;

    let Some(task) = bmake_file.tasks.iter().find(|t| t.name == task_name) else {
        anyhow::bail!("Task '{}' not found in {}", task_name, bm_path.display());
    };

    println!("Task: {}\n", task.name);

    if !task.depends_on.is_empty() {
        println!("Depends-on:");
        for dep in &task.depends_on {
            let exists = bmake_file.tasks.iter().any(|t| t.name == *dep);
            println!("  {} {}", if exists { "✓" } else { "✗" }, dep);
        }
        println!();
    }

    if let Some(cond) = &task.condition {
        let result = bmake_engine::executor::evaluate_condition(cond, &bmake_file);
        println!("Condition:");
        println!("  {}", cond);
        println!("  Result: {}\n", if result { "TRUE" } else { "FALSE" });
    }

    if let Some(w) = task.workdir.as_ref().or(bmake_file.workdir.as_ref()) {
        println!("Workdir:\n  {}\n", w);
    }

    if let Some(runs_on) = &bmake_file.runs_on {
        println!(
            "Runs-on:\n  {} {}\n",
            runs_on,
            bmake_file.runs_on_version.clone().unwrap_or_default()
        );
    }

    println!("Shell:\n  {}\n", bmake_file.shell.clone().unwrap_or_else(|| "sh (default)".to_string()));

    if !task.commands.is_empty() {
        println!("Command:");
        for step in &task.commands {
            println!("  {}", step.command);
            if let Some(oe) = &step.on_error {
                println!("    OnError: {:?}", oe);
            }
            if let Some(t) = step.timeout.or(task.timeout) {
                println!("    Timeout: {}s", t);
            }
        }
        println!();
    }

    let artifacts: Vec<&String> = task.artifacts.iter().chain(bmake_file.artifacts.iter()).collect();
    if !artifacts.is_empty() {
        println!("Artifact:");
        for a in artifacts {
            println!("  {}", a);
        }
        println!();
    }

    if !task.inputs.is_empty() || !task.outputs.is_empty() {
        println!("Input:  {}", task.inputs.join(", "));
        println!("Output: {}", task.outputs.join(", "));
    }

    Ok(())
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", size, UNITS[unit])
    }
}

fn cmd_logs(build_id: Option<String>, last: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = BMakePaths::new(&cwd);

    if let Some(id) = build_id {
        return show_log(&paths, &id);
    }
    if last {
        let ids = bmake_engine::logstore::list_builds(&paths)?;
        let Some(id) = ids.last() else {
            println!(" No builds logged yet.");
            return Ok(());
        };
        return show_log(&paths, id);
    }

    let ids = bmake_engine::logstore::list_builds(&paths)?;
    if ids.is_empty() {
        println!(" No builds logged yet. Run 'bmake run' first.");
        return Ok(());
    }
    println!("BMake Build Logs\n");
    for id in ids.iter().rev().take(20) {
        match bmake_engine::logstore::read_meta(&paths, id) {
            Some(meta) => println!("  {}  {}  {}s  exit {}", id, meta.status, meta.duration_secs, meta.exit_code),
            None => println!("  {}", id),
        }
    }
    println!("\nUse 'bmake logs <build-id>' or 'bmake logs --last' to view full output.");
    Ok(())
}

fn show_log(paths: &BMakePaths, build_id: &str) -> Result<()> {
    if let Some(meta) = bmake_engine::logstore::read_meta(paths, build_id) {
        println!("Build ID: {}", build_id);
        println!("Status: {}", meta.status);
        println!("Runs-on: {:?} {:?}", meta.runs_on, meta.runs_on_version);
        println!("Duration: {}s  Exit code: {}", meta.duration_secs, meta.exit_code);
        println!();
    } else {
        println!("Build ID: {} (no metadata found)\n", build_id);
    }
    match bmake_engine::logstore::read_log(paths, build_id) {
        Ok(content) => print!("{}", content),
        Err(_) => println!(" No log output stored for this build."),
    }
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
        .env("BMAKE_RUNNER_ID", runner_id)
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