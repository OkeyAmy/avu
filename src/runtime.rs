use crate::domain::{BackendKind, CockpitEvent, EventKind, PermissionPosture};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{fs, process::Command, thread, time::Duration};

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
    pub cpu_percent: Reported<u8>,
    pub memory_percent: Reported<u8>,
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
            cpu_percent: Reported::Unreported,
            memory_percent: Reported::Unreported,
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
    let doctor = command_json("hermes", &["doctor", "--json"]);
    let reachable = status.is_some() || doctor.is_some();
    let log_events = read_hermes_log_events();

    RuntimeSnapshot {
        backend: BackendKind::Hermes,
        backend_label: "HERMES".to_string(),
        reachable,
        model_label: json_string(&status, &["model.name", "model"]),
        events: json_bool(&status, &["events", "capabilities.events"]),
        approvals: json_bool(&status, &["approvals", "capabilities.approvals"]),
        interrupt: json_bool(&status, &["interrupt", "capabilities.interrupt"]),
        pause_resume: json_bool(&status, &["pause_resume", "capabilities.pause_resume"]),
        sessions_list: json_bool(&status, &["sessions", "capabilities.sessions_list"]),
        transcript: json_bool(&status, &["transcript", "capabilities.transcript"]),
        wake_available: Reported::Unreported,
        permission_posture: PermissionPosture::ObserveNotify,
        cpu_percent: system_cpu_percent(),
        memory_percent: system_memory_percent(),
        log_events,
        notes: vec!["Hermes command detected; events sourced from hermes logs agent".to_string()],
    }
}

pub fn openclaw_snapshot(command_exists: bool) -> RuntimeSnapshot {
    if !command_exists {
        return RuntimeSnapshot::missing("openclaw");
    }

    let gateway = command_json("openclaw", &["gateway", "status", "--json"]);
    let status = command_json("openclaw", &["status", "--json"]);
    let reachable = gateway.is_some() || status.is_some();
    let log_events = read_openclaw_log_events();

    RuntimeSnapshot {
        backend: BackendKind::OpenClaw,
        backend_label: "OPENCLAW".to_string(),
        reachable,
        model_label: json_string(&status, &["model.name", "model"]),
        events: json_bool(&gateway, &["events", "capabilities.events"]),
        approvals: json_bool(&gateway, &["approvals", "capabilities.approvals"]),
        interrupt: json_bool(&gateway, &["interrupt", "capabilities.interrupt"]),
        pause_resume: json_bool(&gateway, &["pause_resume", "capabilities.pause_resume"]),
        sessions_list: json_bool(&status, &["sessions", "capabilities.sessions_list"]),
        transcript: json_bool(&status, &["transcript", "capabilities.transcript"]),
        wake_available: Reported::Unreported,
        permission_posture: PermissionPosture::ObserveNotify,
        cpu_percent: system_cpu_percent(),
        memory_percent: system_memory_percent(),
        log_events,
        notes: vec![
            "OpenClaw command detected; events sourced from openclaw logs --json".to_string(),
        ],
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

pub fn parse_memory_percent(input: &str) -> Option<u8> {
    let mut total = None;
    let mut available = None;
    for line in input.lines() {
        let mut parts = line.split_whitespace();
        match parts.next()? {
            "MemTotal:" => total = parts.next()?.parse::<u64>().ok(),
            "MemAvailable:" => available = parts.next()?.parse::<u64>().ok(),
            _ => {}
        }
    }
    let total = total?;
    let available = available?;
    if total == 0 || available > total {
        return None;
    }
    Some((((total - available) * 100) / total).min(100) as u8)
}

pub fn parse_cpu_percent(first: &str, second: &str) -> Option<u8> {
    fn totals(input: &str) -> Option<(u64, u64)> {
        let values: Vec<u64> = input
            .lines()
            .next()?
            .strip_prefix("cpu ")?
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        let idle = *values.get(3)? + values.get(4).copied().unwrap_or(0);
        let total = values.iter().sum();
        Some((idle, total))
    }

    let (idle_a, total_a) = totals(first)?;
    let (idle_b, total_b) = totals(second)?;
    let total_delta = total_b.checked_sub(total_a)?;
    let idle_delta = idle_b.checked_sub(idle_a)?;
    if total_delta == 0 || idle_delta > total_delta {
        return None;
    }
    Some((((total_delta - idle_delta) * 100) / total_delta).min(100) as u8)
}

pub fn system_cpu_percent() -> Reported<u8> {
    let first = match fs::read_to_string("/proc/stat") {
        Ok(value) => value,
        Err(_) => return Reported::Unreported,
    };
    thread::sleep(Duration::from_millis(60));
    let second = match fs::read_to_string("/proc/stat") {
        Ok(value) => value,
        Err(_) => return Reported::Unreported,
    };
    parse_cpu_percent(&first, &second).map_or(Reported::Unreported, Reported::Value)
}

pub fn system_memory_percent() -> Reported<u8> {
    fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|input| parse_memory_percent(&input))
        .map_or(Reported::Unreported, Reported::Value)
}

pub fn command_json(command: &str, args: &[&str]) -> Option<Value> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_meminfo_percent_from_linux_proc_text() {
        let input = "MemTotal:       1000 kB\nMemAvailable:    250 kB\n";
        assert_eq!(parse_memory_percent(input), Some(75));
    }

    #[test]
    fn parses_cpu_percent_from_two_proc_stat_samples() {
        let first = "cpu  100 0 100 800 0 0 0 0 0 0\n";
        let second = "cpu  150 0 150 900 0 0 0 0 0 0\n";
        assert_eq!(parse_cpu_percent(first, second), Some(50));
    }

    #[test]
    fn missing_metric_stays_unreported() {
        let snapshot = RuntimeSnapshot::missing("hermes");
        assert_eq!(snapshot.cpu_percent, Reported::Unreported);
        assert_eq!(snapshot.model_label, Reported::Unreported);
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
