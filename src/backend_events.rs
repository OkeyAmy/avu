use crate::domain::EventKind;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BackendTurnContext {
    pub voice_mode: bool,
    pub tts_expected: bool,
    pub progress_updates: bool,
    pub turn_id: String,
}

impl BackendTurnContext {
    pub fn text(turn_id: impl Into<String>) -> Self {
        Self {
            voice_mode: false,
            tts_expected: false,
            progress_updates: false,
            turn_id: turn_id.into(),
        }
    }

    pub fn voice(turn_id: impl Into<String>) -> Self {
        Self {
            voice_mode: true,
            tts_expected: true,
            progress_updates: true,
            turn_id: turn_id.into(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BackendTurnRequest {
    pub prompt: String,
    pub context: BackendTurnContext,
}

impl BackendTurnRequest {
    pub fn text(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            context: BackendTurnContext::text("text-turn"),
        }
    }

    pub fn voice(prompt: impl Into<String>, turn_id: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            context: BackendTurnContext::voice(turn_id),
        }
    }

    pub fn backend_prompt(&self) -> String {
        if !self.context.voice_mode {
            return self.prompt.clone();
        }

        format!(
            "<avu_turn_context voice_mode=\"true\" tts_expected=\"{}\" progress_updates=\"{}\" turn_id=\"{}\">\n\
You are being used through Avu voice mode. Keep full Hermes/OpenClaw tool, agent, permission, and gateway access. Respond conversationally for spoken playback, give concise progress updates when useful, and preserve normal backend safety/approval behavior.\n\
</avu_turn_context>\n{}",
            self.context.tts_expected,
            self.context.progress_updates,
            escape_attr(&self.context.turn_id),
            self.prompt
        )
    }
}

fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BackendEventClass {
    Progress,
    ToolStarted,
    ToolFinished,
    PermissionRequested,
    PermissionResolved,
    FinalResponse,
    SpeakingStarted,
    SpeakingEnded,
    Listening,
    Warning,
    Error,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BackendEvent {
    pub class: BackendEventClass,
    pub label: String,
}

pub fn classify_backend_event(label: &str) -> Option<BackendEventClass> {
    let lower = label.to_lowercase();
    if lower.contains("repeated_exact_failure_warning") {
        Some(BackendEventClass::Warning)
    } else if lower.contains("error") || lower.contains("failed") || lower.contains("exception") {
        Some(BackendEventClass::Error)
    } else if lower.contains("warning") || lower.contains("warn") {
        Some(BackendEventClass::Warning)
    } else if lower.contains("tts ended")
        || lower.contains("audio playback ended")
        || lower.contains("playback complete")
    {
        Some(BackendEventClass::SpeakingEnded)
    } else if lower.contains("voice recording")
        || lower.contains("recording started")
        || lower.contains("listening")
        || lower.contains("awaiting audio")
        || lower.contains("ready for next turn")
    {
        Some(BackendEventClass::Listening)
    } else if lower.contains("tts")
        || lower.contains("speaking")
        || lower.contains("audio playback")
    {
        Some(BackendEventClass::SpeakingStarted)
    } else if lower.contains("permission resolved")
        || lower.contains("approval routed")
        || lower.contains("approved")
        || lower.contains("denied")
    {
        Some(BackendEventClass::PermissionResolved)
    } else if lower.contains("approval")
        || lower.contains("approve")
        || lower.contains("permission requested")
        || lower.contains("requires permission")
    {
        Some(BackendEventClass::PermissionRequested)
    } else if lower.contains("tool")
        && (lower.contains("finish") || lower.contains("complete") || lower.contains("result"))
    {
        Some(BackendEventClass::ToolFinished)
    } else if lower.contains("tool")
        && (lower.contains("start") || lower.contains("call") || lower.contains("dispatch"))
    {
        Some(BackendEventClass::ToolStarted)
    } else if lower.contains("final response")
        || lower.contains("backend replied")
        || lower.contains("response complete")
        || lower.contains("turn complete")
    {
        Some(BackendEventClass::FinalResponse)
    } else if lower.contains("progress")
        || lower.contains("processing")
        || lower.contains("running")
        || lower.contains("session")
    {
        Some(BackendEventClass::Progress)
    } else {
        None
    }
}

pub fn event_kind_for_class(class: &BackendEventClass) -> EventKind {
    match class {
        BackendEventClass::Progress => EventKind::Processing,
        BackendEventClass::ToolStarted => EventKind::ToolStart,
        BackendEventClass::ToolFinished => EventKind::ToolFinish,
        BackendEventClass::PermissionRequested => EventKind::ApprovalRequested,
        BackendEventClass::PermissionResolved => EventKind::ApprovalRouted,
        BackendEventClass::FinalResponse => EventKind::ResponseStream,
        BackendEventClass::SpeakingStarted => EventKind::Speaking,
        BackendEventClass::SpeakingEnded | BackendEventClass::Listening => EventKind::Listening,
        BackendEventClass::Warning => EventKind::Warning,
        BackendEventClass::Error => EventKind::Error,
    }
}

pub fn spoken_progress_for_event(kind: &EventKind, label: &str) -> Option<String> {
    match kind {
        EventKind::Processing => Some("Processing your request".to_string()),
        EventKind::ToolStart => Some(format!("Using {}", compact_tool_label(label))),
        EventKind::ToolFinish => Some(format!("Finished {}", compact_tool_label(label))),
        EventKind::ApprovalRequested => Some("Permission needed".to_string()),
        EventKind::ApprovalRouted => Some("Permission response sent".to_string()),
        EventKind::ResponseStream => Some("Answer ready".to_string()),
        EventKind::Speaking => Some("Speaking response".to_string()),
        EventKind::Listening => Some("Listening for the next turn".to_string()),
        EventKind::Warning => Some("Backend warning".to_string()),
        EventKind::Error => Some("Backend error".to_string()),
        EventKind::WakeWord | EventKind::Idle => None,
    }
}

fn compact_tool_label(label: &str) -> String {
    label
        .split_once(':')
        .map(|(_, rest)| rest)
        .unwrap_or(label)
        .split_whitespace()
        .next()
        .unwrap_or("backend tool")
        .trim_matches(|ch| matches!(ch, '`' | ',' | ';'))
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_turn_preserves_prompt_exactly() {
        let request = BackendTurnRequest::text("/tools && run all backend capabilities");

        assert_eq!(
            request.backend_prompt(),
            "/tools && run all backend capabilities"
        );
    }

    #[test]
    fn voice_turn_adds_context_without_removing_user_prompt_or_tools() {
        let request = BackendTurnRequest::voice("use browser tool then /status", "turn-123");
        let prompt = request.backend_prompt();

        assert!(prompt.contains("voice_mode=\"true\""));
        assert!(prompt.contains("turn_id=\"turn-123\""));
        assert!(
            prompt
                .contains("Keep full Hermes/OpenClaw tool, agent, permission, and gateway access")
        );
        assert!(prompt.ends_with("use browser tool then /status"));
    }

    #[test]
    fn classifies_permission_progress_final_and_speaking_events() {
        assert_eq!(
            classify_backend_event("approval requested for terminal command"),
            Some(BackendEventClass::PermissionRequested)
        );
        assert_eq!(
            classify_backend_event("tool dispatch start: browser.open"),
            Some(BackendEventClass::ToolStarted)
        );
        assert_eq!(
            classify_backend_event("tool result: browser.open complete"),
            Some(BackendEventClass::ToolFinished)
        );
        assert_eq!(
            classify_backend_event("final response ready for turn"),
            Some(BackendEventClass::FinalResponse)
        );
        assert_eq!(
            classify_backend_event("audio playback ended"),
            Some(BackendEventClass::SpeakingEnded)
        );
        assert_eq!(
            classify_backend_event("repeated_exact_failure_warning; count=2"),
            Some(BackendEventClass::Warning)
        );
        assert_eq!(
            classify_backend_event("tools.voice_mode: tts failed: timeout"),
            Some(BackendEventClass::Error)
        );
        assert_eq!(
            classify_backend_event("session failed while processing turn"),
            Some(BackendEventClass::Error)
        );
        assert_eq!(
            classify_backend_event("warning: tts failed with exception"),
            Some(BackendEventClass::Error)
        );
        assert_eq!(
            classify_backend_event("tool loop warning: browser.open failed"),
            Some(BackendEventClass::Error)
        );
    }

    #[test]
    fn creates_concise_spoken_progress_labels() {
        assert_eq!(
            spoken_progress_for_event(&EventKind::ToolStart, "tool:web.search started"),
            Some("Using web.search".to_string())
        );
        assert_eq!(
            spoken_progress_for_event(&EventKind::ApprovalRequested, "approval requested"),
            Some("Permission needed".to_string())
        );
        assert_eq!(
            spoken_progress_for_event(&EventKind::ResponseStream, "final response ready"),
            Some("Answer ready".to_string())
        );
    }
}
