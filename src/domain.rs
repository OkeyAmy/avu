use crate::runtime::{Reported, RuntimeSnapshot};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    Fake,
    Hermes,
    OpenClaw,
    Remote,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CockpitMode {
    Idle,
    Listening,
    WakeDetected,
    Processing,
    ToolActive,
    ApprovalNeeded,
    Speaking,
    Disconnected,
    Error,
}

impl CockpitMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Idle => "IDLE",
            Self::Listening => "LISTENING",
            Self::WakeDetected => "WAKE",
            Self::Processing => "PROCESSING",
            Self::ToolActive => "TOOL ACTIVE",
            Self::ApprovalNeeded => "APPROVAL",
            Self::Speaking => "SPEAKING",
            Self::Disconnected => "DISCONNECTED",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Safe,
    Medium,
    High,
    Destructive,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPosture {
    ObserveOnly,
    ObserveNotify,
    RouteSafeControls,
    RouteApprovalResponses,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct CapabilitySnapshot {
    pub backend: BackendKind,
    pub reachable: bool,
    pub events: bool,
    pub approvals: bool,
    pub interrupt: bool,
    pub pause_resume: bool,
    pub sessions_list: bool,
    pub transcript: bool,
    pub wake_available: bool,
    pub permission_posture: PermissionPosture,
    pub notes: Vec<String>,
}

impl CapabilitySnapshot {
    pub fn fake() -> Self {
        Self {
            backend: BackendKind::Fake,
            reachable: true,
            events: true,
            approvals: true,
            interrupt: true,
            pause_resume: true,
            sessions_list: true,
            transcript: true,
            wake_available: false,
            permission_posture: PermissionPosture::RouteApprovalResponses,
            notes: vec!["fixture backend: safe for setup rehearsal".to_string()],
        }
    }

    pub fn missing(name: &str) -> Self {
        Self {
            backend: BackendKind::Missing,
            reachable: false,
            events: false,
            approvals: false,
            interrupt: false,
            pause_resume: false,
            sessions_list: false,
            transcript: false,
            wake_available: false,
            permission_posture: PermissionPosture::ObserveOnly,
            notes: vec![format!("{name} command was not found on PATH")],
        }
    }

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

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ApprovalRequest {
    pub id: String,
    pub backend: BackendKind,
    pub summary: String,
    pub command: Option<String>,
    pub severity: Severity,
    pub requires_second_confirmation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Listening,
    WakeWord,
    Processing,
    ToolStart,
    ToolFinish,
    ApprovalRequested,
    ApprovalRouted,
    ResponseStream,
    Speaking,
    Idle,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct CockpitEvent {
    pub at: DateTime<Utc>,
    pub kind: EventKind,
    pub label: String,
}

impl CockpitEvent {
    pub fn now(kind: EventKind, label: impl Into<String>) -> Self {
        Self {
            at: Utc::now(),
            kind,
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CockpitState {
    pub app_name: String,
    pub mode: CockpitMode,
    pub backend: BackendKind,
    pub backend_label: String,
    pub model_label: String,
    pub voice_label: String,
    pub tools_active: u16,
    pub capabilities: CapabilitySnapshot,
    pub events: Vec<CockpitEvent>,
    pub pending_approval: Option<ApprovalRequest>,
}

impl CockpitState {
    pub fn fake_listening() -> Self {
        let approval = ApprovalRequest {
            id: "approval-fixture-001".to_string(),
            backend: BackendKind::Hermes,
            summary: "Hermes is asking to run a cache cleanup command".to_string(),
            command: Some("rm -rf build-cache".to_string()),
            severity: Severity::Destructive,
            requires_second_confirmation: true,
        };

        Self {
            app_name: "Avu".to_string(),
            mode: CockpitMode::ApprovalNeeded,
            backend: BackendKind::Fake,
            backend_label: "HERMES/FIXTURE".to_string(),
            model_label: "local".to_string(),
            voice_label: "LISTENING".to_string(),
            tools_active: 2,
            capabilities: CapabilitySnapshot::fake(),
            events: vec![
                CockpitEvent::now(EventKind::Listening, "listening"),
                CockpitEvent::now(EventKind::WakeWord, "wake_word"),
                CockpitEvent::now(EventKind::Processing, "processing"),
                CockpitEvent::now(EventKind::ToolStart, "tool:web.search"),
                CockpitEvent::now(EventKind::ToolStart, "tool:weather"),
                CockpitEvent::now(EventKind::ApprovalRequested, "approval:terminal"),
                CockpitEvent::now(EventKind::ResponseStream, "response_stream"),
            ],
            pending_approval: Some(approval),
        }
    }

    pub fn from_runtime(snapshot: RuntimeSnapshot) -> Self {
        let mode = if snapshot.reachable {
            CockpitMode::Idle
        } else {
            CockpitMode::Disconnected
        };
        let capabilities = CapabilitySnapshot::from_runtime(&snapshot);

        // Use real backend log events when available, otherwise a single status event
        let events: Vec<CockpitEvent> = if !snapshot.log_events.is_empty() {
            snapshot.log_events.clone()
        } else {
            let event_label = if snapshot.reachable {
                "live backend reported idle"
            } else {
                "live backend unavailable"
            };
            vec![CockpitEvent::now(
                if snapshot.reachable {
                    EventKind::Idle
                } else {
                    EventKind::Warning
                },
                event_label,
            )]
        };

        Self {
            app_name: "Avu".to_string(),
            mode,
            backend: snapshot.backend.clone(),
            backend_label: snapshot.backend_label.clone(),
            model_label: reported_string(snapshot.model_label.clone()),
            voice_label: reported_bool(
                snapshot.wake_available.as_ref(),
                "WAKE",
                "KEYBOARD",
                "unreported",
            ),
            tools_active: 0,
            capabilities,
            events,
            pending_approval: None,
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
        assert!(state.pending_approval.is_none());
        assert!(
            !state
                .events
                .iter()
                .any(|event| event.label.contains("tool:web.search"))
        );
    }

    #[test]
    fn live_runtime_uses_reported_model() {
        let mut snapshot = RuntimeSnapshot::missing("hermes");
        snapshot.backend = BackendKind::Hermes;
        snapshot.backend_label = "HERMES".to_string();
        snapshot.reachable = true;
        snapshot.model_label = Reported::Value("claude-local".to_string());
        let state = CockpitState::from_runtime(snapshot);
        assert_eq!(state.model_label, "claude-local");
    }
}
