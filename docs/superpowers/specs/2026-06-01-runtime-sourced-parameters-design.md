# Runtime-Sourced Parameters Design

## Goal

Avu must not invent live cockpit parameters. In Hermes/OpenClaw live modes, every backend-facing value must come from backend config, backend probe output, backend events, or direct system probes. Hardcoded values are allowed only in explicit fixtures and tests.

## Problem

The current implementation still synthesizes live runtime state in several places:

- `CockpitState::from_capability()` fills `model_label`, `voice_label`, CPU, memory, modules, and events with static defaults.
- `engine::project_events()` maps capabilities and events to fixed module percentages like `80`, `95`, and `72`.
- `HermesBackend::probe()` and `OpenClawBackend::probe()` infer many capabilities from a single command success.
- `AvuConfig::default()` reports backend, wake phrase, and posture defaults without reading a config file.
- `fake_listening()` contains demo labels, metrics, modules, events, and approval data, which is valid only for fixture/test mode.

## Design

Add a runtime snapshot boundary between backend/system discovery and the TUI domain state.

```text
Hermes/OpenClaw config + CLI probes + system probes + backend events
        ↓
RuntimeSnapshot
        ↓
CockpitState::from_runtime(snapshot)
        ↓
engine projection + ratatui cockpit
```

`RuntimeSnapshot` is the only live source of cockpit parameters. It records the value and whether it was reported, detected, unavailable, or fixture-provided. The UI can then show `unreported`, `disabled`, or `unavailable` instead of fake numbers.

## Sources

### Backend sources

For Hermes and OpenClaw, Avu should prefer explicit machine-readable commands where available. Initial implementation will safely probe these commands without assuming they exist:

- `hermes config --json`
- `hermes status --json`
- `hermes doctor --json`
- `openclaw config --json`
- `openclaw gateway status --json`
- `openclaw status --json`

If a command is unavailable or non-JSON, the snapshot marks the related field as `unreported` and leaves control disabled unless another explicit source confirms support.

### System sources

System state comes directly from `/proc` on Linux:

- CPU usage from `/proc/stat` using a short sample.
- memory usage from `/proc/meminfo`.
- command availability from `PATH`.
- wake/mic availability from known device/config presence only when detectable; otherwise `unreported`.

### Avu config sources

`~/.avu/config.toml` is only for Avu-owned preferences:

- preferred backend selection.
- UI refresh/cockpit behavior.
- optional wake phrase override for Avu input handling.

It must not override backend truth for permissions, models, tools, approvals, or gateway state.

## Live vs fixture separation

Fixtures remain useful for tests, demos, setup rehearsal, and UI snapshots. They must be explicit:

- `--backend fake`
- `--fixture path`

Auto/Hermes/OpenClaw live modes must never fall back to fixture approvals, fixture tools, fixture model names, fixture metrics, or fixture module levels.

## Error handling

- Missing backend command: live state is disconnected with control disabled.
- Backend command exists but JSON command fails: mark backend reachable only if a non-mutating health command succeeds; otherwise disconnected.
- Missing field in backend JSON: show `unreported`, not a guessed value.
- Unsupported control capability: disable the corresponding control in the TUI.
- System probe failure: show `unreported` for the metric and keep the app usable.

## Testing

Tests must prove live state does not borrow fixture values:

- Missing Hermes/OpenClaw reports disconnected/unreported values.
- Runtime snapshots with partial JSON keep unknown capabilities disabled.
- System metrics parsing works from sample `/proc` text.
- Fixture mode still renders deterministic approval/tool/demo state.
- Auto mode without backends does not select fake unless the user explicitly requested `--backend fake`.

## Self-review

- No placeholders remain.
- Scope is focused on runtime-sourced parameters, not live bidirectional backend control.
- Backend APIs are treated as optional probes, not assumed facts.
- Fixture/demo behavior is preserved but isolated from live paths.
