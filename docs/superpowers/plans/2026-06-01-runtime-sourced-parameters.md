# Runtime-Sourced Parameters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace hardcoded live Avu cockpit parameters with values sourced from Hermes/OpenClaw config/probes, Avu config, and direct system detection.

**Architecture:** Add a `runtime` module that builds a `RuntimeSnapshot` from backend probes and system probes. Convert live snapshots into `CockpitState`, while keeping fixture/demo state explicit and isolated.

**Tech Stack:** Rust 2024, `serde`, `serde_json`, `toml`, `anyhow`, Linux `/proc`, current `ratatui` TUI domain model.

---

## File Structure

- Create `src/runtime.rs`: runtime snapshot types, backend JSON parsing, command probing, Linux system metric parsing.
- Modify `src/main.rs`: add `mod runtime;`.
- Modify `src/domain.rs`: add `CockpitState::from_runtime()` and remove live use of fixture/default modules.
- Modify `src/backend.rs`: make Hermes/OpenClaw build live state from `RuntimeSnapshot`; make auto mode choose missing/live backend, not fake fallback.
- Modify `src/config.rs`: load `~/.avu/config.toml` when present and use defaults only as absent-file fallback.
- Modify `src/engine.rs`: derive module levels from reported snapshot fields; avoid invented live percentages.
- Modify `tests/cli_smoke.rs`: assert live/missing modes do not show fixture values.

---

### Task 1: Add RuntimeSnapshot and system metric parsing

**Files:**
- Create: `src/runtime.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write failing unit tests in `src/runtime.rs`**

```rust
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
}
```

- [ ] **Step 2: Run tests and confirm failure**

Run: `cargo test runtime::tests -- --nocapture`

Expected: compile failure because `runtime` module/types/functions do not exist.

- [ ] **Step 3: Implement `src/runtime.rs`**

```rust
use crate::domain::{BackendKind, PermissionPosture};
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
            notes: vec![format!("{command} command was not found on PATH")],
        }
    }
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
    fn totals(line: &str) -> Option<(u64, u64)> {
        let values: Vec<u64> = line
            .strip_prefix("cpu ")?
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        let idle = *values.get(3)? + values.get(4).copied().unwrap_or(0);
        let total = values.iter().sum();
        Some((idle, total))
    }
    let (idle_a, total_a) = totals(first.lines().next()?)?;
    let (idle_b, total_b) = totals(second.lines().next()?)?;
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
```

- [ ] **Step 4: Register module in `src/main.rs`**

Add this beside existing modules:

```rust
mod runtime;
```

- [ ] **Step 5: Run test to verify pass**

Run: `cargo test runtime::tests -- --nocapture`

Expected: 3 runtime tests pass.

---

### Task 2: Build CockpitState from RuntimeSnapshot

**Files:**
- Modify: `src/domain.rs`
- Test: unit tests in `src/domain.rs`

- [ ] **Step 1: Add failing tests**

```rust
#[cfg(test)]
mod runtime_tests {
    use super::*;
    use crate::runtime::{Reported, RuntimeSnapshot};

    #[test]
    fn live_missing_runtime_does_not_use_fixture_values() {
        let state = CockpitState::from_runtime(RuntimeSnapshot::missing("hermes"));
        assert_eq!(state.backend, BackendKind::Missing);
        assert_eq!(state.model_label, "unreported");
        assert_eq!(state.tools_active, 0);
        assert_eq!(state.cpu_percent, 0);
        assert!(state.pending_approval.is_none());
        assert!(!state.events.iter().any(|event| event.label.contains("tool:web.search")));
    }

    #[test]
    fn live_runtime_uses_reported_model_and_metrics() {
        let mut snapshot = RuntimeSnapshot::missing("hermes");
        snapshot.backend = BackendKind::Hermes;
        snapshot.backend_label = "HERMES".to_string();
        snapshot.reachable = true;
        snapshot.model_label = Reported::Value("claude-local".to_string());
        snapshot.cpu_percent = Reported::Value(31);
        snapshot.memory_percent = Reported::Value(44);
        let state = CockpitState::from_runtime(snapshot);
        assert_eq!(state.model_label, "claude-local");
        assert_eq!(state.cpu_percent, 31);
        assert_eq!(state.memory_percent, 44);
    }
}
```

- [ ] **Step 2: Run tests and confirm failure**

Run: `cargo test domain::runtime_tests -- --nocapture`

Expected: compile failure because `CockpitState::from_runtime` does not exist.

- [ ] **Step 3: Implement conversion**

Add helpers in `src/domain.rs`:

```rust
use crate::runtime::{Reported, RuntimeSnapshot};

impl CockpitState {
    pub fn from_runtime(snapshot: RuntimeSnapshot) -> Self {
        let capabilities = CapabilitySnapshot::from_runtime(&snapshot);
        let mode = if snapshot.reachable {
            CockpitMode::Idle
        } else {
            CockpitMode::Disconnected
        };
        let events = vec![CockpitEvent::now(
            if snapshot.reachable { EventKind::Idle } else { EventKind::Warning },
            if snapshot.reachable { "live backend reported idle" } else { "live backend unavailable" },
        )];
        Self {
            app_name: "Avu".to_string(),
            mode,
            backend: snapshot.backend,
            backend_label: snapshot.backend_label,
            model_label: reported_string(snapshot.model_label),
            voice_label: reported_bool(snapshot.wake_available.as_ref(), "WAKE", "KEYBOARD", "unreported"),
            tools_active: 0,
            cpu_percent: reported_u8(snapshot.cpu_percent),
            memory_percent: reported_u8(snapshot.memory_percent),
            capabilities,
            modules: modules_for_runtime(&snapshot),
            events,
            pending_approval: None,
        }
    }
}

impl CapabilitySnapshot {
    pub fn from_runtime(snapshot: &RuntimeSnapshot) -> Self {
        Self {
            backend: snapshot.backend.clone(),
            reachable: snapshot.reachable,
            events: reported_bool_value(&snapshot.events),
            approvals: reported_bool_value(&snapshot.approvals),
            interrupt: reported_bool_value(&snapshot.interrupt),
            pause_resume: reported_bool_value(&snapshot.pause_resume),
            sessions_list: reported_bool_value(&snapshot.sessions_list),
            transcript: reported_bool_value(&snapshot.transcript),
            wake_available: reported_bool_value(&snapshot.wake_available),
            permission_posture: snapshot.permission_posture.clone(),
            notes: snapshot.notes.clone(),
        }
    }
}

fn reported_string(value: Reported<String>) -> String {
    match value {
        Reported::Value(value) => value,
        Reported::Unavailable => "unavailable".to_string(),
        Reported::Unreported => "unreported".to_string(),
    }
}

fn reported_u8(value: Reported<u8>) -> u8 {
    match value {
        Reported::Value(value) => value,
        Reported::Unavailable | Reported::Unreported => 0,
    }
}

fn reported_bool_value(value: &Reported<bool>) -> bool {
    matches!(value, Reported::Value(true))
}

fn reported_bool(value: Reported<&bool>, yes: &str, no: &str, unknown: &str) -> String {
    match value {
        Reported::Value(true) => yes.to_string(),
        Reported::Value(false) => no.to_string(),
        Reported::Unavailable => "unavailable".to_string(),
        Reported::Unreported => unknown.to_string(),
    }
}

fn modules_for_runtime(snapshot: &RuntimeSnapshot) -> Vec<ModuleStatus> {
    vec![
        module("INPUT", bool_level(&snapshot.wake_available), detail_bool(&snapshot.wake_available, "wake input", "keyboard only")),
        module("VOICE", bool_level(&snapshot.wake_available), detail_bool(&snapshot.wake_available, "wake available", "wake disabled")),
        module("TOOLS", bool_level(&snapshot.events), detail_bool(&snapshot.events, "events reported", "events disabled")),
        module("NET", if snapshot.reachable { 100 } else { 0 }, if snapshot.reachable { "reachable" } else { "unreachable" }),
        module("AGENT", bool_level(&snapshot.sessions_list), detail_bool(&snapshot.sessions_list, "sessions reported", "sessions unavailable")),
        module("NLP", bool_level(&snapshot.transcript), detail_bool(&snapshot.transcript, "transcript reported", "transcript unavailable")),
        module("TTS", 0, "unreported"),
    ]
}

fn bool_level(value: &Reported<bool>) -> u8 {
    match value {
        Reported::Value(true) => 100,
        Reported::Value(false) | Reported::Unavailable | Reported::Unreported => 0,
    }
}

fn detail_bool<'a>(value: &Reported<bool>, yes: &'a str, no: &'a str) -> &'a str {
    match value {
        Reported::Value(true) => yes,
        Reported::Value(false) => no,
        Reported::Unavailable => "unavailable",
        Reported::Unreported => "unreported",
    }
}

fn module(name: &str, level: u8, detail: &str) -> ModuleStatus {
    ModuleStatus {
        name: name.to_string(),
        level,
        active: level > 0,
        detail: detail.to_string(),
    }
}
```

- [ ] **Step 4: Run domain tests**

Run: `cargo test domain::runtime_tests -- --nocapture`

Expected: both tests pass.

---

### Task 3: Probe Hermes/OpenClaw without capability guessing

**Files:**
- Modify: `src/backend.rs`
- Modify: `src/runtime.rs`
- Test: `tests/cli_smoke.rs`

- [ ] **Step 1: Add failing CLI assertions**

Add checks to existing missing-backend tests:

```rust
.stdout(predicate::str::contains("Model: unreported"))
.stdout(predicate::str::contains("Pending approval: none"))
.stdout(predicate::str::contains("tool:web.search").not())
```

- [ ] **Step 2: Run tests and confirm failure if live path still leaks constants**

Run: `cargo test --test cli_smoke -- --nocapture`

Expected: tests fail before backend conversion is complete.

- [ ] **Step 3: Add backend snapshot builders**

In `src/runtime.rs`, add:

```rust
pub fn hermes_snapshot(command_exists: bool) -> RuntimeSnapshot {
    if !command_exists {
        return RuntimeSnapshot::missing("hermes");
    }
    let status = command_json("hermes", &["status", "--json"]);
    let doctor = command_json("hermes", &["doctor", "--json"]);
    let model = status
        .as_ref()
        .and_then(|json| json.pointer("/model/name"))
        .or_else(|| status.as_ref().and_then(|json| json.get("model")))
        .and_then(Value::as_str)
        .map(|value| Reported::Value(value.to_string()))
        .unwrap_or(Reported::Unreported);
    let reachable = status.is_some() || doctor.is_some();
    RuntimeSnapshot {
        backend: BackendKind::Hermes,
        backend_label: "HERMES".to_string(),
        reachable,
        model_label: model,
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
        notes: vec!["Hermes command detected; explicit JSON capabilities only".to_string()],
    }
}

pub fn openclaw_snapshot(command_exists: bool) -> RuntimeSnapshot {
    if !command_exists {
        return RuntimeSnapshot::missing("openclaw");
    }
    let gateway = command_json("openclaw", &["gateway", "status", "--json"]);
    let status = command_json("openclaw", &["status", "--json"]);
    let model = status
        .as_ref()
        .and_then(|json| json.pointer("/model/name"))
        .or_else(|| status.as_ref().and_then(|json| json.get("model")))
        .and_then(Value::as_str)
        .map(|value| Reported::Value(value.to_string()))
        .unwrap_or(Reported::Unreported);
    let reachable = gateway.is_some() || status.is_some();
    RuntimeSnapshot {
        backend: BackendKind::OpenClaw,
        backend_label: "OPENCLAW".to_string(),
        reachable,
        model_label: model,
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
        notes: vec!["OpenClaw command detected; explicit JSON capabilities only".to_string()],
    }
}

fn json_bool(json: &Option<Value>, paths: &[&str]) -> Reported<bool> {
    for path in paths {
        let pointer = format!("/{}", path.replace('.', "/"));
        if let Some(value) = json.as_ref().and_then(|json| json.pointer(&pointer)) {
            if let Some(value) = value.as_bool() {
                return Reported::Value(value);
            }
        }
    }
    Reported::Unreported
}
```

- [ ] **Step 4: Use snapshots in `src/backend.rs`**

Replace live `probe()` and `cockpit_state()` implementation with snapshot conversion:

```rust
fn probe(&self) -> CapabilitySnapshot {
    CapabilitySnapshot::from_runtime(&crate::runtime::hermes_snapshot(command_exists("hermes")))
}

fn cockpit_state(&self) -> CockpitState {
    CockpitState::from_runtime(crate::runtime::hermes_snapshot(command_exists("hermes")))
}
```

Use `openclaw_snapshot(command_exists("openclaw"))` for OpenClaw.

- [ ] **Step 5: Make auto mode avoid fake fallback**

In `adapter_for`, change the final `else` for `BackendChoice::Auto` to prefer a missing Hermes adapter or add a `MissingBackend` adapter. The behavior must report disconnected live state instead of fake fixture state.

- [ ] **Step 6: Run CLI tests**

Run: `cargo test --test cli_smoke -- --nocapture`

Expected: missing live backends show `unreported`, no pending approval, and no fixture events.

---

### Task 4: Load Avu config from disk without overriding backend truth

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Add config loading tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_config_from_toml_text() {
        let input = r#"
backend = "hermes"
permission_posture = "observe_notify"
wake_mode = "keyboard_only"
wake_phrase = "hey avu"
"#;
        let config: AvuConfig = toml::from_str(input).expect("valid config");
        assert_eq!(config.backend, "hermes");
        assert_eq!(config.wake_phrase, "hey avu");
    }
}
```

- [ ] **Step 2: Implement file loading**

Add:

```rust
impl AvuConfig {
    pub fn load(paths: &AvuPaths) -> Result<Self> {
        match std::fs::read_to_string(&paths.config) {
            Ok(contents) => Ok(toml::from_str(&contents)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }
}
```

Change `print_effective()` to call `AvuConfig::load(&paths)?` instead of `AvuConfig::default()`.

- [ ] **Step 3: Run config tests**

Run: `cargo test config::tests -- --nocapture`

Expected: config test passes.

---

### Task 5: Verify full runtime separation

**Files:**
- Modify tests only if verification reveals missing assertions.

- [ ] **Step 1: Run formatter and tests**

Run: `cargo fmt && cargo test`

Expected: all tests pass.

- [ ] **Step 2: Run type/lint checks**

Run: `cargo check && cargo clippy --all-targets --all-features -- -D warnings`

Expected: both pass with zero warnings.

- [ ] **Step 3: Smoke live missing path**

Run: `cargo run -- status --backend hermes`

Expected when Hermes is missing: disconnected/unreported values, no fixture approval, no fixture model, no fixture events.

- [ ] **Step 4: Smoke fixture path**

Run: `cargo run -- tui --fixture fixtures/approval_destructive.json --once`

Expected: deterministic fixture cockpit still renders approval/tool demo state.

---

## Self-review

- Spec coverage: runtime snapshot, backend probes, system metrics, fixture isolation, config loading, and tests are all covered.
- Placeholder scan: no `TBD`, `TODO`, or unresolved implementation instructions remain.
- Type consistency: `RuntimeSnapshot`, `Reported<T>`, `CockpitState::from_runtime`, and backend snapshot functions are named consistently across tasks.
- Commit steps intentionally omitted because the current workspace is not a git repository and commits require explicit user request.
