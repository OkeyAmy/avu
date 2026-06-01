# Avu

**Wake-capable terminal cockpit over Hermes/OpenClaw.**

Avu is a middle-layer terminal UI that sits between you and your AI agent backends (Hermes, OpenClaw). It surfaces real-time agent state — tools, approvals, voice, model, gateway — from backend logs and system probes, without inventing fake data.

## Architecture

```
You (terminal)
  │
  ▼
Avu cockpit (ratatui TUI)
  │
  ├── Hermes adapter ──► hermes logs, hermes status --json, hermes doctor --json
  ├── OpenClaw adapter ──► openclaw logs --json, openclaw gateway status --json
  └── System probes ──► /proc/stat, /proc/meminfo, PATH detection
```

Avu owns the cockpit UX and command routing. Backends own approvals, permissions, memory, and execution.

## Commands

| Command | Description |
|---------|-------------|
| `avu setup` | Run guided terminal setup checks. |
| `avu doctor` | Diagnose backend, capability, config, and wake readiness. |
| `avu status` | Print backend and cockpit status. |
| `avu config` | Print config paths and effective defaults. |
| `avu tui` | Render the Avu cockpit from a live adapter or fixture. |

## Backend adapters

Avu discovers which backend is available and adapts automatically:

- **Hermes** — probes via `hermes status --json`, `hermes doctor --json`, reads events from `hermes logs agent`.
- **OpenClaw** — probes via `openclaw gateway status --json`, `openclaw status --json`, reads events from `openclaw logs --json`.
- **Auto** — detects `hermes` or `openclaw` on PATH. If neither is found, reports disconnected state instead of faking data.
- **Fake** — explicit `--backend fake` for testing with fixture data.

All live backend parameters (model, capabilities, events, tools) come from actual backend config and log output. No hardcoded tool names or synthetic approval data leaks into live mode.

## Configuration

Avu config lives in `~/.avu/config.toml`:

```toml
backend = "auto"
permission_posture = "observe_notify"
wake_mode = "keyboard_only"
wake_phrase = "hey avu"
```

Avu config only controls Avu-owned preferences. Backend truth (model, permissions, tools, approvals) comes from the backend itself.

## Running

```bash
# See current status
avu status

# Quick health check
avu doctor

# Launch the cockpit with a demo fixture
avu tui --fixture fixtures/approval_destructive.json --once

# Launch with live backend
avu tui

# Setup wizard
avu setup
```

## Building

```bash
cargo build --release
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```
