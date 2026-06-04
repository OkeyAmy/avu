use crate::config::AvuConfig;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VoiceActivationStatus {
    pub wake_mode: String,
    pub wake_phrase: String,
    pub keyboard_activation_available: bool,
    pub backend_voice_available: bool,
    pub record_key: Option<String>,
    pub stt_provider: Option<String>,
    pub tts_provider: Option<String>,
    pub notes: Vec<String>,
}

pub fn activation_status(
    avu_config: &AvuConfig,
    backend_label: &str,
    backend_voice_config: Option<&str>,
) -> VoiceActivationStatus {
    let voice = backend_voice_config.and_then(parse_backend_voice_config);
    let keyboard_activation_available = matches!(
        avu_config.wake_mode.as_str(),
        "keyboard_only" | "push_to_talk" | "local_wake_word"
    );
    let mut notes = Vec::new();

    if keyboard_activation_available {
        notes.push(format!(
            "Keyboard wake is available with phrase `{}`",
            avu_config.wake_phrase
        ));
    } else {
        notes.push("Wake activation is disabled in Avu config".to_string());
    }

    let backend_voice_available = voice
        .as_ref()
        .is_some_and(|voice| voice.stt_enabled && voice.tts_provider.is_some());
    if backend_voice_available {
        notes.push(format!(
            "{backend_label} owns microphone capture, STT, TTS, and spoken replies"
        ));
    } else {
        notes.push(format!(
            "{backend_label} voice is not fully configured; verify Hermes voice extras, STT, TTS, and audio device"
        ));
    }

    SelfStatus::from_parts(
        avu_config,
        voice,
        keyboard_activation_available,
        backend_voice_available,
        notes,
    )
    .into_status()
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct BackendVoiceConfig {
    stt_enabled: bool,
    stt_provider: Option<String>,
    tts_provider: Option<String>,
    record_key: Option<String>,
}

struct SelfStatus<'a> {
    avu_config: &'a AvuConfig,
    voice: Option<BackendVoiceConfig>,
    keyboard_activation_available: bool,
    backend_voice_available: bool,
    notes: Vec<String>,
}

impl<'a> SelfStatus<'a> {
    fn from_parts(
        avu_config: &'a AvuConfig,
        voice: Option<BackendVoiceConfig>,
        keyboard_activation_available: bool,
        backend_voice_available: bool,
        notes: Vec<String>,
    ) -> Self {
        Self {
            avu_config,
            voice,
            keyboard_activation_available,
            backend_voice_available,
            notes,
        }
    }

    fn into_status(self) -> VoiceActivationStatus {
        VoiceActivationStatus {
            wake_mode: self.avu_config.wake_mode.clone(),
            wake_phrase: self.avu_config.wake_phrase.clone(),
            keyboard_activation_available: self.keyboard_activation_available,
            backend_voice_available: self.backend_voice_available,
            record_key: self
                .voice
                .as_ref()
                .and_then(|voice| voice.record_key.clone()),
            stt_provider: self
                .voice
                .as_ref()
                .and_then(|voice| voice.stt_provider.clone()),
            tts_provider: self
                .voice
                .as_ref()
                .and_then(|voice| voice.tts_provider.clone()),
            notes: self.notes,
        }
    }
}

fn parse_backend_voice_config(text: &str) -> Option<BackendVoiceConfig> {
    Some(BackendVoiceConfig {
        stt_enabled: yaml_nested_bool(text, "stt", "enabled").unwrap_or(true),
        stt_provider: yaml_nested_value(text, "stt", "provider"),
        tts_provider: yaml_nested_value(text, "tts", "provider"),
        record_key: yaml_nested_value(text, "voice", "record_key"),
    })
}

fn yaml_nested_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') {
            in_section = trimmed.trim_end_matches(':') == section;
            continue;
        }
        if in_section
            && let Some((candidate, value)) = trimmed.split_once(':')
            && candidate.trim() == key
        {
            return clean_yaml_value(value);
        }
    }
    None
}

fn yaml_nested_bool(text: &str, section: &str, key: &str) -> Option<bool> {
    yaml_nested_value(text, section, key).and_then(|value| match value.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    })
}

fn clean_yaml_value(value: &str) -> Option<String> {
    let cleaned = value
        .trim()
        .trim_matches(|ch| matches!(ch, '"' | '\''))
        .to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_configured_hermes_voice_readiness() {
        let avu = AvuConfig::default();
        let hermes_config = r#"
voice:
  record_key: ctrl+b
stt:
  enabled: true
  provider: local
tts:
  provider: edge
"#;

        let status = activation_status(&avu, "Hermes", Some(hermes_config));

        assert!(status.keyboard_activation_available);
        assert!(status.backend_voice_available);
        assert_eq!(status.record_key.as_deref(), Some("ctrl+b"));
        assert_eq!(status.stt_provider.as_deref(), Some("local"));
        assert_eq!(status.tts_provider.as_deref(), Some("edge"));
    }

    #[test]
    fn missing_backend_voice_config_reports_keyboard_only_path() {
        let avu = AvuConfig::default();

        let status = activation_status(&avu, "Hermes", None);

        assert!(status.keyboard_activation_available);
        assert!(!status.backend_voice_available);
        assert!(
            status
                .notes
                .iter()
                .any(|note| note.contains("voice is not fully configured"))
        );
    }
}
