use crate::{
    cli::BackendChoice,
    domain::{CapabilitySnapshot, CockpitState},
    runtime,
};
use anyhow::{Context, Result};
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
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

pub fn quick_cockpit_state(choice: BackendChoice) -> CockpitState {
    match choice {
        BackendChoice::Fake => FakeBackend.cockpit_state(),
        BackendChoice::Hermes => {
            CockpitState::from_runtime(runtime::hermes_quick_snapshot(command_exists("hermes")))
        }
        BackendChoice::Openclaw => {
            CockpitState::from_runtime(runtime::openclaw_quick_snapshot(command_exists("openclaw")))
        }
        BackendChoice::Remote => {
            CockpitState::from_runtime(runtime::RuntimeSnapshot::missing("backend"))
        }
        BackendChoice::Auto => {
            if command_exists("hermes") {
                CockpitState::from_runtime(runtime::hermes_quick_snapshot(true))
            } else if command_exists("openclaw") {
                CockpitState::from_runtime(runtime::openclaw_quick_snapshot(true))
            } else {
                CockpitState::from_runtime(runtime::RuntimeSnapshot::missing("backend"))
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
        "hermes" => run_command_prompt(
            command,
            &["--continue", "avu-tui", "--oneshot", prompt],
            before_audio.as_ref(),
        ),
        other => run_command_prompt(other, &[prompt], before_audio.as_ref()),
    };

    let after_audio = latest_backend_audio(command);
    let new_audio = result
        .audio_path
        .clone()
        .or_else(|| newer_audio(before_audio.as_ref(), after_audio.as_ref()).cloned());
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

pub fn run_backend_cli_command(choice: BackendChoice, command_text: &str) -> BackendCommandResult {
    let command_text = command_text.trim().trim_start_matches('/').trim();
    if command_text.is_empty() {
        return BackendCommandResult::failed("empty backend command");
    }
    let backend = adapter_for(choice);
    let label = backend.label();
    if !matches!(label, "hermes" | "openclaw") {
        return BackendCommandResult::failed("backend CLI command requires Hermes or OpenClaw");
    }
    if !command_exists(label) {
        return BackendCommandResult::failed(format!("{label} command was not found on PATH"));
    }
    let args = match split_command_args(command_text) {
        Ok(args) => args,
        Err(error) => return BackendCommandResult::failed(error),
    };
    if args.is_empty() {
        return BackendCommandResult::failed("empty backend command");
    }
    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    run_command_prompt(label, &arg_refs, None)
}

fn split_command_args(input: &str) -> std::result::Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escape = false;
    for ch in input.chars() {
        if escape {
            current.push(ch);
            escape = false;
            continue;
        }
        match ch {
            '\\' => escape = true,
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            ch if ch.is_whitespace() && quote.is_none() => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            ch => current.push(ch),
        }
    }
    if escape {
        current.push('\\');
    }
    if quote.is_some() {
        return Err("unterminated quote in backend command".to_string());
    }
    if !current.is_empty() {
        args.push(current);
    }
    Ok(args)
}

pub fn run_voice_session(choice: BackendChoice) -> BackendCommandResult {
    match adapter_for(choice).label() {
        "hermes" => run_interactive_backend("hermes", &["--continue", "avu-tui", "--tui"]),
        "openclaw" => {
            BackendCommandResult::failed("OpenClaw voice handoff is not available through Avu yet")
        }
        _ => BackendCommandResult::failed("voice requires a live Hermes backend on PATH"),
    }
}

fn run_interactive_backend(command: &str, args: &[&str]) -> BackendCommandResult {
    if !command_exists(command) {
        return BackendCommandResult::failed(format!("{command} command was not found on PATH"));
    }

    match Command::new(command).args(args).status() {
        Ok(status) if status.success() => BackendCommandResult::ok("voice session ended"),
        Ok(status) => BackendCommandResult::failed(format!("voice session exited with {status}")),
        Err(error) => {
            BackendCommandResult::failed(format!("failed to start voice session: {error}"))
        }
    }
}

fn run_command_prompt(
    command: &str,
    args: &[&str],
    _before_audio: Option<&AudioCandidate>,
) -> BackendCommandResult {
    if !command_exists(command) {
        return BackendCommandResult::failed(format!("{command} command was not found on PATH"));
    }

    let output = match Command::new(command)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            return BackendCommandResult::failed(format!("failed to run {command}: {error}"));
        }
    };

    summarize_output(output)
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
    if cfg!(target_os = "linux")
        && !linux_has_session_audio_sink()
        && let Some(result) = play_audio_with_alsa_hardware(path)
    {
        return Some(result);
    }

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
            ("xdg-open", &[]),
        ]
    };

    for (program, args) in candidates {
        if !command_exists(program) {
            continue;
        }
        let mut command = Command::new(program);
        command
            .args(*args)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match command.spawn() {
            Ok(mut child) => {
                let player = (*program).to_string();
                thread::spawn(move || {
                    let _ = child.wait();
                });
                return Some(format!("started with {player}"));
            }
            Err(_) => continue,
        }
    }
    if cfg!(target_os = "linux") {
        play_audio_with_alsa_hardware(path)
    } else {
        None
    }
}

fn linux_has_session_audio_sink() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    command_stdout("pactl", &["list", "short", "sinks"])
        .map(|text| text.lines().any(|line| !line.trim().is_empty()))
        .unwrap_or(false)
        || command_stdout("wpctl", &["status"])
            .map(|text| {
                text.lines()
                    .any(|line| line.trim_start().starts_with("Sinks:"))
            })
            .unwrap_or(false)
}

fn play_audio_with_alsa_hardware(path: &Path) -> Option<String> {
    if !command_exists("aplay") {
        return None;
    }
    let playable_path = alsa_playable_path(path)?;
    for device in alsa_hardware_devices() {
        if spawn_checked_audio_player(
            "aplay",
            &[
                OsString::from("-D"),
                OsString::from(device.clone()),
                playable_path.clone().into(),
            ],
        ) {
            return Some(format!("started with aplay {device}"));
        }
    }
    None
}

fn alsa_playable_path(path: &Path) -> Option<PathBuf> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if extension == "wav" {
        return Some(path.to_path_buf());
    }
    if !command_exists("ffmpeg") {
        return None;
    }
    let mut target = env::temp_dir();
    target.push(format!(
        "avu-audio-{}-{}.wav",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let status = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-ar", "48000", "-ac", "2"])
        .arg(&target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    status.success().then_some(target)
}

fn alsa_hardware_devices() -> Vec<String> {
    command_stdout("aplay", &["-l"])
        .map(|text| parse_alsa_hardware_devices(&text))
        .unwrap_or_default()
}

fn parse_alsa_hardware_devices(text: &str) -> Vec<String> {
    let mut devices = Vec::new();
    for line in text.lines() {
        let Some(card_part) = line.trim_start().strip_prefix("card ") else {
            continue;
        };
        let Some((card, rest)) = card_part.split_once(':') else {
            continue;
        };
        let Some(device_part) = rest.split("device ").nth(1) else {
            continue;
        };
        let Some((device, _)) = device_part.split_once(':') else {
            continue;
        };
        devices.push(format!("hw:{},{}", card.trim(), device.trim()));
    }
    devices
}

fn spawn_checked_audio_player(program: &str, args: &[OsString]) -> bool {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    thread::sleep(Duration::from_millis(300));
    match child.try_wait() {
        Ok(Some(status)) => status.success(),
        Ok(None) => {
            thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    if !command_exists(program) {
        return None;
    }
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

fn summarize_output(output: std::process::Output) -> BackendCommandResult {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines: Vec<&str> = stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let audio_path = lines.iter().find_map(|line| media_path_from_line(line));
    let summary = lines
        .iter()
        .copied()
        .find(|line| !line.starts_with("MEDIA:") && *line != "[[audio_as_voice]]")
        .or_else(|| audio_path.as_ref().map(|_| "backend generated audio"))
        .unwrap_or(if output.status.success() {
            "backend command completed"
        } else {
            "backend command failed without output"
        })
        .to_string();

    if output.status.success() {
        BackendCommandResult::ok(summary).with_audio(audio_path)
    } else {
        BackendCommandResult::failed(summary).with_audio(audio_path)
    }
}

fn media_path_from_line(line: &str) -> Option<PathBuf> {
    line.strip_prefix("MEDIA:")
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
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

    #[test]
    fn summarize_output_extracts_backend_media_path() {
        let output = std::process::Output {
            status: success_status(),
            stdout: b"[[audio_as_voice]]\nMEDIA:/tmp/avu-voice.ogg\n".to_vec(),
            stderr: vec![],
        };

        let result = summarize_output(output);

        assert!(result.ok);
        assert_eq!(result.summary, "backend generated audio");
        assert_eq!(result.audio_path, Some(PathBuf::from("/tmp/avu-voice.ogg")));
    }

    #[test]
    fn parses_alsa_hardware_devices_from_aplay_output() {
        let text = "**** List of PLAYBACK Hardware Devices ****\ncard 0: I82801AAICH [Intel 82801AA-ICH], device 0: Intel ICH [Intel ICH]\n  Subdevices: 1/1\ncard 2: USB [USB Audio], device 3: Speaker [USB Speaker]\n";

        let devices = parse_alsa_hardware_devices(text);

        assert_eq!(devices, vec!["hw:0,0", "hw:2,3"]);
    }

    #[cfg(unix)]
    fn success_status() -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(0)
    }

    #[cfg(windows)]
    fn success_status() -> std::process::ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(0)
    }
}
