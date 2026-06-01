use crate::domain::{CockpitMode, CockpitState, EventKind};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PipelineStage {
    Input,
    Wake,
    Processing,
    Tooling,
    Approval,
    Responding,
    Speaking,
    Idle,
}

impl PipelineStage {
    pub fn from_event(kind: &EventKind) -> Self {
        match kind {
            EventKind::Listening => Self::Input,
            EventKind::WakeWord => Self::Wake,
            EventKind::Processing => Self::Processing,
            EventKind::ToolStart | EventKind::ToolFinish => Self::Tooling,
            EventKind::ApprovalRequested | EventKind::ApprovalRouted => Self::Approval,
            EventKind::ResponseStream => Self::Responding,
            EventKind::Speaking => Self::Speaking,
            EventKind::Idle => Self::Idle,
            EventKind::Warning | EventKind::Error => Self::Idle,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CockpitProjection {
    pub mode: CockpitMode,
    pub voice_label: String,
    pub tools_active: u16,
}

pub fn project_events(state: &CockpitState) -> CockpitProjection {
    let mut mode = if state.capabilities.reachable {
        CockpitMode::Idle
    } else {
        CockpitMode::Disconnected
    };
    let mut tools_active = 0u16;
    for event in &state.events {
        match PipelineStage::from_event(&event.kind) {
            PipelineStage::Input => {
                mode = CockpitMode::Listening;
            }
            PipelineStage::Wake => {
                mode = CockpitMode::WakeDetected;
            }
            PipelineStage::Processing => {
                mode = CockpitMode::Processing;
            }
            PipelineStage::Tooling if matches!(event.kind, EventKind::ToolStart) => {
                mode = CockpitMode::ToolActive;
                tools_active = tools_active.saturating_add(1);
            }
            PipelineStage::Tooling => {
                tools_active = tools_active.saturating_sub(1);
            }
            PipelineStage::Approval if matches!(event.kind, EventKind::ApprovalRequested) => {
                mode = CockpitMode::ApprovalNeeded;
            }
            PipelineStage::Approval => {
                mode = CockpitMode::Processing;
            }
            PipelineStage::Responding => {
                mode = CockpitMode::Processing;
            }
            PipelineStage::Speaking => {
                mode = CockpitMode::Speaking;
            }
            PipelineStage::Idle if matches!(event.kind, EventKind::Idle) => {
                if state.pending_approval.is_none() {
                    mode = CockpitMode::Idle;
                }
            }
            PipelineStage::Idle if matches!(event.kind, EventKind::Warning) => {}
            PipelineStage::Idle => {
                mode = CockpitMode::Error;
            }
        }
    }

    if state.pending_approval.is_some() {
        mode = CockpitMode::ApprovalNeeded;
    }

    let voice_label = match mode {
        CockpitMode::Listening | CockpitMode::WakeDetected => "LISTENING",
        CockpitMode::Speaking => "SPEAKING",
        CockpitMode::ApprovalNeeded => "GATED",
        CockpitMode::Disconnected => "KEYBOARD",
        _ => "ARMED",
    }
    .to_string();

    CockpitProjection {
        mode,
        voice_label,
        tools_active,
    }
}

pub fn apply_projection(state: &mut CockpitState) {
    if is_runtime_snapshot_state(state) {
        return;
    }
    let projection = project_events(state);
    state.mode = projection.mode;
    state.voice_label = projection.voice_label;
    state.tools_active = projection.tools_active;
}

fn is_runtime_snapshot_state(state: &CockpitState) -> bool {
    if state.backend == crate::domain::BackendKind::Fake {
        return false;
    }
    if state.pending_approval.is_some() {
        return false;
    }
    // Runtime snapshot state: either a single status event or events from backend logs
    if state.events.is_empty() {
        return true;
    }
    if state.events.len() == 1
        && state
            .events
            .first()
            .is_some_and(|event| event.label.starts_with("live backend"))
    {
        return true;
    }
    // Events sourced from backend logs (not fixture events)
    state.events.iter().all(|event| {
        !event.label.contains("tool:web.search")
            && !event.label.contains("tool:weather")
            && !event.label.contains("tool:terminal")
    })
}

#[cfg(test)]
pub fn push_event(state: &mut CockpitState, event: crate::domain::CockpitEvent) {
    state.events.push(event);
    apply_projection(state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CockpitEvent, CockpitState, EventKind};

    #[test]
    fn reducer_derives_tool_and_approval_mode_from_events() {
        let mut state = CockpitState::fake_listening();
        state.pending_approval = None;
        state.events.clear();
        push_event(
            &mut state,
            CockpitEvent::now(EventKind::ToolStart, "tool:terminal"),
        );
        assert_eq!(state.mode, CockpitMode::ToolActive);
        assert_eq!(state.tools_active, 1);
        push_event(
            &mut state,
            CockpitEvent::now(EventKind::ApprovalRequested, "approval:terminal"),
        );
        assert_eq!(state.mode, CockpitMode::ApprovalNeeded);
    }
}
