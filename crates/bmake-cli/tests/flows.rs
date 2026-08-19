use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;

fn bmake() -> Command {
    Command::cargo_bin("bmake").unwrap()
}

fn write(dir: &Path, rel: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, content).unwrap();
    path
}

fn write_flow(dir: &Path, flow_path: &str, content: &str) {
    write(dir, &format!(".bmake/flows/{}.bm", flow_path), content);
}

// ---------- basic Uses ----------

#[test]
fn uses_runs_flow_command() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("built.marker");
    write_flow(
        dir.path(),
        "android/build",
        &format!("Name: Android Build\nDescription: Build it\n\nCommand: touch {}", marker.display()),
    );
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nUses: android/build\n\nStop");

    bmake().current_dir(dir.path()).args(["run", "--no-color", "--no-animation"]).assert().success();
    assert!(marker.exists());
}

#[test]
fn uses_nested_flow_path_resolves() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("built.marker");
    write_flow(
        dir.path(),
        "android/release/build",
        &format!("Name: Android Release\nDescription: Build release\n\nCommand: touch {}", marker.display()),
    );
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nUses: android/release/build\n\nStop");

    bmake().current_dir(dir.path()).args(["run", "--no-color", "--no-animation"]).assert().success();
    assert!(marker.exists());
}

#[test]
fn uses_missing_flow_errors_clearly() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nUses: android/build\n\nStop");
    bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Plugin Flow not found"));
}

#[test]
fn uses_flow_missing_name_errors() {
    let dir = tempfile::tempdir().unwrap();
    write_flow(dir.path(), "android/build", "Description: Build it\n\nCommand: echo hi");
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nUses: android/build\n\nStop");
    bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Missing required metadata"))
        .stderr(predicate::str::contains("Name:"));
}

#[test]
fn uses_flow_missing_description_errors() {
    let dir = tempfile::tempdir().unwrap();
    write_flow(dir.path(), "android/build", "Name: Android Build\n\nCommand: echo hi");
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nUses: android/build\n\nStop");
    bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Description:"));
}

#[test]
fn uses_path_traversal_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nUses: ../../secret\n\nStop");
    bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("path traversal"));
}

#[test]
fn uses_absolute_path_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nUses: /etc/passwd\n\nStop");
    bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("absolute paths"));
}

// ---------- Plugin Flow + Task/Depends-on ----------

#[test]
fn task_can_depend_on_a_flow() {
    let dir = tempfile::tempdir().unwrap();
    let prepare_marker = dir.path().join("prepare.marker");
    let test_marker = dir.path().join("test.marker");
    write_flow(
        dir.path(),
        "android/prepare",
        &format!("Name: Prepare\nDescription: Prepare env\n\nCommand: touch {}", prepare_marker.display()),
    );
    write(
        dir.path(),
        "BMake.bm",
        &format!(
            "<Version: 1.0>\n\nStart\n\nUses: android/prepare\n\n<Task: Test>\n    Depends-on: android/prepare\n    Command: touch {}\n</Task>\n\nStop",
            test_marker.display()
        ),
    );

    bmake().current_dir(dir.path()).args(["run", "--no-color", "--no-animation"]).assert().success();
    assert!(prepare_marker.exists());
    assert!(test_marker.exists());
}

#[test]
fn flow_and_task_name_collision_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    write_flow(dir.path(), "android/build", "Name: X\nDescription: Y\n\nCommand: echo hi");
    write(
        dir.path(),
        "BMake.bm",
        "<Version: 1.0>\n\nStart\n\nUses: android/build\n\n<Task: android/build>\n    Command: echo dup\n</Task>\n\nStop",
    );
    bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Duplicate Task/Plugin Flow"));
}

// ---------- Value / Secret ----------

#[test]
fn flow_resolves_value_reference() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("out-Release.marker");
    write_flow(
        dir.path(),
        "android/build",
        "Name: Android Build\nDescription: Build it\n\nCommand: touch out-${{ Value.Build.Type }}.marker",
    );
    write(
        dir.path(),
        "BMake.bm",
        "<Version: 1.0>\n\nStart\n\nValue:\n    Build:\n        Type: Release\n\nUses: android/build\n\nStop",
    );

    bmake().current_dir(dir.path()).args(["run", "--no-color", "--no-animation"]).assert().success();
    assert!(marker.exists());
}

#[test]
fn flow_resolves_secret_from_local_vault() {
    let dir = tempfile::tempdir().unwrap();
    let vault = bmake_engine::vault::Vault::at(dir.path());
    vault.create("test-pass").unwrap();
    vault.add_secret("test-pass", "DeployToken", "topsecretvalue").unwrap();

    write_flow(
        dir.path(),
        "android/sign",
        "Name: Sign\nDescription: Sign it\n\nEnv: TOKEN=${{ Secret.DeployToken }}\n\nCommand: echo done",
    );
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nSecret: DeployToken\n\nUses: android/sign\n\nStop");

    bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .write_stdin("test-pass\n")
        .assert()
        .success();
}

// ---------- Retry / Timeout / ContinueOnError ----------

#[test]
fn flow_retries_command_until_success() {
    let dir = tempfile::tempdir().unwrap();
    let counter = dir.path().join("attempts");
    fs::write(&counter, "0").unwrap();
    let script = dir.path().join("flaky.sh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nn=$(cat {0})\nn=$((n+1))\necho $n > {0}\nif [ \"$n\" -lt 3 ]; then exit 1; fi\nexit 0\n",
            counter.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
    }

    write_flow(
        dir.path(),
        "flaky/build",
        &format!("Name: Flaky\nDescription: Flaky build\n\nCommand: sh {}\nOnError: retry\nRetry: 5", script.display()),
    );
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nUses: flaky/build\n\nStop");

    bmake().current_dir(dir.path()).args(["run", "--no-color", "--no-animation"]).assert().success();
    let attempts: u32 = fs::read_to_string(&counter).unwrap().trim().parse().unwrap();
    assert_eq!(attempts, 3);
}

#[test]
fn flow_timeout_reports_timeout_status_in_summary() {
    let dir = tempfile::tempdir().unwrap();
    write_flow(dir.path(), "slow/build", "Name: Slow\nDescription: Too slow\n\nCommand: sleep 5\nTimeout: 1");
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nUses: slow/build\n\nStop");

    bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("TIMEOUT"));
}

#[test]
fn flow_continue_on_error_lets_dependents_run() {
    let dir = tempfile::tempdir().unwrap();
    let after_marker = dir.path().join("after.marker");
    write_flow(dir.path(), "flaky/build", "Name: Flaky\nDescription: Fails\n\nCommand: exit 1\nContinueOnError: true");
    write(
        dir.path(),
        "BMake.bm",
        &format!(
            "<Version: 1.0>\n\nStart\n\nUses: flaky/build\n\n<Task: After>\n    Depends-on: flaky/build\n    Command: touch {}\n</Task>\n\nStop",
            after_marker.display()
        ),
    );

    bmake().current_dir(dir.path()).args(["run", "--no-color", "--no-animation"]).assert().failure();
    assert!(after_marker.exists(), "After should still run because the flow set ContinueOnError: true");
}

// ---------- Output verification ----------

#[test]
fn flow_output_verification_passes_when_output_exists() {
    let dir = tempfile::tempdir().unwrap();
    write_flow(
        dir.path(),
        "android/build",
        "Name: Android Build\nDescription: Build it\n\nCommand: mkdir -p build && touch build/app.apk\nOutput: build/app.apk",
    );
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nUses: android/build\n\nStop");

    bmake().current_dir(dir.path()).args(["run", "--no-color", "--no-animation"]).assert().success();
}

#[test]
fn flow_output_verification_fails_when_output_missing() {
    let dir = tempfile::tempdir().unwrap();
    write_flow(
        dir.path(),
        "android/build",
        "Name: Android Build\nDescription: Build it\n\nCommand: echo no output\nOutput: build/app.apk",
    );
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nUses: android/build\n\nStop");

    bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("declared output was not produced"));
}

// ---------- parallel / failure / summary ----------

#[test]
fn independent_flows_run_in_parallel_wave() {
    let dir = tempfile::tempdir().unwrap();
    let m1 = dir.path().join("a.marker");
    let m2 = dir.path().join("b.marker");
    write_flow(dir.path(), "a/build", &format!("Name: A\nDescription: A\n\nCommand: touch {}", m1.display()));
    write_flow(dir.path(), "b/build", &format!("Name: B\nDescription: B\n\nCommand: touch {}", m2.display()));
    write(
        dir.path(),
        "BMake.bm",
        "<Version: 1.0>\n\nStart\n\nParallel: true\n\nUses: a/build\n\nUses: b/build\n\nStop",
    );

    bmake().current_dir(dir.path()).args(["run", "--no-color", "--no-animation"]).assert().success();
    assert!(m1.exists());
    assert!(m2.exists());
}

#[test]
fn flow_failure_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    write_flow(dir.path(), "android/build", "Name: Android Build\nDescription: Build it\n\nCommand: exit 1");
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nUses: android/build\n\nStop");

    bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("Android Build"));
}

#[test]
fn build_summary_lists_flow_by_path_name() {
    let dir = tempfile::tempdir().unwrap();
    write_flow(dir.path(), "android/build", "Name: Android Build\nDescription: Build it\n\nCommand: echo ok");
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nUses: android/build\n\nStop");

    bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .assert()
        .success()
        .stdout(predicate::str::contains("android/build : SUCCESS"));
}

// ---------- CLI discovery / bmake syntax ----------

#[test]
fn list_flows_shows_metadata() {
    let dir = tempfile::tempdir().unwrap();
    write_flow(dir.path(), "android/build", "Name: Android Build\nDescription: Build application\n\nCommand: echo hi");
    write_flow(dir.path(), "android/test", "Name: Android Test\nDescription: Run tests\n\nCommand: echo test");

    bmake()
        .current_dir(dir.path())
        .args(["list", "flows"])
        .assert()
        .success()
        .stdout(predicate::str::contains("android/build"))
        .stdout(predicate::str::contains("Android Build"))
        .stdout(predicate::str::contains("android/test"));
}

#[test]
fn list_flows_reports_empty_when_none_found() {
    let dir = tempfile::tempdir().unwrap();
    bmake()
        .current_dir(dir.path())
        .args(["list", "flows"])
        .assert()
        .success()
        .stdout(predicate::str::contains("none found"));
}

#[test]
fn syntax_reference_documents_uses() {
    let dir = tempfile::tempdir().unwrap();
    bmake().current_dir(dir.path()).arg("syntax").assert().success().stdout(predicate::str::contains("Uses:"));
}

// ---------- Run-bm ----------

#[test]
fn run_bm_executes_referenced_file() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("deployed.marker");
    write(
        dir.path(),
        "deploy.bm",
        &format!(
            "<Version: 1.0>\n\nStart\n\n<Task: Deploy>\n    Command: touch {}\n</Task>\n\nStop",
            marker.display()
        ),
    );
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nRun-bm: deploy.bm\n\nStop");

    bmake().current_dir(dir.path()).args(["run", "--no-color", "--no-animation"]).assert().success();
    assert!(marker.exists());
}

#[test]
fn run_bm_missing_file_errors_clearly() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "BMake.bm", "<Version: 1.0>\n\nStart\n\nRun-bm: deploy.bm\n\nStop");
    bmake()
        .current_dir(dir.path())
        .args(["run", "--no-color", "--no-animation"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Run-bm file not found"));
}

#[test]
fn run_bm_detects_circular_reference() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.bm", "<Version: 1.0>\n\nStart\n\nRun-bm: b.bm\n\nStop");
    write(dir.path(), "b.bm", "<Version: 1.0>\n\nStart\n\nRun-bm: a.bm\n\nStop");

    // What matters here is that the recursive chain terminates instead of
    // hanging forever — the circular error surfaces from whichever process
    // in the chain detects it first.
    bmake().current_dir(dir.path()).args(["run", "a.bm", "--no-color", "--no-animation"]).assert().failure();
}