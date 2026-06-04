use crate::domain::{ApprovalRequest, Severity};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Intent {
    Status,
    ListSessions,
    Pause,
    Resume,
    Interrupt,
    Approve,
    Reject,
    ConfirmApprove,
    SwitchSession(u8),
    Mute,
    Unknown(String),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ApprovalGate {
    NoPendingApproval,
    RouteReject { approval_id: String },
    RouteApprove { approval_id: String },
    RequireSecondConfirmation { summary: String },
    UnsupportedIntent,
}

pub fn parse_intent(input: &str) -> Intent {
    let parsed = parse_intent_with_wake_phrase(input, "hey avu");
    if matches!(parsed, Intent::Unknown(_)) {
        parse_intent_with_wake_phrase(input, "avu")
    } else {
        parsed
    }
}

pub fn parse_intent_with_wake_phrase(input: &str, wake_phrase: &str) -> Intent {
    let without_wake = strip_wake_phrase(input.trim(), wake_phrase);
    let normalized = input
        .trim()
        .to_lowercase()
        .replace([',', '.'], "")
        .trim()
        .to_string();
    let normalized = without_wake
        .unwrap_or(normalized.as_str())
        .to_lowercase()
        .replace([',', '.'], "")
        .trim()
        .to_string();

    match normalized.as_str() {
        "status" | "what is happening" => Intent::Status,
        "what is running" | "sessions" | "list sessions" => Intent::ListSessions,
        "pause" | "pause run" => Intent::Pause,
        "resume" | "resume run" => Intent::Resume,
        "interrupt" | "stop" => Intent::Interrupt,
        "approve" => Intent::Approve,
        "reject" | "deny" => Intent::Reject,
        "confirm approve" | "confirm approval" => Intent::ConfirmApprove,
        "mute" | "mute microphone" => Intent::Mute,
        other if other.starts_with("switch to session ") => {
            let number = other
                .trim_start_matches("switch to session ")
                .parse()
                .unwrap_or(1);
            Intent::SwitchSession(number)
        }
        _ => Intent::Unknown(normalized),
    }
}

fn strip_wake_phrase<'a>(input: &'a str, wake_phrase: &str) -> Option<&'a str> {
    let phrase = wake_phrase.trim();
    if phrase.is_empty() {
        return None;
    }
    let lower_input = input.to_lowercase();
    let lower_phrase = phrase.to_lowercase();
    let rest = lower_input.strip_prefix(&lower_phrase)?;
    if rest.is_empty() {
        return Some("");
    }
    let first = rest.chars().next()?;
    if first.is_whitespace() || matches!(first, ',' | '.' | ':' | ';' | '!' | '?') {
        Some(&input[phrase.len()..])
    } else {
        None
    }
}

pub fn approval_gate(
    intent: &Intent,
    pending: Option<&ApprovalRequest>,
    confirmation_armed: bool,
) -> ApprovalGate {
    let Some(approval) = pending else {
        return match intent {
            Intent::Approve | Intent::Reject | Intent::ConfirmApprove => {
                ApprovalGate::NoPendingApproval
            }
            _ => ApprovalGate::UnsupportedIntent,
        };
    };

    match intent {
        Intent::Reject => ApprovalGate::RouteReject {
            approval_id: approval.id.clone(),
        },
        Intent::Approve => {
            if needs_second_confirmation(approval) && !confirmation_armed {
                ApprovalGate::RequireSecondConfirmation {
                    summary: approval.summary.clone(),
                }
            } else {
                ApprovalGate::RouteApprove {
                    approval_id: approval.id.clone(),
                }
            }
        }
        Intent::ConfirmApprove => ApprovalGate::RouteApprove {
            approval_id: approval.id.clone(),
        },
        _ => ApprovalGate::UnsupportedIntent,
    }
}

fn needs_second_confirmation(approval: &ApprovalRequest) -> bool {
    approval.requires_second_confirmation
        || matches!(approval.severity, Severity::High | Severity::Destructive)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ApprovalRequest, BackendKind};

    #[test]
    fn parses_wake_prefixed_commands() {
        assert_eq!(parse_intent("Hey Avu, interrupt"), Intent::Interrupt);
        assert_eq!(parse_intent("avu confirm approve"), Intent::ConfirmApprove);
    }

    #[test]
    fn parses_configured_wake_phrase_commands() {
        assert_eq!(
            parse_intent_with_wake_phrase("Computer, status", "computer"),
            Intent::Status
        );
        assert_eq!(
            parse_intent_with_wake_phrase("Computerized status", "computer"),
            Intent::Unknown("computerized status".to_string())
        );
    }

    #[test]
    fn destructive_approval_requires_confirmation() {
        let approval = ApprovalRequest {
            id: "a1".to_string(),
            backend: BackendKind::Hermes,
            summary: "run destructive command".to_string(),
            command: Some("rm -rf build-cache".to_string()),
            severity: Severity::Destructive,
            requires_second_confirmation: true,
        };
        assert_eq!(
            approval_gate(&Intent::Approve, Some(&approval), false),
            ApprovalGate::RequireSecondConfirmation {
                summary: "run destructive command".to_string()
            }
        );
        assert_eq!(
            approval_gate(&Intent::ConfirmApprove, Some(&approval), true),
            ApprovalGate::RouteApprove {
                approval_id: "a1".to_string()
            }
        );
    }
}
