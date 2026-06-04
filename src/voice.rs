#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum VoiceState {
    Idle,
    Listening,
    Capturing,
    SilencePending,
    Dispatching,
    BackendProcessing,
    WaitingPermission,
    Speaking,
    Interrupted,
    ErrorRecovery,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum VoiceEvent {
    Arm,
    SpeechStarted,
    SilenceDetected,
    TranscriptReady { text: String },
    BackendAccepted,
    BackendProgress,
    PermissionRequested,
    PermissionResolved { approved: bool },
    FinalResponseReady,
    TtsStarted,
    TtsEnded,
    UserInterrupted,
    Error { summary: String },
    Reset,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VoiceMachine {
    state: VoiceState,
    last_transcript: Option<String>,
    last_error: Option<String>,
}

impl Default for VoiceMachine {
    fn default() -> Self {
        Self {
            state: VoiceState::Idle,
            last_transcript: None,
            last_error: None,
        }
    }
}

impl VoiceMachine {
    pub fn state(&self) -> VoiceState {
        self.state
    }

    pub fn last_transcript(&self) -> Option<&str> {
        self.last_transcript.as_deref()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn apply(&mut self, event: VoiceEvent) -> VoiceState {
        match event {
            VoiceEvent::Arm | VoiceEvent::Reset => {
                self.last_error = None;
                self.state = VoiceState::Listening;
            }
            VoiceEvent::SpeechStarted => {
                self.state = VoiceState::Capturing;
            }
            VoiceEvent::SilenceDetected => {
                if matches!(self.state, VoiceState::Capturing) {
                    self.state = VoiceState::SilencePending;
                }
            }
            VoiceEvent::TranscriptReady { text } => {
                let text = text.trim();
                if text.is_empty() {
                    self.state = VoiceState::Listening;
                } else {
                    self.last_transcript = Some(text.to_string());
                    self.state = VoiceState::Dispatching;
                }
            }
            VoiceEvent::BackendAccepted | VoiceEvent::BackendProgress => {
                self.state = VoiceState::BackendProcessing;
            }
            VoiceEvent::PermissionRequested => {
                self.state = VoiceState::WaitingPermission;
            }
            VoiceEvent::PermissionResolved { approved } => {
                self.state = if approved {
                    VoiceState::BackendProcessing
                } else {
                    VoiceState::Listening
                };
            }
            VoiceEvent::FinalResponseReady | VoiceEvent::TtsStarted => {
                self.state = VoiceState::Speaking;
            }
            VoiceEvent::TtsEnded => {
                self.state = VoiceState::Listening;
            }
            VoiceEvent::UserInterrupted => {
                self.state = match self.state {
                    VoiceState::Speaking => VoiceState::Interrupted,
                    _ => VoiceState::Capturing,
                };
            }
            VoiceEvent::Error { summary } => {
                self.last_error = Some(summary);
                self.state = VoiceState::ErrorRecovery;
            }
        }
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_turn_returns_to_listening_after_tts_ends() {
        let mut machine = VoiceMachine::default();

        assert_eq!(machine.apply(VoiceEvent::Arm), VoiceState::Listening);
        assert_eq!(
            machine.apply(VoiceEvent::SpeechStarted),
            VoiceState::Capturing
        );
        assert_eq!(
            machine.apply(VoiceEvent::SilenceDetected),
            VoiceState::SilencePending
        );
        assert_eq!(
            machine.apply(VoiceEvent::TranscriptReady {
                text: "open the dashboard".to_string()
            }),
            VoiceState::Dispatching
        );
        assert_eq!(machine.last_transcript(), Some("open the dashboard"));
        assert_eq!(
            machine.apply(VoiceEvent::BackendAccepted),
            VoiceState::BackendProcessing
        );
        assert_eq!(
            machine.apply(VoiceEvent::BackendProgress),
            VoiceState::BackendProcessing
        );
        assert_eq!(
            machine.apply(VoiceEvent::FinalResponseReady),
            VoiceState::Speaking
        );
        assert_eq!(machine.apply(VoiceEvent::TtsEnded), VoiceState::Listening);
    }

    #[test]
    fn user_can_interrupt_speaking_and_start_capturing_again() {
        let mut machine = VoiceMachine::default();
        machine.apply(VoiceEvent::Arm);
        machine.apply(VoiceEvent::FinalResponseReady);

        assert_eq!(
            machine.apply(VoiceEvent::UserInterrupted),
            VoiceState::Interrupted
        );
        assert_eq!(
            machine.apply(VoiceEvent::SpeechStarted),
            VoiceState::Capturing
        );
    }

    #[test]
    fn empty_transcript_returns_to_listening_without_dispatch() {
        let mut machine = VoiceMachine::default();
        machine.apply(VoiceEvent::Arm);
        machine.apply(VoiceEvent::SpeechStarted);
        machine.apply(VoiceEvent::SilenceDetected);

        assert_eq!(
            machine.apply(VoiceEvent::TranscriptReady {
                text: "   ".to_string()
            }),
            VoiceState::Listening
        );
        assert_eq!(machine.last_transcript(), None);
    }

    #[test]
    fn permission_resolution_resumes_or_returns_to_listening() {
        let mut machine = VoiceMachine::default();
        machine.apply(VoiceEvent::Arm);
        machine.apply(VoiceEvent::BackendAccepted);
        assert_eq!(
            machine.apply(VoiceEvent::PermissionRequested),
            VoiceState::WaitingPermission
        );
        assert_eq!(
            machine.apply(VoiceEvent::PermissionResolved { approved: true }),
            VoiceState::BackendProcessing
        );
        machine.apply(VoiceEvent::PermissionRequested);
        assert_eq!(
            machine.apply(VoiceEvent::PermissionResolved { approved: false }),
            VoiceState::Listening
        );
    }
}
