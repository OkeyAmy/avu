use crate::{
    cli::BackendChoice,
    domain::{CapabilitySnapshot, CockpitState},
    runtime,
};
use anyhow::{Context, Result};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, SystemTime},
};

pub trait BackendAdapter {
    fn label(&self) -> &'static str;
    fn probe(&self) -> CapabilitySnapshot;
    fn cockpit_state(&self) -> CockpitState;
    fn route_prompt(&self, prompt: &str) -> BackendCommandResult;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BackendCommandResult {
    pub ok: bool,
    pub summary: String,
    pub audio_path: Option<PathBuf>,
}

impl BackendCommandResult {
    fn ok(summary: impl Into<String>) -> Self {
        Self {
            ok: true,
            summary: summary.into(),
            audio_path: None,
        }
    }

    fn failed(summary: impl Into<String>) -> Self {
        Self {
            ok: false,
            summary: summary.into(),
            audio_path: None,
        }
    }

    fn with_audio(mut self, audio_path: Option<PathBuf>) -> Self {
        self.audio_path = audio_path;
        self
    }
}

pub struct FakeBackend;
pub struct HermesBackend;
pub struct OpenClawBackend;
pub struct MissingBackend;

impl BackendAdapter for FakeBackend {
    fn label(&self) -> &'static str {
        "fake"
    }

    fn probe(&self) -> CapabilitySnapshot {
        CapabilitySnapshot::fake()
    }

    fn cockpit_state(&self) -> CockpitState {
        CockpitState::fake_listening()
    }

    fn route_prompt(&self, prompt: &str) -> BackendCommandResult {
        BackendCommandResult::ok(format!("fixture backend received: {prompt}"))
    }
}

impl BackendAdapter for HermesBackend {
    fn label(&self) -> &'static str {
        "hermes"
    }

    fn probe(&self) -> CapabilitySnapshot {
        if !command_exists("hermes") {
            return CapabilitySnapshot::missing("hermes");
        }
        CapabilitySnapshot::from_runtime(&runtime::hermes_snapshot(true))
    }

    fn cockpit_state(&self) -> CockpitState {
        CockpitState::from_runtime(runtime::hermes_snapshot(command_exists("hermes")))
    }

    fn route_prompt(&self, prompt: &str) -> BackendCommandResult {
        run_backend_prompt("hermes", prompt)
    }
}

impl BackendAdapter for OpenClawBackend {
    fn label(&self) -> &'static str {
        "openclaw"
    }

    fn probe(&self) -> CapabilitySnapshot {
        if !command_exists("openclaw") {
            return CapabilitySnapshot::missing("openclaw");
        }
        CapabilitySnapshot::from_runtime(&runtime::openclaw_snapshot(true))
    }

    fn cockpit_state(&self) -> CockpitState {
        CockpitState::from_runtime(runtime::openclaw_snapshot(command_exists("openclaw")))
    }

    fn route_prompt(&self, prompt: &str) -> BackendCommandResult {
        run_command_prompt("openclaw", &["agent", "--message", prompt], None)
    }
}

impl BackendAdapter for MissingBackend {
    fn label(&self) -> &'static str {
        "missing"
    }

    fn probe(&self) -> CapabilitySnapshot {
        CapabilitySnapshot::from_runtime(&runtime::RuntimeSnapshot::missing("backend"))
    }

    fn cockpit_state(&self) -> CockpitState {
        CockpitState::from_runtime(runtime::RuntimeSnapshot::missing("backend"))
    }

    fn route_prompt(&self, _prompt: &str) -> BackendCommandResult {
        BackendCommandResult::failed("backend command was not found on PATH")
    }
}

pub fn adapter_for(choice: BackendChoice) -> Box<dyn BackendAdapter> {
    match choice {
        BackendChoice::Fake => Box::new(FakeBackend),
        BackendChoice::Hermes => Box::new(HermesBackend),
        BackendChoice::Openclaw => Box::new(OpenClawBackend),
        BackendChoice::Remote => Box::new(MissingBackend),
        BackendChoice::Auto => {
            if command_exists("hermes") {
                Box::new(HermesBackend)
            } else if command_exists("openclaw") {
                Box::new(OpenClawBackend)
            } else {
                Box::new(MissingBackend)
            }
        }
    }
}

pub fn load_fixture(path: &Path) -> Result<CockpitState> {
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read fixture {}", path.display()))?;
    serde_json::from_str(&data).with_context(|| format!("invalid fixture JSON {}", path.display()))
}

pub fn command_exists(command: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| {
            env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(command);
                candidate.is_file() || (cfg!(windows) && candidate.with_extension("exe").is_file())
            })
        })
        .unwrap_or(false)
}

fn run_backend_prompt(command: &str, prompt: &str) -> BackendCommandResult {
    if !command_exists(command) {
        return BackendCommandResult::failed(format!("{command} command was not found on PATH"));
    }

    let before_audio = latest_backend_audio(command);

    let result = match command {
        "hermes" => run_command_prompt(command, &["--oneshot", prompt], before_audio.as_ref()),
        other => run_command_prompt(other, &[prompt], before_audio.as_ref()),
    };

    let after_audio = latest_backend_audio(command);
    let new_audio = newer_audio(before_audio.as_ref(), after_audio.as_ref()).cloned();
    let played = new_audio.as_ref().and_then(|path| play_audio(path));
    let result = result.with_audio(new_audio.clone());

    if let Some(audio_path) = new_audio {
        let playback = played.unwrap_or_else(|| "no audio player found".to_string());
        BackendCommandResult {
            summary: format!(
                "{} · audio: {} ({playback})",
                result.summary,
                audio_path.display()
            ),
            ..result
        }
    } else {
        result
    }
}

pub fn run_shell_command(command: &str) -> BackendCommandResult {
    let command = command.trim();
    if command.is_empty() {
        return BackendCommandResult::failed("empty shell command");
    }

    let mut process = if cfg!(windows) {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", command]);
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.args(["-lc", command]);
        cmd
    };

    let output = match process
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(output) => output,
        Err(error) => return BackendCommandResult::failed(format!("shell failed: {error}")),
    };

    summarize_output(output)
}

fn run_command_prompt(
    command: &str,
    args: &[&str],
    _before_audio: Option<&AudioCandidate>,
) -> BackendCommandResult {
    if !command_exists(command) {
        return BackendCommandResult::failed(format!("{command} command was not found on PATH"));
    }

    let mut child = match Command::new(command)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return BackendCommandResult::failed(format!("failed to start {command}: {error}"));
        }
    };

    for _ in 0..90 {
        match child.try_wait() {
            Ok(Some(_)) => match child.wait_with_output() {
                Ok(output) => return summarize_output(output),
                Err(error) => {
                    return BackendCommandResult::failed(format!(
                        "failed to read {command} output: {error}"
                    ));
                }
            },
            Ok(None) => thread::sleep(Duration::from_millis(500)),
            Err(error) => {
                return BackendCommandResult::failed(format!(
                    "failed while waiting for {command}: {error}"
                ));
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    BackendCommandResult::failed(format!("{command} prompt timed out after 45s"))
}

#[derive(Debug, Clone)]
struct AudioCandidate {
    path: PathBuf,
    modified: SystemTime,
}

fn latest_backend_audio(command: &str) -> Option<AudioCandidate> {
    let mut roots = Vec::new();
    if command == "hermes" {
        if let Some(root) = env::var_os("HERMES_AUDIO_CACHE_DIR") {
            roots.push(PathBuf::from(root));
        }
        if let Some(home) = env::var_os("HOME") {
            let home = PathBuf::from(home);
            roots.push(home.join(".hermes/audio_cache"));
            roots.push(home.join(".hermes/cache/audio"));
        }
    } else if command == "openclaw" {
        if let Some(root) = env::var_os("OPENCLAW_STATE_DIR") {
            roots.push(PathBuf::from(root).join("media"));
        }
        if let Some(home) = env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".openclaw/media"));
        }
    }

    roots
        .into_iter()
        .filter(|root| root.exists())
        .flat_map(audio_files_under)
        .max_by_key(|candidate| candidate.modified)
}

fn audio_files_under(root: PathBuf) -> Vec<AudioCandidate> {
    let mut files = Vec::new();
    collect_audio_files(&root, &mut files, 0);
    files
}

fn collect_audio_files(path: &Path, files: &mut Vec<AudioCandidate>, depth: u8) {
    if depth > 3 {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_audio_files(&path, files, depth + 1);
            continue;
        }
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if !matches!(
            extension.to_ascii_lowercase().as_str(),
            "mp3" | "wav" | "ogg" | "m4a"
        ) {
            continue;
        }
        if let Ok(metadata) = entry.metadata()
            && let Ok(modified) = metadata.modified()
        {
            files.push(AudioCandidate { path, modified });
        }
    }
}

fn newer_audio<'a>(
    before: Option<&AudioCandidate>,
    after: Option<&'a AudioCandidate>,
) -> Option<&'a PathBuf> {
    let after = after?;
    if before.is_none_or(|before| after.modified > before.modified) {
        Some(&after.path)
    } else {
        None
    }
}

fn play_audio(path: &Path) -> Option<String> {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("afplay", &[])]
    } else if cfg!(windows) {
        &[(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "(New-Object Media.SoundPlayer $args[0]).PlaySync()",
            ],
        )]
    } else {
        &[
            ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
            ("mpv", &["--really-quiet"]),
            ("paplay", &[]),
            ("aplay", &[]),
            ("xdg-open", &[]),
        ]
    };

    for (program, args) in candidates {
        if !command_exists(program) {
            continue;
        }
        let mut command = Command::new(program);
        command.args(*args).arg(path);
        match command.status() {
            Ok(status) if status.success() => return Some(format!("played with {program}")),
            Ok(_) => continue,
            Err(_) => continue,
        }
    }
    None
}

fn summarize_output(output: std::process::Output) -> BackendCommandResult {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let summary = stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(if output.status.success() {
            "backend command completed"
        } else {
            "backend command failed without output"
        })
        .to_string();

    if output.status.success() {
        BackendCommandResult::ok(summary)
    } else {
        BackendCommandResult::failed(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_backend_exposes_required_capabilities() {
        let probe = FakeBackend.probe();
        assert!(probe.reachable);
        assert!(probe.events);
        assert!(probe.approvals);
        assert!(probe.interrupt);
    }

    #[test]
    fn fake_backend_routes_prompts_for_tui_tests() {
        let result = FakeBackend.route_prompt("/voice");
        assert!(result.ok);
        assert_eq!(result.summary, "fixture backend received: /voice");
    }

    #[test]
    fn missing_backend_rejects_prompt_routing() {
        let result = MissingBackend.route_prompt("/voice");
        assert!(!result.ok);
        assert!(result.summary.contains("not found"));
    }

    #[test]
    fn shell_command_returns_output_for_cmd_mode() {
        let result = if cfg!(windows) {
            run_shell_command("echo avu-cmd")
        } else {
            run_shell_command("printf avu-cmd")
        };
        assert!(result.ok);
        assert_eq!(result.summary, "avu-cmd");
    }
}
