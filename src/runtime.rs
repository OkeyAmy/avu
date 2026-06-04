use crate::{
    backend_events::{classify_backend_event, event_kind_for_class},
    domain::{BackendKind, CockpitEvent, EventKind, PermissionPosture},
    gateway,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::Value;
use std::{env, ffi::OsString, fs, path::PathBuf, process::Command};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reported<T> {
    Value(T),
    Unreported,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub backend: BackendKind,
    pub backend_label: String,
    pub reachable: bool,
    pub model_label: Reported<String>,
    pub voice_label: Reported<String>,
    pub events: Reported<bool>,
    pub approvals: Reported<bool>,
    pub interrupt: Reported<bool>,
    pub pause_resume: Reported<bool>,
    pub sessions_list: Reported<bool>,
    pub transcript: Reported<bool>,
    pub wake_available: Reported<bool>,
    pub permission_posture: PermissionPosture,
    pub log_events: Vec<CockpitEvent>,
    pub notes: Vec<String>,
}

impl RuntimeSnapshot {
    pub fn missing(command: &str) -> Self {
        Self {
            backend: BackendKind::Missing,
            backend_label: command.to_uppercase(),
            reachable: false,
            model_label: Reported::Unreported,
            voice_label: Reported::Unreported,
            events: Reported::Unavailable,
            approvals: Reported::Unavailable,
            interrupt: Reported::Unavailable,
            pause_resume: Reported::Unavailable,
            sessions_list: Reported::Unavailable,
            transcript: Reported::Unavailable,
            wake_available: Reported::Unreported,
            permission_posture: PermissionPosture::ObserveOnly,
            log_events: vec![],
            notes: vec![format!("{command} command was not found on PATH")],
        }
    }
}

pub fn hermes_snapshot(command_exists: bool) -> RuntimeSnapshot {
    if !command_exists {
        return RuntimeSnapshot::missing("hermes");
    }

    let status = command_json("hermes", &["status", "--json"]);
    let status_text = command_text("hermes", &["status"]);
    let gateway_text = command_text("hermes", &["gateway", "status"]);
    let configured_model = command_text("hermes", &["config", "get", "model"]);
    let reachable = status.is_some() || status_text.is_some();
    let log_events = read_hermes_log_events();
    let config_text = hermes_config_text();
    let mut notes =
        vec!["Hermes command detected; events sourced from hermes logs agent".to_string()];
    if status.is_none() {
        notes.push(
            "Hermes status JSON unavailable; capability detection is degraded until Hermes exposes machine-readable status".to_string(),
        );
    }
    notes.extend(hermes_gateway_notes(
        gateway_text.as_deref(),
        hermes_default_profile(config_text.as_deref()).as_deref(),
    ));

    RuntimeSnapshot {
        backend: BackendKind::Hermes,
        backend_label: "HERMES".to_string(),
        reachable,
        model_label: model_from_backend(
            &status,
            status_text.as_deref(),
            configured_model.as_deref(),
            &log_events,
        ),
        voice_label: voice_from_hermes_config(config_text.as_deref()),
        events: json_bool(&status, &["events", "capabilities.events"]),
        approvals: json_bool(&status, &["approvals", "capabilities.approvals"]),
        interrupt: json_bool(&status, &["interrupt", "capabilities.interrupt"]),
        pause_resume: json_bool(&status, &["pause_resume", "capabilities.pause_resume"]),
        sessions_list: json_bool(&status, &["sessions", "capabilities.sessions_list"]),
        transcript: json_bool(&status, &["transcript", "capabilities.transcript"]),
        wake_available: Reported::Unreported,
        permission_posture: PermissionPosture::ObserveNotify,
        log_events,
        notes,
    }
}

pub fn hermes_quick_snapshot(command_exists: bool) -> RuntimeSnapshot {
    if !command_exists {
        return RuntimeSnapshot::missing("hermes");
    }

    let config_text = hermes_config_text();
    RuntimeSnapshot {
        backend: BackendKind::Hermes,
        backend_label: "HERMES".to_string(),
        reachable: true,
        model_label: model_from_hermes_config(config_text.as_deref()),
        voice_label: voice_from_hermes_config(config_text.as_deref()),
        events: Reported::Unreported,
        approvals: Reported::Unreported,
        interrupt: Reported::Unreported,
        pause_resume: Reported::Unreported,
        sessions_list: Reported::Unreported,
        transcript: Reported::Unreported,
        wake_available: Reported::Unreported,
        permission_posture: PermissionPosture::ObserveNotify,
        log_events: vec![],
        notes: vec![
            "Hermes command detected; live status refresh is running in the background".to_string(),
        ],
    }
}

pub fn openclaw_quick_snapshot(command_exists: bool) -> RuntimeSnapshot {
    if !command_exists {
        return RuntimeSnapshot::missing("openclaw");
    }

    RuntimeSnapshot {
        backend: BackendKind::OpenClaw,
        backend_label: "OPENCLAW".to_string(),
        reachable: true,
        model_label: Reported::Unreported,
        voice_label: Reported::Unreported,
        events: Reported::Unreported,
        approvals: Reported::Unreported,
        interrupt: Reported::Unreported,
        pause_resume: Reported::Unreported,
        sessions_list: Reported::Unreported,
        transcript: Reported::Unreported,
        wake_available: Reported::Unreported,
        permission_posture: PermissionPosture::ObserveNotify,
        log_events: vec![],
        notes: vec![
            "OpenClaw command detected; live status refresh is running in the background"
                .to_string(),
        ],
    }
}

pub fn openclaw_snapshot(command_exists: bool) -> RuntimeSnapshot {
    if !command_exists {
        return RuntimeSnapshot::missing("openclaw");
    }

    let gateway = command_json("openclaw", &["gateway", "status", "--json"]);
    let status = command_json("openclaw", &["status", "--json"]);
    let gateway_text = command_text("openclaw", &["gateway", "status"]);
    let reachable = gateway.is_some() || status.is_some() || gateway_text.is_some();
    let log_events = read_openclaw_log_events();
    let mut notes =
        vec!["OpenClaw command detected; events sourced from openclaw logs --json".to_string()];
    if gateway.is_none() {
        notes.push(
            "OpenClaw gateway status JSON unavailable; run openclaw onboard or openclaw gateway status to finish backend setup".to_string(),
        );
    }

    RuntimeSnapshot {
        backend: BackendKind::OpenClaw,
        backend_label: "OPENCLAW".to_string(),
        reachable,
        model_label: model_from_backend(&status, None, None, &log_events),
        voice_label: Reported::Unreported,
        events: json_bool(&gateway, &["events", "capabilities.events"]),
        approvals: json_bool(&gateway, &["approvals", "capabilities.approvals"]),
        interrupt: json_bool(&gateway, &["interrupt", "capabilities.interrupt"]),
        pause_resume: json_bool(&gateway, &["pause_resume", "capabilities.pause_resume"]),
        sessions_list: json_bool(&status, &["sessions", "capabilities.sessions_list"]),
        transcript: json_bool(&status, &["transcript", "capabilities.transcript"]),
        wake_available: Reported::Unreported,
        permission_posture: PermissionPosture::ObserveNotify,
        log_events,
        notes,
    }
}

/// Read recent Hermes agent log output via `hermes logs agent --since 15m --lines 30`.
/// Hermes agent.log captures all agent activity: API calls, tool dispatch, session lifecycle.
pub fn read_hermes_log_events() -> Vec<CockpitEvent> {
    let text = hermes_agent_log_text().unwrap_or_default();
    let mut events: Vec<CockpitEvent> = vec![];
    let cutoff = Utc::now() - ChronoDuration::minutes(15);
    for line in text
        .lines()
        .rev()
        .take(200)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        if let Some(event) = parse_hermes_log_line(line) {
            if event.at < cutoff {
                continue;
            }
            events.push(event);
        }
    }
    keep_recent_events(&mut events, 12);
    events
}

fn hermes_agent_log_text() -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(home) = env::var_os("HERMES_HOME") {
        candidates.push(PathBuf::from(home).join("logs/agent.log"));
    }
    if let Some(home) = env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".hermes/logs/agent.log"));
    }
    candidates
        .into_iter()
        .find_map(|path| fs::read_to_string(path).ok())
}

/// Parse a single Hermes log line into a CockpitEvent.
/// Hermes log format: `2026-06-01 12:00:01,234 [LEVEL] component: message`
fn parse_hermes_log_line(line: &str) -> Option<CockpitEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // Try to extract timestamp from the beginning
    // Format: "2026-06-01 12:00:01,234 [LEVEL] component: message"
    let (timestamp_str, rest) = if line.len() > 24 && line.as_bytes()[10] == b' ' {
        let ts = &line[..19]; // "2026-06-01 12:00:01"
        let rest = &line[line.find(']').map(|i| i + 1).unwrap_or(20)..];
        (ts, rest.trim())
    } else {
        return None;
    };

    let at = if !timestamp_str.is_empty() {
        chrono::NaiveDateTime::parse_from_str(timestamp_str, "%Y-%m-%d %H:%M:%S")
            .ok()
            .map(|ndt| DateTime::from_naive_utc_and_offset(ndt, chrono::Utc))
            .unwrap_or_else(Utc::now)
    } else {
        Utc::now()
    };

    let lower = rest.to_lowercase();

    if let Some(class) = classify_backend_event(rest) {
        Some(CockpitEvent {
            at,
            kind: event_kind_for_class(&class),
            label: rest.to_string(),
        })
    } else if lower.contains("voice recording")
        || lower.contains("recording started")
        || lower.contains("listening")
        || lower.contains("awaiting audio")
    {
        Some(CockpitEvent {
            at,
            kind: EventKind::Listening,
            label: rest.to_string(),
        })
    } else if lower.contains("transcrib")
        || lower.contains("stt")
        || lower.contains("speech-to-text")
    {
        Some(CockpitEvent {
            at,
            kind: EventKind::Processing,
            label: rest.to_string(),
        })
    } else if lower.contains("text-to-speech")
        || lower.contains("tts")
        || lower.contains("speaking")
        || lower.contains("audio playback")
    {
        Some(CockpitEvent {
            at,
            kind: EventKind::Speaking,
            label: rest.to_string(),
        })
    } else if lower.contains("tool")
        && (lower.contains("dispatch") || lower.contains("start") || lower.contains("call"))
    {
        Some(CockpitEvent {
            at,
            kind: EventKind::ToolStart,
            label: rest.to_string(),
        })
    } else if lower.contains("tool")
        && (lower.contains("result") || lower.contains("finish") || lower.contains("complete"))
    {
        Some(CockpitEvent {
            at,
            kind: EventKind::ToolFinish,
            label: rest.to_string(),
        })
    } else if lower.contains("approval") || lower.contains("approve") {
        Some(CockpitEvent {
            at,
            kind: EventKind::ApprovalRequested,
            label: rest.to_string(),
        })
    } else if lower.contains("tool loop warning")
        || lower.contains("repeated_exact_failure_warning")
    {
        Some(CockpitEvent {
            at,
            kind: EventKind::Warning,
            label: rest.to_string(),
        })
    } else if lower.contains("error") || lower.contains("failed") || lower.contains("exception") {
        Some(CockpitEvent {
            at,
            kind: EventKind::Error,
            label: rest.to_string(),
        })
    } else if lower.contains("warning") || lower.contains("warn") {
        Some(CockpitEvent {
            at,
            kind: EventKind::Warning,
            label: rest.to_string(),
        })
    } else if lower.contains("awaiting") {
        Some(CockpitEvent {
            at,
            kind: EventKind::Listening,
            label: rest.to_string(),
        })
    } else if lower.contains("response") || lower.contains("stream") {
        Some(CockpitEvent {
            at,
            kind: EventKind::ResponseStream,
            label: rest.to_string(),
        })
    } else if lower.contains("session") || lower.contains("processing") || lower.contains("running")
    {
        Some(CockpitEvent {
            at,
            kind: EventKind::Processing,
            label: rest.to_string(),
        })
    } else if lower.contains("idle") {
        Some(CockpitEvent {
            at,
            kind: EventKind::Idle,
            label: rest.to_string(),
        })
    } else {
        None
    }
}

/// Read recent OpenClaw gateway log output via `openclaw logs --json --limit 20`.
/// OpenClaw logs are JSONL format with type-tagged log entries.
pub fn read_openclaw_log_events() -> Vec<CockpitEvent> {
    let output = match Command::new("openclaw")
        .args(["logs", "--json", "--limit", "20"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return vec![],
    };
    let text = match String::from_utf8(output.stdout) {
        Ok(text) => text,
        Err(_) => return vec![],
    };
    let mut events: Vec<CockpitEvent> = vec![];
    for line in text.lines() {
        if let Ok(json) = serde_json::from_str::<Value>(line)
            && let Some(event) = parse_openclaw_log_json(&json)
        {
            events.push(event);
        }
    }
    keep_recent_events(&mut events, 12);
    events
}

fn keep_recent_events(events: &mut Vec<CockpitEvent>, limit: usize) {
    if events.len() > limit {
        let drop_count = events.len() - limit;
        events.drain(0..drop_count);
    }
}

/// Parse an OpenClaw JSONL log entry into a CockpitEvent.
/// Format: {"timestamp":"...","level":"info","message":"...","type":"session.tool"}
fn parse_openclaw_log_json(json: &Value) -> Option<CockpitEvent> {
    let type_field = json.get("type").and_then(Value::as_str);
    let message = json.get("message").and_then(Value::as_str).unwrap_or("");
    let ts_str = json.get("timestamp").and_then(Value::as_str).unwrap_or("");

    let at = if !ts_str.is_empty() {
        chrono::DateTime::parse_from_rfc3339(ts_str)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now)
    } else {
        Utc::now()
    };

    let lower_msg = message.to_lowercase();
    let lower_type = type_field.unwrap_or("").to_lowercase();

    // Map based on OpenClaw event types
    let kind = if lower_type.contains("session.message") && lower_msg.contains("response") {
        EventKind::ResponseStream
    } else if let Some(class) = classify_backend_event(&format!("{lower_type} {lower_msg}")) {
        event_kind_for_class(&class)
    } else if lower_type.contains("session.tool") && lower_msg.contains("start") {
        EventKind::ToolStart
    } else if lower_type.contains("session.tool")
        && (lower_msg.contains("finish")
            || lower_msg.contains("complete")
            || lower_msg.contains("result"))
    {
        EventKind::ToolFinish
    } else if lower_type.contains("session.tool") {
        EventKind::ToolStart
    } else if lower_type.contains("approval") || lower_msg.contains("approval") {
        EventKind::ApprovalRequested
    } else if lower_type.contains("session") {
        EventKind::Processing
    } else if lower_type.contains("health") || lower_type.contains("heartbeat") {
        EventKind::Idle
    } else if lower_msg.contains("error") || lower_msg.contains("fail") {
        EventKind::Error
    } else if lower_msg.contains("warn") {
        EventKind::Warning
    } else if lower_msg.contains("listen") {
        EventKind::Listening
    } else {
        return None;
    };

    let label = if !message.is_empty() {
        message.to_string()
    } else {
        type_field.unwrap_or("log event").to_string()
    };

    Some(CockpitEvent { at, kind, label })
}

pub fn command_json(command: &str, args: &[&str]) -> Option<Value> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

pub fn command_text(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn json_bool(json: &Option<Value>, paths: &[&str]) -> Reported<bool> {
    for path in paths {
        let pointer = format!("/{}", path.replace('.', "/"));
        if let Some(value) = json.as_ref().and_then(|json| json.pointer(&pointer))
            && let Some(value) = value.as_bool()
        {
            return Reported::Value(value);
        }
    }
    Reported::Unreported
}

fn json_string(json: &Option<Value>, paths: &[&str]) -> Reported<String> {
    for path in paths {
        let pointer = format!("/{}", path.replace('.', "/"));
        if let Some(value) = json.as_ref().and_then(|json| json.pointer(&pointer))
            && let Some(value) = value.as_str()
        {
            return Reported::Value(value.to_string());
        }
    }
    Reported::Unreported
}

fn model_from_backend(
    status: &Option<Value>,
    status_text: Option<&str>,
    config_model: Option<&str>,
    log_events: &[CockpitEvent],
) -> Reported<String> {
    if let Some(model) = model_from_log_events(log_events) {
        return Reported::Value(model);
    }
    let from_status = json_string(
        status,
        &[
            "model.name",
            "model.model",
            "model.id",
            "model",
            "models.active.name",
            "models.active.model",
            "models.default.name",
            "models.default.model",
            "provider.model",
            "routing.model",
        ],
    );
    if !matches!(from_status, Reported::Unreported) {
        return from_status;
    }
    if let Some(model) = model_from_status_text(status_text) {
        return Reported::Value(model);
    }
    config_model
        .filter(|value| !value.trim().is_empty())
        .map(|value| Reported::Value(value.trim().to_string()))
        .unwrap_or(Reported::Unreported)
}

fn model_from_hermes_config(config_text: Option<&str>) -> Reported<String> {
    if let Ok(model) = env::var("HERMES_INFERENCE_MODEL")
        && !model.trim().is_empty()
    {
        return Reported::Value(model.trim().to_string());
    }

    let Some(text) = config_text else {
        return Reported::Unreported;
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some((label, value)) = trimmed.split_once(':')
            && label.trim() == "model"
        {
            let model = value.trim().trim_matches(['\'', '"']);
            if !model.is_empty() {
                return Reported::Value(model.to_string());
            }
        }
    }
    Reported::Unreported
}

fn model_from_log_events(events: &[CockpitEvent]) -> Option<String> {
    for event in events {
        if let Some(model) = value_after_key(&event.label, "model=") {
            return Some(model);
        }
        if let Some(model) = value_after_key(&event.label, "model:") {
            return Some(model);
        }
    }
    None
}

fn value_after_key(input: &str, key: &str) -> Option<String> {
    let start = input.find(key)? + key.len();
    let value = input[start..]
        .split_whitespace()
        .next()?
        .trim_matches(|ch: char| ch == ',' || ch == ';' || ch == ')' || ch == '"' || ch == '\'')
        .to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn model_from_status_text(status_text: Option<&str>) -> Option<String> {
    let text = status_text?;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some((label, value)) = trimmed.split_once(':')
            && label.trim().eq_ignore_ascii_case("model")
        {
            let model = value.trim();
            if !model.is_empty() {
                return Some(model.to_string());
            }
        }
    }
    None
}

fn hermes_gateway_notes(gateway_text: Option<&str>, default_profile: Option<&str>) -> Vec<String> {
    let Some(text) = gateway_text else {
        return vec!["Hermes gateway status unavailable; run `hermes gateway status` to inspect messaging backends".to_string()];
    };
    let resolution = gateway::resolve_hermes_gateway(default_profile, Some(text));
    let lower = text.to_lowercase();
    let mut notes = resolution.notes;
    let auto_restarting = lower.contains("activating (auto-restart)")
        || lower.contains("restart pending")
        || lower.contains("auto-restart");
    let stopped = lower.contains("user gateway service is stopped")
        || lower.contains("active: inactive (dead)");
    let running_profile = running_gateway_profile(text);

    if lower.contains("telegram bot token already in use") {
        let conflict = text
            .lines()
            .map(str::trim)
            .find(|line| {
                line.to_lowercase()
                    .contains("telegram bot token already in use")
            })
            .unwrap_or("telegram bot token already in use");
        if auto_restarting {
            notes.push(format!("Hermes gateway conflict: {conflict}"));
        } else {
            notes.push(format!("Hermes gateway last startup issue: {conflict}"));
        }
        if let Some(profile) = running_profile.as_deref() {
            notes.push(format!(
                "Hermes gateway active profile: `{profile}` is running; use `hermes --profile {profile} gateway restart` to restart it, or keep default stopped unless default should own the Telegram token"
            ));
        } else {
            notes.push("Hermes gateway repair: stop the process named in `hermes gateway status`, then run `hermes gateway restart`".to_string());
        }
    } else if stopped && let Some(profile) = running_profile.as_deref() {
        notes.push(format!(
            "Hermes gateway active profile: `{profile}` is running; default profile gateway is stopped"
        ));
    } else if lower.contains("user gateway service is running") {
        notes.push("Hermes gateway service is running".to_string());
    } else if lower.contains("user gateway service is stopped") {
        notes.push("Hermes gateway service is stopped; run `hermes gateway start` if this profile should receive messages".to_string());
    }

    notes
}

fn running_gateway_profile(gateway_text: &str) -> Option<String> {
    gateway_text.lines().find_map(|line| {
        let trimmed = line.trim();
        let rest = trimmed.strip_prefix('✓')?.trim();
        let profile = rest.split_whitespace().next()?;
        if profile == "User" || profile == "Systemd" {
            None
        } else {
            Some(profile.to_string())
        }
    })
}

pub fn hermes_config_text() -> Option<String> {
    hermes_config_candidates(env::var_os("HERMES_HOME"), env::var_os("HOME"))
        .into_iter()
        .find_map(|path| fs::read_to_string(path).ok())
}

fn hermes_config_candidates(hermes_home: Option<OsString>, home: Option<OsString>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = hermes_home {
        candidates.push(PathBuf::from(home).join("config.yaml"));
    }
    if let Some(home) = home {
        candidates.push(PathBuf::from(home).join(".hermes/config.yaml"));
    }
    candidates
}

fn hermes_default_profile(config_text: Option<&str>) -> Option<String> {
    env::var("HERMES_PROFILE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| yaml_section_value(config_text?, "profile", "default"))
        .or_else(|| yaml_section_value(config_text?, "profiles", "default"))
        .or_else(|| yaml_top_level_value(config_text?, "profile"))
        .or_else(|| yaml_top_level_value(config_text?, "default_profile"))
}

fn voice_from_hermes_config(config_text: Option<&str>) -> Reported<String> {
    let Some(text) = config_text else {
        return Reported::Unreported;
    };
    let stt_enabled =
        yaml_section_value(text, "stt", "enabled").unwrap_or_else(|| "true".to_string());
    let stt_provider =
        yaml_section_value(text, "stt", "provider").unwrap_or_else(|| "auto".to_string());
    let tts_provider =
        yaml_section_value(text, "tts", "provider").unwrap_or_else(|| "auto".to_string());
    let tts_voice = yaml_nested_value(text, "tts", &tts_provider, "voice")
        .or_else(|| yaml_nested_value(text, "tts", &tts_provider, "voice_id"));
    let tts_model = yaml_nested_value(text, "tts", &tts_provider, "model")
        .or_else(|| yaml_nested_value(text, "tts", &tts_provider, "model_id"));
    let record_key =
        yaml_section_value(text, "voice", "record_key").unwrap_or_else(|| "ctrl+b".to_string());
    let auto_tts =
        yaml_section_value(text, "voice", "auto_tts").unwrap_or_else(|| "false".to_string());

    let mut parts = vec![format!("STT {}", bool_prefix(&stt_enabled, &stt_provider))];
    let mut tts = format!("TTS {tts_provider}");
    if let Some(voice) = tts_voice {
        tts.push('/');
        tts.push_str(&voice);
    } else if let Some(model) = tts_model {
        tts.push('/');
        tts.push_str(&model);
    }
    parts.push(tts);
    parts.push(format!("key {record_key}"));
    if auto_tts.eq_ignore_ascii_case("true") {
        parts.push("auto-tts".to_string());
    }
    Reported::Value(parts.join(" · "))
}

fn bool_prefix(enabled: &str, provider: &str) -> String {
    if enabled.eq_ignore_ascii_case("false") {
        format!("off/{provider}")
    } else {
        provider.to_string()
    }
}

fn yaml_section_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') && trimmed.ends_with(':') {
            in_section = trimmed.trim_end_matches(':') == section;
            continue;
        }
        if in_section
            && line.starts_with("  ")
            && !line.starts_with("    ")
            && let Some((candidate, value)) = trimmed.split_once(':')
            && candidate.trim() == key
        {
            return clean_yaml_value(value);
        }
    }
    None
}

fn yaml_top_level_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        if line.starts_with(' ') {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((candidate, value)) = trimmed.split_once(':')
            && candidate.trim() == key
        {
            return clean_yaml_value(value);
        }
    }
    None
}

fn yaml_nested_value(text: &str, section: &str, nested: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    let mut in_nested = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') && trimmed.ends_with(':') {
            in_section = trimmed.trim_end_matches(':') == section;
            in_nested = false;
            continue;
        }
        if in_section && line.starts_with("  ") && !line.starts_with("    ") {
            in_nested = trimmed.trim_end_matches(':') == nested;
            continue;
        }
        if in_section
            && in_nested
            && line.starts_with("    ")
            && let Some((candidate, value)) = trimmed.split_once(':')
            && candidate.trim() == key
        {
            return clean_yaml_value(value);
        }
    }
    None
}

fn clean_yaml_value(value: &str) -> Option<String> {
    let value = value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();
    if value.is_empty() { None } else { Some(value) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_metric_stays_unreported() {
        let snapshot = RuntimeSnapshot::missing("hermes");
        assert_eq!(snapshot.model_label, Reported::Unreported);
    }

    #[test]
    fn model_comes_from_backend_status_before_config_fallback() {
        let status = Some(serde_json::json!({ "model": { "model": "gpt-5.4" } }));
        assert_eq!(
            model_from_backend(&status, None, Some("fallback-model"), &[]),
            Reported::Value("gpt-5.4".to_string())
        );
    }

    #[test]
    fn model_prefers_recent_log_activity() {
        let status = Some(serde_json::json!({ "model": { "model": "old-default" } }));
        let events = vec![CockpitEvent::now(
            EventKind::Processing,
            "conversation turn: model=gemini-3.1-flash-lite-preview provider=gemini",
        )];
        assert_eq!(
            model_from_backend(&status, None, None, &events),
            Reported::Value("gemini-3.1-flash-lite-preview".to_string())
        );
    }

    #[test]
    fn model_can_come_from_human_status_text() {
        let status_text = r#"
◆ Environment
  Model:        openrouter/owl-alpha
  Provider:     OpenRouter
"#;
        assert_eq!(
            model_from_backend(&None, Some(status_text), None, &[]),
            Reported::Value("openrouter/owl-alpha".to_string())
        );
    }

    #[test]
    fn model_can_fall_back_to_backend_config_command() {
        assert_eq!(
            model_from_backend(&None, None, Some("claude-sonnet"), &[]),
            Reported::Value("claude-sonnet".to_string())
        );
    }

    #[test]
    fn voice_label_comes_from_hermes_config() {
        let config = r#"
tts:
  provider: gemini
  gemini:
    model: gemini-2.5-flash-preview-tts
    voice: Kore
stt:
  enabled: true
  provider: groq
voice:
  record_key: ctrl+b
  auto_tts: false
"#;
        assert_eq!(
            voice_from_hermes_config(Some(config)),
            Reported::Value("STT groq · TTS gemini/Kore · key ctrl+b".to_string())
        );
    }

    #[test]
    fn gateway_notes_detect_profile_token_conflict() {
        let status = r#"
Active: activating (auto-restart) since Tue 2026-06-02 03:05:43 EDT
✗ User gateway service is stopped
Recent gateway health:
  ⚠ telegram: Telegram bot token already in use (PID 741). Stop the other gateway first.
  ⏳ Restart pending: systemd is waiting to relaunch the gateway
Other profiles:
  ✓ portal           — PID 741
"#;
        let notes = hermes_gateway_notes(Some(status), Some("default"));
        assert!(
            notes
                .iter()
                .any(|note| note.contains("Telegram bot token already in use"))
        );
        assert!(notes.iter().any(|note| {
            note.contains("hermes --profile portal gateway restart")
                && note.contains("active profile")
        }));
    }

    #[test]
    fn gateway_notes_treat_stopped_default_conflict_as_last_issue() {
        let status = r#"
Active: inactive (dead) since Tue 2026-06-02 03:14:56 EDT
✗ User gateway service is stopped
Recent gateway health:
  ⚠ Last startup issue: telegram: Telegram bot token already in use (PID 741). Stop the other gateway first.
Other profiles:
  ✓ portal           — PID 115002
"#;
        let notes = hermes_gateway_notes(Some(status), Some("default"));
        assert!(notes.iter().any(|note| note.contains("last startup issue")));
        assert!(
            notes
                .iter()
                .any(|note| note.contains("active profile: `portal`"))
        );
    }

    #[test]
    fn gateway_notes_detect_running_service() {
        let notes =
            hermes_gateway_notes(Some("✓ User gateway service is running"), Some("default"));
        assert!(
            notes
                .iter()
                .any(|note| note == "Hermes default gateway is running")
        );
    }

    #[test]
    fn default_profile_can_come_from_hermes_config() {
        let config = r#"
profile: default
tts:
  provider: gemini
"#;

        assert_eq!(
            hermes_default_profile(Some(config)),
            Some("default".to_string())
        );
    }

    #[test]
    fn hermes_config_candidates_prefer_hermes_home() {
        let candidates = hermes_config_candidates(
            Some(OsString::from("/tmp/hermes-custom")),
            Some(OsString::from("/home/user")),
        );

        assert_eq!(
            candidates,
            vec![
                PathBuf::from("/tmp/hermes-custom/config.yaml"),
                PathBuf::from("/home/user/.hermes/config.yaml"),
            ]
        );
    }

    #[test]
    fn keeps_recent_events_in_chronological_order() {
        let mut events = vec![
            CockpitEvent::now(EventKind::Idle, "old idle"),
            CockpitEvent::now(EventKind::Processing, "middle processing"),
            CockpitEvent::now(EventKind::Speaking, "new speaking"),
        ];
        keep_recent_events(&mut events, 2);
        assert_eq!(events[0].label, "middle processing");
        assert_eq!(events[1].label, "new speaking");
    }

    #[test]
    fn parses_hermes_log_line_tool_start() {
        let line = "2026-06-01 12:00:03,456 [INFO] tools: tool dispatch start: web.search";
        let event = parse_hermes_log_line(line);
        assert!(event.is_some());
        assert_eq!(event.as_ref().unwrap().kind, EventKind::ToolStart);
        assert!(event.unwrap().label.contains("web.search"));
    }

    #[test]
    fn parses_hermes_voice_recording_as_listening() {
        let line = "2026-06-01 17:33:44,018 INFO tools.voice_mode: Voice recording started (rate=16000, channels=1)";
        let event = parse_hermes_log_line(line);
        assert_eq!(event.as_ref().unwrap().kind, EventKind::Listening);
    }

    #[test]
    fn parses_hermes_stt_as_processing() {
        let line =
            "2026-06-01 17:33:44,018 INFO tools.voice_mode: STT transcribing audio with groq";
        let event = parse_hermes_log_line(line);
        assert_eq!(event.as_ref().unwrap().kind, EventKind::Processing);
    }

    #[test]
    fn parses_hermes_tts_as_speaking() {
        let line = "2026-06-01 17:33:44,018 INFO tools.voice_mode: TTS audio playback started with gemini voice Kore";
        let event = parse_hermes_log_line(line);
        assert_eq!(event.as_ref().unwrap().kind, EventKind::Speaking);
    }

    #[test]
    fn parses_hermes_tts_end_as_listening() {
        let line = "2026-06-01 17:33:45,018 INFO tools.voice_mode: audio playback ended; ready for next turn";
        let event = parse_hermes_log_line(line);
        assert_eq!(event.as_ref().unwrap().kind, EventKind::Listening);
    }

    #[test]
    fn parses_hermes_turn_complete_as_response() {
        let line = "2026-06-01 17:33:45,018 INFO agent: final response ready for turn voice-1";
        let event = parse_hermes_log_line(line);
        assert_eq!(event.as_ref().unwrap().kind, EventKind::ResponseStream);
    }

    #[test]
    fn parses_hermes_log_line_tool_finish() {
        let line = "2026-06-01 12:00:05,456 [INFO] tools: tool result: web.search (success)";
        let event = parse_hermes_log_line(line);
        assert!(event.is_some());
        assert_eq!(event.unwrap().kind, EventKind::ToolFinish);
    }

    #[test]
    fn parses_hermes_log_line_approval() {
        let line =
            "2026-06-01 12:00:10,123 [WARNING] agent: approval requested for rm -rf build-cache";
        let event = parse_hermes_log_line(line);
        assert!(event.is_some());
        assert_eq!(event.unwrap().kind, EventKind::ApprovalRequested);
    }

    #[test]
    fn parses_hermes_log_line_error() {
        let line = "2026-06-01 12:00:15,789 [ERROR] tools: web.search failed: timeout";
        let event = parse_hermes_log_line(line);
        assert!(event.is_some());
        assert_eq!(event.unwrap().kind, EventKind::Error);
    }

    #[test]
    fn parses_hermes_loop_failure_warning_without_error_mode() {
        let line = "2026-06-02 08:01:07,185 [WARNING] repeated_exact_failure_warning; count=2; skill_manage has failed 2 times with identical arguments";
        let event = parse_hermes_log_line(line);
        assert!(event.is_some());
        assert_eq!(event.unwrap().kind, EventKind::Warning);
    }

    #[test]
    fn parses_openclaw_log_json_tool_start() {
        let json: Value = serde_json::from_str(
            r#"{"timestamp":"2026-06-01T12:00:03.000Z","level":"info","message":"tool:web.search started","type":"session.tool","session_id":"abc123"}"#,
        )
        .unwrap();
        let event = parse_openclaw_log_json(&json);
        assert!(event.is_some());
        assert_eq!(event.as_ref().unwrap().kind, EventKind::ToolStart);
        assert!(event.unwrap().label.contains("web.search"));
    }

    #[test]
    fn parses_openclaw_log_json_approval() {
        let json: Value = serde_json::from_str(
            r#"{"timestamp":"2026-06-01T12:00:10.000Z","level":"warn","message":"approval required for destructive operation","type":"session.operation"}"#,
        )
        .unwrap();
        let event = parse_openclaw_log_json(&json);
        assert!(event.is_some());
        assert_eq!(event.unwrap().kind, EventKind::ApprovalRequested);
    }

    #[test]
    fn parses_openclaw_audio_end_as_listening() {
        let json: Value = serde_json::from_str(
            r#"{"timestamp":"2026-06-01T12:00:12.000Z","level":"info","message":"audio playback ended; ready for next turn","type":"session.audio"}"#,
        )
        .unwrap();
        let event = parse_openclaw_log_json(&json);
        assert!(event.is_some());
        assert_eq!(event.unwrap().kind, EventKind::Listening);
    }

    #[test]
    fn parses_openclaw_session_message_response_before_generic_session_progress() {
        let json: Value = serde_json::from_str(
            r#"{"timestamp":"2026-06-01T12:00:13.000Z","level":"info","message":"response complete","type":"session.message"}"#,
        )
        .unwrap();
        let event = parse_openclaw_log_json(&json);
        assert!(event.is_some());
        assert_eq!(event.unwrap().kind, EventKind::ResponseStream);
    }
}
