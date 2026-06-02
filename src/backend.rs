use crate::{
    cli::BackendChoice,
    domain::{CapabilitySnapshot, CockpitState},
    runtime,
};
use anyhow::{Context, Result};
use std::{env, path::Path, process::Command, thread, time::Duration};

pub trait BackendAdapter {
    fn label(&self) -> &'static str;
    fn probe(&self) -> CapabilitySnapshot;
    fn cockpit_state(&self) -> CockpitState;
    fn route_prompt(&self, prompt: &str) -> BackendCommandResult;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BackendCommandResult {
    pub ok: bool,
    pub summary: String,
}

impl BackendCommandResult {
    fn ok(summary: impl Into<String>) -> Self {
        Self {
            ok: true,
            summary: summary.into(),
        }
    }

    fn failed(summary: impl Into<String>) -> Self {
        Self {
            ok: false,
            summary: summary.into(),
        }
    }
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

    fn route_prompt(&self, prompt: &str) -> BackendCommandResult {
        BackendCommandResult::ok(format!("fixture backend received: {prompt}"))
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

    fn route_prompt(&self, prompt: &str) -> BackendCommandResult {
        run_backend_prompt("hermes", prompt)
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

    fn route_prompt(&self, _prompt: &str) -> BackendCommandResult {
        BackendCommandResult::failed("OpenClaw prompt routing is not available yet")
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

    fn route_prompt(&self, _prompt: &str) -> BackendCommandResult {
        BackendCommandResult::failed("backend command was not found on PATH")
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

fn run_backend_prompt(command: &str, prompt: &str) -> BackendCommandResult {
    if !command_exists(command) {
        return BackendCommandResult::failed(format!("{command} command was not found on PATH"));
    }

    let mut child = match Command::new(command)
        .args(["--oneshot", prompt])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return BackendCommandResult::failed(format!("failed to start {command}: {error}"));
        }
    };

    for _ in 0..90 {
        match child.try_wait() {
            Ok(Some(_)) => match child.wait_with_output() {
                Ok(output) => return summarize_output(output),
                Err(error) => {
                    return BackendCommandResult::failed(format!(
                        "failed to read {command} output: {error}"
                    ));
                }
            },
            Ok(None) => thread::sleep(Duration::from_millis(500)),
            Err(error) => {
                return BackendCommandResult::failed(format!(
                    "failed while waiting for {command}: {error}"
                ));
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    BackendCommandResult::failed(format!("{command} prompt timed out after 45s"))
}

fn summarize_output(output: std::process::Output) -> BackendCommandResult {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let summary = stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(if output.status.success() {
            "backend command completed"
        } else {
            "backend command failed without output"
        })
        .to_string();

    if output.status.success() {
        BackendCommandResult::ok(summary)
    } else {
        BackendCommandResult::failed(summary)
    }
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

    #[test]
    fn fake_backend_routes_prompts_for_tui_tests() {
        let result = FakeBackend.route_prompt("/voice");
        assert!(result.ok);
        assert_eq!(result.summary, "fixture backend received: /voice");
    }

    #[test]
    fn missing_backend_rejects_prompt_routing() {
        let result = MissingBackend.route_prompt("/voice");
        assert!(!result.ok);
        assert!(result.summary.contains("not found"));
    }
}
