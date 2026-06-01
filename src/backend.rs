use crate::{
    cli::BackendChoice,
    domain::{CapabilitySnapshot, CockpitState},
    runtime,
};
use anyhow::{Context, Result};
use std::{env, path::Path};

pub trait BackendAdapter {
    fn label(&self) -> &'static str;
    fn probe(&self) -> CapabilitySnapshot;
    fn cockpit_state(&self) -> CockpitState;
}

pub struct FakeBackend;
pub struct HermesBackend;
pub struct OpenClawBackend;
pub struct MissingBackend;

impl BackendAdapter for FakeBackend {
    fn label(&self) -> &'static str {
        "fake"
    }

    fn probe(&self) -> CapabilitySnapshot {
        CapabilitySnapshot::fake()
    }

    fn cockpit_state(&self) -> CockpitState {
        CockpitState::fake_listening()
    }
}

impl BackendAdapter for HermesBackend {
    fn label(&self) -> &'static str {
        "hermes"
    }

    fn probe(&self) -> CapabilitySnapshot {
        if !command_exists("hermes") {
            return CapabilitySnapshot::missing("hermes");
        }
        CapabilitySnapshot::from_runtime(&runtime::hermes_snapshot(true))
    }

    fn cockpit_state(&self) -> CockpitState {
        CockpitState::from_runtime(runtime::hermes_snapshot(command_exists("hermes")))
    }
}

impl BackendAdapter for OpenClawBackend {
    fn label(&self) -> &'static str {
        "openclaw"
    }

    fn probe(&self) -> CapabilitySnapshot {
        if !command_exists("openclaw") {
            return CapabilitySnapshot::missing("openclaw");
        }
        CapabilitySnapshot::from_runtime(&runtime::openclaw_snapshot(true))
    }

    fn cockpit_state(&self) -> CockpitState {
        CockpitState::from_runtime(runtime::openclaw_snapshot(command_exists("openclaw")))
    }
}

impl BackendAdapter for MissingBackend {
    fn label(&self) -> &'static str {
        "missing"
    }

    fn probe(&self) -> CapabilitySnapshot {
        CapabilitySnapshot::from_runtime(&runtime::RuntimeSnapshot::missing("backend"))
    }

    fn cockpit_state(&self) -> CockpitState {
        CockpitState::from_runtime(runtime::RuntimeSnapshot::missing("backend"))
    }
}

pub fn adapter_for(choice: BackendChoice) -> Box<dyn BackendAdapter> {
    match choice {
        BackendChoice::Fake => Box::new(FakeBackend),
        BackendChoice::Hermes => Box::new(HermesBackend),
        BackendChoice::Openclaw => Box::new(OpenClawBackend),
        BackendChoice::Remote => Box::new(MissingBackend),
        BackendChoice::Auto => {
            if command_exists("hermes") {
                Box::new(HermesBackend)
            } else if command_exists("openclaw") {
                Box::new(OpenClawBackend)
            } else {
                Box::new(MissingBackend)
            }
        }
    }
}

pub fn load_fixture(path: &Path) -> Result<CockpitState> {
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read fixture {}", path.display()))?;
    serde_json::from_str(&data).with_context(|| format!("invalid fixture JSON {}", path.display()))
}

pub fn command_exists(command: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| {
            env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(command);
                candidate.is_file() || (cfg!(windows) && candidate.with_extension("exe").is_file())
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_backend_exposes_required_capabilities() {
        let probe = FakeBackend.probe();
        assert!(probe.reachable);
        assert!(probe.events);
        assert!(probe.approvals);
        assert!(probe.interrupt);
    }
}
