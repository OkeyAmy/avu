use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn doctor_fake_reports_capabilities() {
    let mut cmd = Command::cargo_bin("avu").expect("binary exists");
    cmd.args(["doctor", "--backend", "fake", "--json"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("\"approvals\": true"))
        .stdout(predicate::str::contains("\"interrupt\": true"));
}

#[test]
fn status_fixture_shows_pending_approval() {
    let mut cmd = Command::cargo_bin("avu").expect("binary exists");
    cmd.args(["status", "--fixture", "fixtures/approval_destructive.json"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Pending approval"))
        .stdout(predicate::str::contains("rm -rf build-cache"));
}

#[test]
fn tui_fixture_renders_cockpit_layout() {
    let mut cmd = Command::cargo_bin("avu").expect("binary exists");
    cmd.args([
        "tui",
        "--fixture",
        "fixtures/approval_destructive.json",
        "--once",
    ]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("AVU EVENT RADAR"))
        .stdout(predicate::str::contains("ACTIVITY LOG"))
        .stdout(predicate::str::contains("APPROVAL"));
}

#[test]
fn setup_fake_runs_terminal_wizard_checks() {
    let mut cmd = Command::cargo_bin("avu").expect("binary exists");
    cmd.args(["setup", "--backend", "fake", "--non-interactive"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Avu setup"))
        .stdout(predicate::str::contains("Capability discovery"))
        .stdout(predicate::str::contains("Safety rehearsal"));
}

#[test]
fn setup_remote_keeps_capability_and_safety_nonfatal() {
    let mut cmd = Command::cargo_bin("avu").expect("binary exists");
    cmd.args(["setup", "--backend", "remote", "--non-interactive"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains(
            "3. [!!] Backend health verification",
        ))
        .stdout(predicate::str::contains("4. [ok] Capability discovery"))
        .stdout(predicate::str::contains("7. [ok] Safety rehearsal"));
}

#[test]
fn doctor_remote_disables_approval_controls_safely() {
    let mut cmd = Command::cargo_bin("avu").expect("binary exists");
    cmd.args(["doctor", "--backend", "remote"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("[!!] backend_reachable"))
        .stdout(predicate::str::contains(
            "[ok] approval_policy_owned_by_backend",
        ))
        .stdout(predicate::str::contains(
            "approval controls disabled safely",
        ));
}

#[test]
fn hermes_status_does_not_show_fake_approval_when_unavailable() {
    let mut cmd = Command::cargo_bin("avu").expect("binary exists");
    cmd.args(["status", "--backend", "hermes"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Model: unreported"))
        .stdout(predicate::str::contains("Voice: unreported"))
        .stdout(predicate::str::contains("Pending approval").not())
        .stdout(predicate::str::contains("rm -rf build-cache").not())
        .stdout(predicate::str::contains("tool:web.search").not());
}

#[test]
fn openclaw_status_does_not_show_fake_approval_when_unavailable() {
    let mut cmd = Command::cargo_bin("avu").expect("binary exists");
    cmd.args(["status", "--backend", "openclaw"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Model: unreported"))
        .stdout(predicate::str::contains("Voice: unreported"))
        .stdout(predicate::str::contains("Pending approval").not())
        .stdout(predicate::str::contains("rm -rf build-cache").not())
        .stdout(predicate::str::contains("tool:web.search").not());
}

#[test]
fn auto_status_without_backends_does_not_use_fake_fixture_state() {
    let mut cmd = Command::cargo_bin("avu").expect("binary exists");
    cmd.args(["status"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Model: unreported"))
        .stdout(predicate::str::contains("Voice: unreported"))
        .stdout(predicate::str::contains("Pending approval").not())
        .stdout(predicate::str::contains("rm -rf build-cache").not())
        .stdout(predicate::str::contains("tool:web.search").not());
}

#[test]
fn invalid_fixture_fails_with_context() {
    let mut cmd = Command::cargo_bin("avu").expect("binary exists");
    cmd.args(["status", "--fixture", "fixtures/invalid.json"]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("invalid fixture JSON"));
}
