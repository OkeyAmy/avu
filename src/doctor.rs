use crate::{
    backend::{adapter_for, command_exists},
    cli::DoctorArgs,
    config::{AvuConfig, AvuPaths},
    runtime, voice_activation,
};
use anyhow::Result;
use serde::Serialize;
use std::process::{Command, Stdio};

#[derive(Debug, Serialize)]
struct DoctorReport {
    ok: bool,
    config_home: String,
    backend_label: String,
    capability: crate::domain::CapabilitySnapshot,
    checks: Vec<DoctorCheck>,
}

#[derive(Debug, Serialize)]
struct DoctorCheck {
    name: &'static str,
    ok: bool,
    detail: String,
}

pub fn run(args: DoctorArgs) -> Result<()> {
    let paths = AvuPaths::discover();
    let adapter = adapter_for(args.backend);
    let capability = adapter.probe();
    let mut checks = vec![
        DoctorCheck {
            name: "avu_config_home",
            ok: true,
            detail: paths.home.display().to_string(),
        },
        DoctorCheck {
            name: "backend_reachable",
            ok: capability.reachable,
            detail: format!("{} reachable={}", adapter.label(), capability.reachable),
        },
        DoctorCheck {
            name: "capability_known",
            ok: true,
            detail: "capability probe completed".to_string(),
        },
        DoctorCheck {
            name: "approval_policy_owned_by_backend",
            ok: true,
            detail: if capability.approvals {
                "backend reports approval capability; Avu will only surface and route responses"
                    .to_string()
            } else if capability.reachable {
                "backend approval capability is unavailable; Avu disables live approval controls safely"
                    .to_string()
            } else {
                "backend unreachable; approval controls disabled safely".to_string()
            },
        },
        DoctorCheck {
            name: "wake_fallback",
            ok: true,
            detail: "keyboard-only fallback available when mic/wake is unavailable".to_string(),
        },
    ];
    checks.extend(voice_checks(adapter.label(), capability.reachable));
    let ok = checks.iter().all(|check| check.ok);
    let report = DoctorReport {
        ok,
        config_home: paths.home.display().to_string(),
        backend_label: adapter.label().to_string(),
        capability,
        checks,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Avu doctor: {}",
            if report.ok { "ok" } else { "attention needed" }
        );
        println!("Config home: {}", report.config_home);
        println!("Backend: {}", report.backend_label);
        for check in report.checks {
            println!(
                "[{}] {} — {}",
                if check.ok { "ok" } else { "!!" },
                check.name,
                check.detail
            );
        }
        for note in report.capability.notes {
            println!("note: {note}");
        }
    }

    Ok(())
}

fn voice_checks(backend_label: &str, backend_reachable: bool) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();
    checks.push(DoctorCheck {
        name: "avu_native_microphone_capture",
        ok: false,
        detail: "Avu does not capture microphone audio directly yet; use Ctrl+B to hand off to Hermes voice/TUI, or type in Chat mode".to_string(),
    });

    let paths = AvuPaths::discover();
    let avu_config = AvuConfig::load(&paths).unwrap_or_default();
    let hermes_config = (backend_label == "hermes")
        .then(runtime::hermes_config_text)
        .flatten();
    let activation =
        voice_activation::activation_status(&avu_config, backend_label, hermes_config.as_deref());
    checks.push(DoctorCheck {
        name: "backend_voice_config",
        ok: backend_label != "hermes" || activation.backend_voice_available,
        detail: voice_config_detail(&activation),
    });
    checks.push(audio_player_check());
    checks.push(linux_audio_sink_check());
    checks.push(linux_audio_source_check());
    checks.push(hermes_tui_handoff_check(backend_label, backend_reachable));
    checks.push(hermes_tui_voice_command_check(
        backend_label,
        backend_reachable,
    ));
    checks
}

fn voice_config_detail(status: &voice_activation::VoiceActivationStatus) -> String {
    let stt = status.stt_provider.as_deref().unwrap_or("unreported");
    let tts = status.tts_provider.as_deref().unwrap_or("unreported");
    let key = status.record_key.as_deref().unwrap_or("unreported");
    format!(
        "wake_mode={} wake_phrase=`{}` STT={stt} TTS={tts} record_key={key}; {}",
        status.wake_mode,
        status.wake_phrase,
        status.notes.join("; ")
    )
}

fn audio_player_check() -> DoctorCheck {
    let players = ["ffplay", "mpv", "paplay", "aplay", "xdg-open"];
    let found = players
        .iter()
        .copied()
        .filter(|program| command_exists(program))
        .collect::<Vec<_>>();
    DoctorCheck {
        name: "audio_output_player",
        ok: !found.is_empty(),
        detail: if found.is_empty() {
            "no supported audio player found; install ffplay, mpv, paplay, or aplay for spoken output".to_string()
        } else {
            format!("available players: {}", found.join(", "))
        },
    }
}

fn linux_audio_sink_check() -> DoctorCheck {
    if !cfg!(target_os = "linux") {
        return DoctorCheck {
            name: "audio_output_sink",
            ok: true,
            detail: "non-Linux platform; sink probing skipped".to_string(),
        };
    }
    let pactl = command_stdout("pactl", &["list", "short", "sinks"]);
    let wpctl = command_stdout("wpctl", &["status"]);
    let ok = pactl
        .as_deref()
        .is_some_and(|text| text.lines().any(|line| !line.trim().is_empty()))
        || wpctl.as_deref().is_some_and(|text| {
            text.lines()
                .any(|line| line.trim_start().starts_with("Sinks:"))
        });
    DoctorCheck {
        name: "audio_output_sink",
        ok,
        detail: if ok {
            compact_probe_detail("audio sink detected", pactl.as_deref().or(wpctl.as_deref()))
        } else {
            "no PipeWire/PulseAudio sink detected; user may not hear Avu/Hermes speech".to_string()
        },
    }
}

fn linux_audio_source_check() -> DoctorCheck {
    if !cfg!(target_os = "linux") {
        return DoctorCheck {
            name: "audio_input_source",
            ok: true,
            detail: "non-Linux platform; source probing skipped".to_string(),
        };
    }
    let pactl = command_stdout("pactl", &["list", "short", "sources"]);
    let wpctl = command_stdout("wpctl", &["status"]);
    let ok = pactl.as_deref().is_some_and(non_monitor_source_seen)
        || wpctl.as_deref().is_some_and(|text| {
            text.lines()
                .any(|line| line.trim_start().starts_with("Sources:"))
        });
    DoctorCheck {
        name: "audio_input_source",
        ok,
        detail: if ok {
            audio_source_detail(pactl.as_deref(), wpctl.as_deref())
        } else {
            "no microphone/source detected; Hermes STT cannot record user speech".to_string()
        },
    }
}

fn hermes_tui_handoff_check(backend_label: &str, backend_reachable: bool) -> DoctorCheck {
    if backend_label != "hermes" {
        return DoctorCheck {
            name: "hermes_tui_voice_handoff",
            ok: true,
            detail: "not using Hermes backend; handoff check skipped".to_string(),
        };
    }
    if !backend_reachable || !command_exists("hermes") {
        return DoctorCheck {
            name: "hermes_tui_voice_handoff",
            ok: false,
            detail: "Hermes is not reachable; Ctrl+B voice handoff cannot start".to_string(),
        };
    }
    let Some(probe) = command_combined_output("timeout", &["6", "hermes", "--continue", "--tui"])
    else {
        return DoctorCheck {
            name: "hermes_tui_voice_handoff",
            ok: false,
            detail: "could not run `timeout 6 hermes --continue --tui`; install coreutils timeout or test Hermes TUI manually".to_string(),
        };
    };
    let classification = classify_hermes_tui_probe(&probe);
    DoctorCheck {
        name: "hermes_tui_voice_handoff",
        ok: classification.ok,
        detail: classification.detail,
    }
}

fn hermes_tui_voice_command_check(backend_label: &str, backend_reachable: bool) -> DoctorCheck {
    if backend_label != "hermes" {
        return DoctorCheck {
            name: "hermes_tui_voice_command",
            ok: true,
            detail: "not using Hermes backend; voice command probe skipped".to_string(),
        };
    }
    if !backend_reachable || !command_exists("hermes") {
        return DoctorCheck {
            name: "hermes_tui_voice_command",
            ok: false,
            detail: "Hermes is not reachable; `/voice on` cannot be tested".to_string(),
        };
    }
    let Some(probe) = command_combined_output(
        "timeout",
        &[
            "14",
            "bash",
            "-lc",
            "command -v script >/dev/null && script -qfec \"printf '/voice on\\\\r' | hermes --continue --tui\" /dev/null",
        ],
    ) else {
        return DoctorCheck {
            name: "hermes_tui_voice_command",
            ok: false,
            detail: "could not run Hermes `/voice on` pseudo-terminal probe; install bash, timeout, and script".to_string(),
        };
    };
    let classification = classify_hermes_tui_probe(&probe);
    DoctorCheck {
        name: "hermes_tui_voice_command",
        ok: classification.ok,
        detail: if classification.ok {
            "Hermes `/voice on` pseudo-terminal probe did not report known startup/build errors"
                .to_string()
        } else {
            classification.detail
        },
    }
}

struct ProbeClassification {
    ok: bool,
    detail: String,
}

fn classify_hermes_tui_probe(output: &str) -> ProbeClassification {
    let lower = output.to_lowercase();
    if lower.contains("npm install failed") || lower.contains("enotempty") {
        return ProbeClassification {
            ok: false,
            detail: "Hermes TUI dependency install failed; repair with `cd ~/.hermes/hermes-agent/ui-tui && rm -rf node_modules package-lock.json && npm install`".to_string(),
        };
    }
    if lower.contains("tui build failed")
        || lower.contains("could not resolve \"yoga-layout\"")
        || lower.contains("could not resolve 'yoga-layout'")
    {
        return ProbeClassification {
            ok: false,
            detail: "Hermes TUI voice build failed resolving `yoga-layout`; repair Hermes ui-tui dependencies or update Hermes before AVU Ctrl+B voice can record".to_string(),
        };
    }
    if lower.contains("cannot read properties of null") && lower.contains("usestate")
        || lower.contains("reactsharedinternals.h.usestate")
    {
        return ProbeClassification {
            ok: false,
            detail: "Hermes TUI voice runtime failed inside React/useState; update or repair Hermes ui-tui dependencies before AVU Ctrl+B voice can record".to_string(),
        };
    }
    if lower.contains("no session found") {
        return ProbeClassification {
            ok: false,
            detail: "Hermes TUI could not resume a session; Avu should use `hermes --continue --tui` and user can run `hermes sessions list`".to_string(),
        };
    }
    if lower.contains("command not found") || lower.contains("no such file") {
        return ProbeClassification {
            ok: false,
            detail: compact_probe_detail("Hermes TUI launch failed", Some(output)),
        };
    }
    ProbeClassification {
        ok: true,
        detail: if output.trim().is_empty() {
            "Hermes TUI launch probe produced no startup error before timeout; Ctrl+B handoff should open Hermes voice/TUI".to_string()
        } else {
            compact_probe_detail(
                "Hermes TUI launch probe did not report known startup errors",
                Some(output),
            )
        },
    }
}

fn non_monitor_source_seen(text: &str) -> bool {
    text.lines().any(|line| {
        let lower = line.to_lowercase();
        !line.trim().is_empty() && !lower.contains(".monitor")
    })
}

fn audio_source_detail(pactl: Option<&str>, wpctl: Option<&str>) -> String {
    if let Some(line) = pactl.and_then(first_non_monitor_source_line) {
        return format!("audio input source detected: {}", compact(line, 180));
    }
    compact_probe_detail("audio input source detected", wpctl)
}

fn first_non_monitor_source_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| {
        let lower = line.to_lowercase();
        !line.is_empty() && !lower.contains(".monitor")
    })
}

fn compact_probe_detail(prefix: &str, text: Option<&str>) -> String {
    let sample = text
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no details reported");
    format!("{prefix}: {}", compact(sample, 180))
}

fn compact(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut output: String = input.chars().take(max_chars.saturating_sub(1)).collect();
    output.push('…');
    output
}

fn command_stdout(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

fn command_combined_output(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hermes_tui_probe_reports_npm_install_failure_with_repair_command() {
        let result = classify_hermes_tui_probe(
            "Installing TUI dependencies…\nnpm install failed.\nENOTEMPTY",
        );

        assert!(!result.ok);
        assert!(
            result
                .detail
                .contains("rm -rf node_modules package-lock.json")
        );
    }

    #[test]
    fn hermes_tui_probe_reports_voice_build_failure() {
        let result =
            classify_hermes_tui_probe("TUI build failed. ERROR: Could not resolve \"yoga-layout\"");

        assert!(!result.ok);
        assert!(result.detail.contains("yoga-layout"));
    }

    #[test]
    fn hermes_tui_probe_reports_react_runtime_failure() {
        let result = classify_hermes_tui_probe(
            "ERROR Cannot read properties of null (reading 'useState') ReactSharedInternals.H.useState",
        );

        assert!(!result.ok);
        assert!(result.detail.contains("React/useState"));
    }

    #[test]
    fn hermes_tui_probe_reports_named_session_failure() {
        let result = classify_hermes_tui_probe("No session found matching 'avu-tui'.");

        assert!(!result.ok);
        assert!(result.detail.contains("hermes --continue --tui"));
    }

    #[test]
    fn hermes_tui_probe_passes_empty_timeout_output() {
        let result = classify_hermes_tui_probe("");

        assert!(result.ok);
        assert!(result.detail.contains("no startup error"));
    }

    #[test]
    fn source_detection_ignores_monitor_only_sources() {
        assert!(!non_monitor_source_seen(
            "52\talsa_output.pci.monitor\tPipeWire\ts16le"
        ));
        assert!(non_monitor_source_seen(
            "53\talsa_input.pci-0000_00_05.0.analog-stereo\tPipeWire\ts16le"
        ));
    }

    #[test]
    fn source_detail_prefers_real_input_over_monitor() {
        let detail = audio_source_detail(
            Some(
                "52\talsa_output.pci.monitor\tPipeWire\ts16le\n53\talsa_input.pci-0000_00_05.0.analog-stereo\tPipeWire\ts16le",
            ),
            None,
        );

        assert!(detail.contains("alsa_input"));
        assert!(!detail.contains("alsa_output.pci.monitor"));
    }
}
