use crate::domain::{BackendKind, CockpitEvent, EventKind, PermissionPosture};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reported<T> {
    Value(T),
    Unreported,
    Unavailable,
}

impl<T> Reported<T> {
    pub fn as_ref(&self) -> Reported<&T> {
        match self {
            Self::Value(value) => Reported::Value(value),
            Self::Unreported => Reported::Unreported,
            Self::Unavailable => Reported::Unavailable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub backend: BackendKind,
    pub backend_label: String,
    pub reachable: bool,
    pub model_label: Reported<String>,
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
    let doctor_text = command_text("hermes", &["doctor"]);
    let configured_model = command_text("hermes", &["config", "get", "model"]);
    let reachable = status.is_some() || status_text.is_some() || doctor_text.is_some();
    let log_events = read_hermes_log_events();
    let mut notes =
        vec!["Hermes command detected; events sourced from hermes logs agent".to_string()];
    if status.is_none() {
        notes.push(
            "Hermes status JSON unavailable; capability detection is degraded until Hermes exposes machine-readable status".to_string(),
        );
    }

    RuntimeSnapshot {
        backend: BackendKind::Hermes,
        backend_label: "HERMES".to_string(),
        reachable,
        model_label: model_from_backend(&status, configured_model.as_deref()),
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
        model_label: model_from_backend(&status, None),
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

/// Read recent Hermes agent log output via `hermes logs agent --lines 30`.
/// Hermes agent.log captures all agent activity: API calls, tool dispatch, session lifecycle.
pub fn read_hermes_log_events() -> Vec<CockpitEvent> {
    let output = match Command::new("hermes")
        .args(["logs", "agent", "--lines", "30"])
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
        if let Some(event) = parse_hermes_log_line(line) {
            events.push(event);
        }
    }
    events.reverse();
    events.truncate(12);
    events
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
        ("", line)
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

    if lower.contains("tool")
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
    } else if lower.contains("listening") || lower.contains("awaiting") {
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
    events.reverse();
    events.truncate(12);
    events
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
    let kind = if lower_type.contains("session.tool") && lower_msg.contains("start") {
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
    } else if lower_type.contains("session.message") && lower_msg.contains("response") {
        EventKind::ResponseStream
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

fn model_from_backend(status: &Option<Value>, config_model: Option<&str>) -> Reported<String> {
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
    config_model
        .filter(|value| !value.trim().is_empty())
        .map(|value| Reported::Value(value.trim().to_string()))
        .unwrap_or(Reported::Unreported)
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
            model_from_backend(&status, Some("fallback-model")),
            Reported::Value("gpt-5.4".to_string())
        );
    }

    #[test]
    fn model_can_fall_back_to_backend_config_command() {
        assert_eq!(
            model_from_backend(&None, Some("claude-sonnet")),
            Reported::Value("claude-sonnet".to_string())
        );
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
}
