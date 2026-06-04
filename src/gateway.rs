#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GatewayResolution {
    pub default_profile: Option<String>,
    pub running_profile: Option<String>,
    pub selected_profile: Option<String>,
    pub status: GatewayStatus,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GatewayStatus {
    DefaultRunning,
    DefaultStopped,
    OtherProfileRunning,
    Conflict,
    Unknown,
}

pub fn resolve_hermes_gateway(
    default_profile: Option<&str>,
    gateway_status: Option<&str>,
) -> GatewayResolution {
    let default_profile = default_profile
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let running_profile = gateway_status.and_then(running_gateway_profile);
    let text = gateway_status.unwrap_or_default();
    let lower = text.to_lowercase();
    let conflict = lower.contains("token already in use") || lower.contains("already in use");
    let default_running = lower.contains("user gateway service is running")
        || default_profile
            .as_deref()
            .is_some_and(|profile| running_profile.as_deref() == Some(profile));
    let default_stopped = lower.contains("user gateway service is stopped")
        || lower.contains("active: inactive (dead)");

    let status = if conflict {
        GatewayStatus::Conflict
    } else if default_running {
        GatewayStatus::DefaultRunning
    } else if running_profile.is_some() && default_stopped {
        GatewayStatus::OtherProfileRunning
    } else if default_stopped {
        GatewayStatus::DefaultStopped
    } else {
        GatewayStatus::Unknown
    };

    let selected_profile = match status {
        GatewayStatus::DefaultRunning | GatewayStatus::DefaultStopped => default_profile.clone(),
        GatewayStatus::OtherProfileRunning | GatewayStatus::Conflict | GatewayStatus::Unknown => {
            default_profile.clone()
        }
    };

    let mut notes = Vec::new();
    match status {
        GatewayStatus::DefaultRunning => notes.push("Hermes default gateway is running".to_string()),
        GatewayStatus::DefaultStopped => notes.push(
            "Hermes default gateway is stopped; Avu will not silently switch profiles".to_string(),
        ),
        GatewayStatus::OtherProfileRunning => {
            if let Some(profile) = running_profile.as_deref() {
                notes.push(format!(
                    "Hermes profile `{profile}` is running, but Avu will not silently switch away from the configured/default profile"
                ));
            }
        }
        GatewayStatus::Conflict => {
            notes.push("Hermes gateway conflict detected; resolve token/process ownership before relying on gateway delivery".to_string());
            if let Some(profile) = running_profile.as_deref() {
                notes.push(format!(
                    "Running profile `{profile}` is reported for diagnostics only; Avu will not silently select it"
                ));
            }
        }
        GatewayStatus::Unknown => notes.push(
            "Hermes gateway status is unavailable or unrecognized; run `hermes gateway status --all`".to_string(),
        ),
    }

    GatewayResolution {
        default_profile,
        running_profile,
        selected_profile,
        status,
        notes,
    }
}

fn running_gateway_profile(gateway_text: &str) -> Option<String> {
    gateway_text.lines().find_map(|line| {
        let trimmed = line.trim();
        let rest = trimmed.strip_prefix('✓')?.trim();
        let profile = rest.split_whitespace().next()?.trim();
        if profile == "User" || profile == "Systemd" || profile.is_empty() {
            None
        } else {
            Some(profile.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_running_selects_configured_default() {
        let resolution = resolve_hermes_gateway(
            Some("default"),
            Some("✓ User gateway service is running\nOther profiles:\n  ✓ portal — PID 123"),
        );

        assert_eq!(resolution.status, GatewayStatus::DefaultRunning);
        assert_eq!(resolution.selected_profile.as_deref(), Some("default"));
    }

    #[test]
    fn other_running_profile_does_not_replace_default() {
        let status = r#"
✗ User gateway service is stopped
Other profiles:
  ✓ portal           — PID 115002
"#;

        let resolution = resolve_hermes_gateway(Some("default"), Some(status));

        assert_eq!(resolution.status, GatewayStatus::OtherProfileRunning);
        assert_eq!(resolution.running_profile.as_deref(), Some("portal"));
        assert_eq!(resolution.selected_profile.as_deref(), Some("default"));
        assert!(
            resolution
                .notes
                .iter()
                .any(|note| note.contains("will not silently switch"))
        );
    }

    #[test]
    fn token_conflict_reports_running_profile_only_for_diagnostics() {
        let status = r#"
Active: inactive (dead)
✗ User gateway service is stopped
Recent gateway health:
  ⚠ telegram: Telegram bot token already in use (PID 741). Stop the other gateway first.
Other profiles:
  ✓ portal           — PID 741
"#;

        let resolution = resolve_hermes_gateway(Some("default"), Some(status));

        assert_eq!(resolution.status, GatewayStatus::Conflict);
        assert_eq!(resolution.running_profile.as_deref(), Some("portal"));
        assert_eq!(resolution.selected_profile.as_deref(), Some("default"));
        assert!(
            resolution
                .notes
                .iter()
                .any(|note| note.contains("will not silently select"))
        );
    }
}
