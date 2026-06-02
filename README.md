# Avu

**Terminal cockpit for live AI-agent backends.**

Avu sits in front of [Hermes](https://hermes-agent.nousresearch.com/) or OpenClaw and surfaces real-time backend state — model, gateway health, voice config, recent activity, approvals — without inventing fake data. The backend owns tools, memory, and execution; Avu owns the cockpit UI and routes your input.

---

## Install

```bash
# Linux / macOS / WSL2
curl -fsSL https://raw.githubusercontent.com/OkeyAmy/avu/master/scripts/install.sh | bash

# Windows PowerShell
iwr -useb https://raw.githubusercontent.com/OkeyAmy/avu/master/scripts/install.ps1 | iex

# npm (any platform)
npm install -g github:OkeyAmy/avu#master
avu install --check
```

Requirements: one backend CLI on `PATH` (`hermes` or `openclaw`). Avu itself does not need Python, Rust, PortAudio, or STT/TTS runtimes — backends own those layers.

---

## Quick start

```bash
avu install --check    # verify installation
avu setup              # guided setup checks
avu doctor             # diagnose backend readiness
avu status             # print live status
avu tui                # launch the cockpit
```

---

## Commands

| Command | Purpose |
|---------|---------|
| `avu status` | Print live backend/cockpit status |
| `avu status --json` | Machine-readable status |
| `avu tui` | Launch the live cockpit |
| `avu tui --once` | Render one frame (tests/CI) |
| `avu setup` | Guided setup checks |
| `avu doctor` | Diagnose backend/config/gateway readiness |
| `avu config` | Show config paths and defaults |
| `avu install --check` | Verify installation and PATH |
| `avu install --fix-self` | Copy binary to `~/.local/bin` |

---

## TUI controls

| Key | Action |
|-----|--------|
| `:` | Open input picker (CMD or Chat) |
| `c` after `:` | Enter CMD mode — run shell commands |
| `t` after `:` | Enter Chat mode — send text to backend |
| `/` after `:` | Chat mode with `/` prefilled (backend slash commands) |
| `Backspace` / `Delete` | Delete last character |
| `Esc` | Cancel input or quit when not typing |
| `s` | Refresh backend status immediately |
| `m` | Show voice command help |
| `q` | Quit |

```
: c pwd                       # run local shell command
: t hello Hermes              # send chat message
: /voice on                   # backend slash command
: /tts say hello from Avu     # TTS generation
```

---

## Architecture

```
You (terminal)
  │
  ▼
Avu cockpit (Rust + ratatui)
  │
  ├── Hermes adapter ── hermes status, gateway, logs, config
  ├── OpenClaw adapter ── openclaw status, gateway, logs
  └── Local CMD mode ── shell execution
```

Avu discovers which backend is on `PATH` and adapts automatically. All live data comes from backend CLI output — no hardcoded model names, tools, or fake approval states.

---

## Development

```bash
git clone https://github.com/OkeyAmy/avu
cd avu
cargo build
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

### Local run

```bash
cargo run -- status --backend fake
cargo run -- tui --fixture fixtures/approval_destructive.json --once
cargo run -- tui --backend hermes
```

### Pre-push checklist

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo build --release
node --check npm/bin/avu.js
node --check npm/install.js
bash -n scripts/install.sh
npm pack --dry-run
```

---

## Release

Maintainers push a version tag matching `Cargo.toml` and `package.json`:

```bash
git tag v0.1.0
git push origin v0.1.0
```

CI builds and uploads prebuilt binaries for Linux (x86_64, aarch64), macOS (x86_64, aarch64), and Windows (x86_64) with SHA256SUMS and provenance attestation. The npm wrapper verifies checksums on first run.
