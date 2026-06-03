use assert_cmd::cargo::CommandCargoExt;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const AVU_HOME: &str = "/tmp/avu-kali/avu-home";
const EVIDENCE_DIR: &str = "/tmp/avu-kali/evidence";

#[test]
#[ignore]
fn hermes_telegram_update_roundtrip() {
    let evidence = prepare_evidence_dir();
    capture_command(
        &evidence,
        "hermes-status.txt",
        command("hermes", &["status"]),
    );
    capture_command(
        &evidence,
        "hermes-gateway-status.txt",
        command("hermes", &["gateway", "status"]),
    );
    let prompt = format!(
        "Send a Telegram update saying 'Avu Kali verification ping {}'",
        chrono::Utc::now().to_rfc3339()
    );
    let output = capture_command(
        &evidence,
        "hermes-telegram-prompt.txt",
        command(
            "hermes",
            &["--continue", "avu-live-test", "--oneshot", &prompt],
        ),
    );
    assert_success("hermes telegram prompt", &output);
    capture_command(
        &evidence,
        "avu-status-hermes.txt",
        avu_command(&["status", "--backend", "hermes"]),
    );
}

#[test]
#[ignore]
fn hermes_tts_audio_generation_and_playback_signal() {
    let evidence = prepare_evidence_dir();
    capture_command(
        &evidence,
        "hermes-status.txt",
        command("hermes", &["status"]),
    );
    let output = capture_command(
        &evidence,
        "hermes-tts-output.txt",
        command(
            "hermes",
            &[
                "--continue",
                "avu-live-test",
                "--oneshot",
                "/tts say Avu Kali audio verification",
            ],
        ),
    );
    assert_success("hermes tts prompt", &output);
    capture_command(
        &evidence,
        "avu-status-after-tts.txt",
        avu_command(&["status", "--backend", "hermes"]),
    );
    let audio_files = newest_audio_files();
    fs::write(
        evidence.join("detected-audio-files.txt"),
        audio_files
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("write audio evidence");
    assert!(!audio_files.is_empty(), "no Hermes audio files detected");
}

#[test]
#[ignore]
fn avu_session_log_is_created_during_live_tui_session() {
    let evidence = prepare_evidence_dir();
    let avu_bin = assert_cmd::cargo::cargo_bin("avu");
    let script = r#"
import os, pty, subprocess, time
avu = os.environ['AVU_BIN']
env = os.environ.copy()
env['AVU_HOME'] = os.environ['AVU_HOME_PATH']
pid, fd = pty.fork()
if pid == 0:
    os.execvpe(avu, [avu, 'tui', '--backend', 'fake'], env)
time.sleep(1.0)
os.write(fd, b'q')
_, status = os.waitpid(pid, 0)
raise SystemExit(os.waitstatus_to_exitcode(status))
"#;
    let mut pty_command = command("python3", &["-c", script]);
    pty_command
        .env("AVU_BIN", avu_bin.as_os_str())
        .env("AVU_HOME_PATH", AVU_HOME);
    let output = capture_command(&evidence, "avu-tui-fake-pty.txt", pty_command);
    assert_success("avu fake tui pty", &output);
    let logs = fs::read_dir(Path::new(AVU_HOME).join("logs"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    fs::write(
        evidence.join("avu-log-files.txt"),
        logs.iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("write log evidence");
    assert!(!logs.is_empty(), "fake TUI session did not create Avu logs");
}

fn prepare_evidence_dir() -> PathBuf {
    let evidence = PathBuf::from(EVIDENCE_DIR);
    fs::create_dir_all(&evidence).expect("create evidence dir");
    fs::create_dir_all(AVU_HOME).expect("create avu home");
    evidence
}

fn avu_command(args: &[&str]) -> Command {
    let mut command = Command::cargo_bin("avu").expect("avu binary");
    command.args(args).env("AVU_HOME", AVU_HOME);
    command
}

fn command(program: &str, args: &[&str]) -> Command {
    let mut command = Command::new(program);
    command.args(args).env("AVU_HOME", AVU_HOME);
    command
}

fn capture_command(evidence: &Path, name: &str, mut command: Command) -> Output {
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            fs::write(
                evidence.join(name),
                format!("--- command error ---\nfailed to run command for {name}: {error}\n"),
            )
            .expect("write command error evidence");
            panic!("failed to run command for {name}: {error}");
        }
    };
    let mut text = String::new();
    text.push_str("--- stdout ---\n");
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str("\n--- stderr ---\n");
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text.push_str(&format!("\n--- status ---\n{}\n", output.status));
    fs::write(evidence.join(name), text).expect("write evidence");
    output
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn newest_audio_files() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let roots = [
        home.join(".hermes/audio_cache"),
        home.join(".hermes/cache/audio"),
    ];
    roots
        .into_iter()
        .filter(|root| root.exists())
        .flat_map(audio_files_under)
        .collect()
}

fn audio_files_under(root: PathBuf) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .map(|extension| matches!(extension, "mp3" | "wav" | "ogg" | "m4a"))
                .unwrap_or(false)
        })
        .collect()
}
