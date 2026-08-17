use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;

fn bmake() -> Command {
    Command::cargo_bin("bmake").unwrap()
}

fn write_bm(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

const SIMPLE_BM: &str = "<Version: 1.0>\n\nStart\n\n<Task: Build>\n    Command: echo build-ran\n</Task>\n\nStop";

// ---------- init ----------

#[test]
fn init_creates_file_ending_exactly_at_stop_no_trailing_bytes() {
    let dir = tempfile::tempdir().unwrap();
    bmake().current_dir(dir.path()).arg("init").assert().success();

    let bytes = fs::read(dir.path().join("BMake.bm")).unwrap();
    assert!(bytes.ends_with(b"Stop"), "file must end exactly with 'Stop'");
}

#[test]
fn init_kts_creates_bm_kts_file() {
    let dir = tempfile::tempdir().unwrap();
    bmake().current_dir(dir.path()).args(["init", "--kts"]).assert().success();
    assert!(dir.path().join("BMake.bm.kts").exists());
}

#[test]
fn init_fails_if_file_already_exists() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    bmake().current_dir(dir.path()).arg("init").assert().failure();
}

// ---------- validate ----------

#[test]
fn validate_passes_for_valid_file() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    bmake()
        .current_dir(dir.path())
        .arg("validate")
        .assert()
        .success()
        .stdout(predicate::str::contains("BMake file is valid"));
}

#[test]
fn validate_fails_for_unknown_directive() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nNotReal: value\n\nStop");
    bmake().current_dir(dir.path()).arg("validate").assert().failure();
}

#[test]
fn validate_reports_circular_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let src = "<Version: 1.0>\n\nStart\n\n<Task: A>\n    Depends-on: B\n    Command: echo a\n</Task>\n\n<Task: B>\n    Depends-on: A\n    Command: echo b\n</Task>\n\nStop";
    write_bm(dir.path(), "BMake.bm", src);
    bmake()
        .current_dir(dir.path())
        .arg("validate")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Circular"));
}

// ---------- run ----------

#[test]
fn run_executes_task_and_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .assert()
        .success();
}

#[test]
fn run_with_task_flag_runs_only_transitive_closure() {
    let dir = tempfile::tempdir().unwrap();
    let marker_compile = dir.path().join("compile.marker");
    let marker_package = dir.path().join("package.marker");
    let src = format!(
        "<Version: 1.0>\n\nStart\n\n<Task: Compile>\n    Command: touch {}\n</Task>\n\n<Task: Package>\n    Depends-on: Compile\n    Command: touch {}\n</Task>\n\nStop",
        marker_compile.display(),
        marker_package.display()
    );
    write_bm(dir.path(), "BMake.bm", &src);

    bmake()
        .current_dir(dir.path())
        .args(["run", "--task", "Compile", "--no-color", "--no-animation"])
        .assert()
        .success();

    assert!(marker_compile.exists(), "Compile should have run");
    assert!(!marker_package.exists(), "Package should NOT have run for --task Compile");
}

#[test]
fn run_full_graph_runs_every_task() {
    let dir = tempfile::tempdir().unwrap();
    let marker_compile = dir.path().join("compile.marker");
    let marker_package = dir.path().join("package.marker");
    let src = format!(
        "<Version: 1.0>\n\nStart\n\n<Task: Compile>\n    Command: touch {}\n</Task>\n\n<Task: Package>\n    Depends-on: Compile\n    Command: touch {}\n</Task>\n\nStop",
        marker_compile.display(),
        marker_package.display()
    );
    write_bm(dir.path(), "BMake.bm", &src);

    bmake().current_dir(dir.path()).args(["run", "--no-color", "--no-animation"]).assert().success();

    assert!(marker_compile.exists());
    assert!(marker_package.exists());
}

// ---------- list ----------

#[test]
fn list_tasks_shows_task_names() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    bmake()
        .current_dir(dir.path())
        .args(["list", "tasks"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Build"));
}

#[test]
fn list_unknown_subcommand_errors() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    bmake().current_dir(dir.path()).args(["list", "not-a-real-thing"]).assert().failure();
}

// ---------- graph ----------

#[test]
fn graph_shows_dependency_tree() {
    let dir = tempfile::tempdir().unwrap();
    let src = "<Version: 1.0>\n\nStart\n\n<Task: Compile>\n    Command: echo compile\n</Task>\n\n<Task: Package>\n    Depends-on: Compile\n    Command: echo package\n</Task>\n\nStop";
    write_bm(dir.path(), "BMake.bm", src);
    bmake()
        .current_dir(dir.path())
        .arg("graph")
        .assert()
        .success()
        .stdout(predicate::str::contains("Compile"))
        .stdout(predicate::str::contains("Package"));
}

#[test]
fn graph_dot_output_is_dot_format() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    bmake()
        .current_dir(dir.path())
        .args(["graph", "--dot"])
        .assert()
        .success()
        .stdout(predicate::str::contains("digraph BMake"));
}

#[test]
fn graph_detects_circular_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let src = "<Version: 1.0>\n\nStart\n\n<Task: A>\n    Depends-on: B\n    Command: echo a\n</Task>\n\n<Task: B>\n    Depends-on: A\n    Command: echo b\n</Task>\n\nStop";
    write_bm(dir.path(), "BMake.bm", src);
    bmake()
        .current_dir(dir.path())
        .arg("graph")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Circular"));
}

// ---------- explain ----------

#[test]
fn explain_shows_condition_result() {
    let dir = tempfile::tempdir().unwrap();
    let src = "<Version: 1.0>\n\nStart\n\nProfile: Release\n\n<Task: Release>\n    Condition: Profile == Release\n    Command: echo release\n</Task>\n\nStop";
    write_bm(dir.path(), "BMake.bm", src);
    bmake()
        .current_dir(dir.path())
        .args(["explain", "Release"])
        .assert()
        .success()
        .stdout(predicate::str::contains("TRUE"));
}

#[test]
fn explain_unknown_task_errors() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    bmake().current_dir(dir.path()).args(["explain", "DoesNotExist"]).assert().failure();
}

// ---------- clean / cache ----------

#[test]
fn clean_removes_cache_but_keeps_engines_and_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    let bmake_dir = dir.path().join(".bmake");
    fs::create_dir_all(bmake_dir.join("cache")).unwrap();
    fs::create_dir_all(bmake_dir.join("engines/1.0")).unwrap();
    fs::create_dir_all(bmake_dir.join("dependencies")).unwrap();
    fs::write(bmake_dir.join("cache/leftover.txt"), b"x").unwrap();
    fs::write(bmake_dir.join("engines/1.0/bmake-engine"), b"fake").unwrap();

    bmake().current_dir(dir.path()).arg("clean").assert().success();

    assert!(!bmake_dir.join("cache/leftover.txt").exists());
    assert!(bmake_dir.join("engines/1.0/bmake-engine").exists(), "engines/ must survive plain clean");
    assert!(bmake_dir.join("dependencies").exists(), "dependencies/ must survive plain clean");
}

#[test]
fn clean_deep_requires_explicit_flag_to_remove_engines() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    let bmake_dir = dir.path().join(".bmake");
    fs::create_dir_all(bmake_dir.join("engines/1.0")).unwrap();
    fs::write(bmake_dir.join("engines/1.0/bmake-engine"), b"fake").unwrap();

    bmake().current_dir(dir.path()).args(["clean", "--deep"]).assert().success();

    assert!(!bmake_dir.join("engines").exists());
}

#[test]
fn cache_info_runs_without_error() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    bmake()
        .current_dir(dir.path())
        .args(["cache", "info"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Location"));
}

#[test]
fn cache_clear_removes_cache_dir_contents() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    let cache_dir = dir.path().join(".bmake/cache");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(cache_dir.join("stale.txt"), b"x").unwrap();

    bmake().current_dir(dir.path()).args(["cache", "clear"]).assert().success();

    assert!(!cache_dir.join("stale.txt").exists());
}

// ---------- logs ----------

#[test]
fn logs_reports_no_builds_yet_before_any_run() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    bmake()
        .current_dir(dir.path())
        .arg("logs")
        .assert()
        .success()
        .stdout(predicate::str::contains("No builds logged yet"));
}

#[test]
fn logs_last_shows_output_after_a_run() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    bmake().current_dir(dir.path()).args(["run", "--no-color", "--no-animation"]).assert().success();

    bmake()
        .current_dir(dir.path())
        .args(["logs", "--last"])
        .assert()
        .success()
        .stdout(predicate::str::contains("build-ran"));
}

// ---------- env ----------

#[test]
fn env_runs_without_error() {
    let dir = tempfile::tempdir().unwrap();
    bmake()
        .current_dir(dir.path())
        .arg("env")
        .assert()
        .success()
        .stdout(predicate::str::contains("BMake Environment"));
}

// ---------- import ----------

#[test]
fn run_resolves_imported_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("imported.marker");
    write_bm(
        dir.path(),
        "other.bm",
        &format!("<Version: 1.0>\n\nStart\n\n<Task: FromOther>\n    Command: touch {}\n</Task>\n\nStop", marker.display()),
    );
    write_bm(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nimport = other.bm\n\nStop");

    bmake().current_dir(dir.path()).args(["run", "--no-color", "--no-animation"]).assert().success();
    assert!(marker.exists());
}

#[test]
fn run_reports_missing_import_clearly() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nimport = missing.bm\n\nStop");
    bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Failed to import").or(predicate::str::contains("Failed to read")));
}

// ---------- UI flags ----------

#[test]
fn no_color_flag_produces_output_without_ansi_escapes() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    let output = bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout).to_string();
    assert!(!stdout.contains('\u{1b}'), "output should contain no ANSI escape codes with --no-color");
}

#[test]
fn ci_env_var_suppresses_animation_without_crashing() {
    let dir = tempfile::tempdir().unwrap();
    write_bm(dir.path(), "BMake.bm", SIMPLE_BM);
    bmake().current_dir(dir.path()).env("CI", "true").arg("run").assert().success();
}