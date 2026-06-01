use crate::{backend::adapter_for, cli::DoctorArgs, config::AvuPaths};
use anyhow::Result;
use serde::Serialize;

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
    let checks = vec![
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
            ok: capability.approvals || !capability.reachable,
            detail: if capability.approvals {
                "backend reports approval capability; Avu will only surface and route responses"
                    .to_string()
            } else if capability.reachable {
                "backend is reachable but approval capability is unavailable; Avu must not show approval controls"
                    .to_string()
            } else {
                "backend unreachable; approval controls disabled".to_string()
            },
        },
        DoctorCheck {
            name: "wake_fallback",
            ok: true,
            detail: "keyboard-only fallback available when mic/wake is unavailable".to_string(),
        },
    ];
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
