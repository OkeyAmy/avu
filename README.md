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
| `avu install` | Show installation state, check PATH, or repair self-install. |

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

## Host dependencies

Avu is intentionally thin. A normal user should only need:

- the `avu` command, installed as a Node/npm wrapper around a prebuilt native binary;
- one backend CLI on `PATH`: `hermes` or `openclaw`;
- whatever dependencies that backend already installed for itself.

Avu does not require users to install Rust, Cargo, Python, ripgrep, ffmpeg, PortAudio, or wake/STT runtimes separately during Avu install. Hermes and OpenClaw already own those layers:

- **Hermes** installer handles `uv`, Python 3.11, Node.js 22, ripgrep, ffmpeg, virtualenv, PATH setup, and `~/.hermes/` config/state.
- **OpenClaw** installer handles Node 24 or Node 22.19+, npm/git install modes, gateway service setup, onboarding, PATH setup, and `~/.openclaw` state.

Avu reads backend-reported status/config/logs (`hermes status`, `hermes config get model`, `hermes logs`, `openclaw status --json`, `openclaw gateway status --json`, `openclaw logs`) instead of inventing model names, tool state, provider state, or host metrics.

Because both supported backends provide or require Node, Avu's normal package path is Node/npm plus a prebuilt release binary. Rust is only for contributors building Avu itself.

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

## Installation

Avu provides one-line installers for Linux/macOS/WSL2 and Windows, as well as manual install options.

### One-line installer (Linux/macOS/WSL2)
```bash
curl -fsSL https://raw.githubusercontent.com/OkeyAmy/avu/master/scripts/install.sh | bash
```

### One-line installer (Windows PowerShell)
```powershell
iwr -useb https://raw.githubusercontent.com/OkeyAmy/avu/master/scripts/install.ps1 | iex
```

### Node/npm install
```bash
npm install -g github:OkeyAmy/avu#master
```

The npm wrapper downloads the matching prebuilt Avu binary from GitHub Releases on the first `avu` command. If npm is unavailable, install Hermes or OpenClaw first and open a fresh terminal so their Node/npm runtime is on PATH.

### Developer source build
```bash
# Contributors only; normal users should use the installer or npm wrapper.
git clone https://github.com/OkeyAmy/avu
cd avu
cargo build --release
```

### Developer local install
```bash
# Contributors only.
git clone https://github.com/OkeyAmy/avu
cd avu
cargo install --path .
```

After installing, run the setup wizard:
```bash
avu setup
```

### Post-install verification
```bash
# Check installation state
avu install --check

# Repair PATH if needed
avu install --fix-self

# Get JSON report for automation
avu install --info
```

## Building

Building is for contributors. Normal users should not need Rust or Cargo.

```bash
cargo build --release
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## Maintainer release flow

Normal users install Avu from GitHub Release binaries. Maintainers create those binaries by pushing a version tag that matches both `Cargo.toml` and `package.json`.

```bash
# Example for version 0.1.0
git tag v0.1.0
git push origin v0.1.0
```

The secure release workflow builds and uploads:

- `avu-x86_64-unknown-linux-musl.tar.gz`
- `avu-aarch64-unknown-linux-musl.tar.gz`
- `avu-x86_64-apple-darwin.tar.gz`
- `avu-aarch64-apple-darwin.tar.gz`
- `avu-x86_64-pc-windows-msvc.zip`
- `avu-aarch64-pc-windows-msvc.zip`
- `avu-<version>.cdx.json`
- `SHA256SUMS`
- `SHA256SUMS.sig`
- `SHA256SUMS.pem`

The npm wrapper downloads `SHA256SUMS` and verifies the selected archive on first run. Security-conscious users can also verify release provenance and checksum signatures:

```bash
gh attestation verify avu-x86_64-unknown-linux-musl.tar.gz -R OkeyAmy/avu
cosign verify-blob --certificate SHA256SUMS.pem --signature SHA256SUMS.sig SHA256SUMS
```

After the GitHub Release finishes, verify a fresh install with:

```bash
npm install -g github:OkeyAmy/avu#master
avu install --check
```
