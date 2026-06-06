# Avu

> **Archived project**
>
> This project has been archived and is no longer actively maintained. The repository may be revived if there is clear demand from heavy users of Hermes or OpenClaw. It would also be great to receive approval or support from the Hermes team itself.

**Terminal cockpit for live AI-agent backends.**

Avu is a Ratatui-based terminal UI that connects to [Hermes Agent](https://hermes-agent.nousresearch.com/) or OpenClaw. It surfaces real-time backend state — model, gateway health, voice/STT/TTS configuration, recent activity, approvals, and operator controls — without inventing fake data. The backend owns tools, permissions, memory, sessions, and execution; Avu owns the cockpit UI and routes your input.

---

## Features

- **Live backend status** — reads from `hermes status`, `hermes gateway status`, `hermes logs`, `openclaw status`, `openclaw gateway status`, and `openclaw logs`.
- **Interactive TUI** — Ratatui cockpit with fast startup and background live refresh.
- **Dual input modes** — CMD mode for local shell commands, Chat mode for backend prompts.
- **Approval routing** — surfaces pending approvals from the backend with safety gating.
- **Voice awareness** — detects Hermes voice config (STT, TTS providers, record key), auto-plays generated audio, and hands off to Hermes voice mode with `Ctrl+B`.
- **Event radar** — visual HUD that maps backend log events to cockpit modes (LISTENING, PROCESSING, SPEAKING, TOOL ACTIVE, APPROVAL, etc.).
- **Fixture mode** — test the TUI with pre-recorded fixture data without a live backend.
- **Backend auto-detection** — discovers Hermes or OpenClaw on `PATH` and adapts automatically.

---

## Install

### Linux / macOS / WSL2

```bash
curl -fsSL https://raw.githubusercontent.com/OkeyAmy/avu/master/scripts/install.sh | bash
```

### Windows PowerShell

```powershell
iwr -useb https://raw.githubusercontent.com/OkeyAmy/avu/master/scripts/install.ps1 | iex
```

### npm (any platform)

```bash
npm install -g github:OkeyAmy/avu#master
avu install --check
```

The npm wrapper downloads a prebuilt native binary from GitHub Releases on first run and verifies it against `SHA256SUMS`.

### Build from source (contributors)

```bash
git clone https://github.com/OkeyAmy/avu
cd avu
cargo build --release
cargo install --path .
```

See [Development](#development) for full contributor setup.

---

## Requirements

Normal users need:

| Requirement | Details |
|-------------|---------|
| **Avu binary** | installed via one of the methods above |
| **Backend CLI** | `hermes` or `openclaw` on `PATH` |
| **Audio player** (optional) | `ffplay`, `mpv`, `paplay`, `aplay`, `xdg-open` (Linux); `afplay` (macOS); built-in (Windows) |

Avu does **not** require Python, Rust, Cargo, PortAudio, ffmpeg, STT, or TTS runtimes at install time — Hermes and OpenClaw own those layers.

---

## Quick start

```bash
# Verify the installation
avu install --check

# Run the guided setup wizard
avu setup

# Diagnose backend readiness
avu doctor

# Print current status
avu status

# Launch the cockpit
avu tui
```

If Avu reports `command was not found on PATH`, open a fresh terminal after installing Hermes/OpenClaw, or run the backend installer's PATH repair command.

---

## Commands

| Command | Purpose |
|---------|---------|
| `avu status` | Print live backend and cockpit status |
| `avu status --json` | Machine-readable status output |
| `avu tui` | Launch the live interactive cockpit |
| `avu tui --once` | Render one frame and exit (for tests and CI) |
| `avu tui --backend fake` | Launch with fixture data (no live backend needed) |
| `avu tui --fixture <path>` | Launch with a specific fixture file |
| `avu setup` | Run guided terminal setup checks |
| `avu setup --quick` | Quick setup, only check missing settings |
| `avu setup --non-interactive` | Safe defaults, print next steps |
| `avu doctor` | Diagnose backend, capability, config, and wake readiness |
| `avu doctor --json` | Doctor report as JSON |
| `avu config` | Show config paths and effective defaults |
| `avu config --json` | Config as JSON |
| `avu install --check` | Verify installation state and PATH |
| `avu install --fix-self` | Copy binary to `~/.local/bin` and repair PATH |
| `avu install --info` | JSON install report for automation |

### Backend selection

All commands accept `--backend <choice>`:

| Choice | Behaviour |
|--------|-----------|
| `auto` (default) | Detect `hermes` or `openclaw` on PATH |
| `hermes` | Use Hermes backend adapter |
| `openclaw` | Use OpenClaw backend adapter |
| `fake` | Use fixture data (no live backend) |
| `remote` | Future remote backend (currently reports missing) |

---

## TUI controls

Once inside the cockpit (`avu tui`):

| Key | Action |
|-----|--------|
| `:` | Open input picker |
| `:` then `c` | Enter **CMD mode** — type a shell command, press Enter to run |
| `:` then `t` | Enter **Chat mode** — type a message for the backend |
| `:` then `/` | Enter **Slash mode** — raw backend slash commands are sent unchanged |
| `Enter` | Submit the current input |
| `Backspace` / `Delete` | Delete last character |
| `Ctrl+B` | Hand the terminal to Hermes interactive voice/TUI mode |
| `Esc` | Cancel current input / quit when not typing |
| `s` | Refresh backend status immediately |
| `w` | Arm keyboard wake/listening mode for the configured wake phrase |
| `m` | Show voice command help in the activity log |
| `i` | Request interrupt (backend routing not yet enabled) |
| `q` | Quit the cockpit |

### Examples

```
: c pwd                          # run local shell command
: c ls ~/.hermes/audio_cache     # inspect Hermes audio files
: t hello Hermes                 # send chat message to backend
: /voice on                      # send backend slash command
: /tts say hello from Avu        # ask Hermes to generate TTS audio
```

### Approval controls

When a structured pending approval is displayed and routing is enabled for that backend:

| Key | Action |
|-----|--------|
| `a` | Arm approval intent (for destructive operations) |
| `A` | Confirm armed approval (requires second confirmation) |
| `r` | Route rejection via backend |

Live Hermes/OpenClaw log approval events are observe/notify until the backend exposes a structured pending approval source that Avu can bind to a backend, approval ID, and permission posture. Avu will not route approval responses from fixtures or vague log text as live backend authority.

---

## Configuration

Avu config lives in `~/.avu/config.toml`:

```toml
backend = "auto"
permission_posture = "observe_notify"
wake_mode = "keyboard_only"
wake_phrase = "hey avu"
```

Override the config directory with `$AVU_HOME`. Avu config only controls Avu-owned preferences — backend truth (model, permissions, tools, approvals) comes from the backend itself.

### Config paths

| Path | Purpose |
|------|---------|
| `~/.avu/config.toml` | User configuration |
| `~/.avu/auth.json` | Auth metadata |
| `~/.avu/capabilities.json` | Cached capability snapshot |
| `~/.avu/logs/` | Avu log output |
| `~/.avu/fixtures/` | Local fixture storage |

---

## Architecture

```
── Terminal ──
      │
      ▼
── Avu Cockpit (Rust + Ratatui) ──
      │
      ├── Hermes adapter
      │     ├── hermes status / gateway status / logs
      │     ├── ~/.hermes/config.yaml (voice, STT, TTS)
      │     └── ~/.hermes/audio_cache for generated audio
      │
      ├── OpenClaw adapter
      │     ├── openclaw status --json / gateway status --json
      │     ├── openclaw logs --json
      │     └── ~/.openclaw/media for generated audio
      │
      └── CMD mode
            └── local shell execution (sh -lc / cmd /C)
```

Avu discovers which backend is on `PATH` in auto mode. All live data comes from backend CLI output — no hardcoded model names, tools, or fake approval states. The backend owns tools, permissions, memory, sessions, voice recording, and message delivery. Avu owns the cockpit UI and routes your input to the backend CLI.

---

## Backend integration

### Hermes

Avu reads from Hermes via:

- `hermes status` / `hermes status --json` — model, provider, component state
- `hermes gateway status` — gateway service health and profile conflicts
- `~/.hermes/logs/agent.log` — recent timestamped agent activity, filtered to the last 15 minutes
- `~/.hermes/config.yaml` — STT, TTS, and voice settings
- Audio cache: `$HERMES_AUDIO_CACHE_DIR`, `~/.hermes/audio_cache`, `~/.hermes/cache/audio`

Chat input routes to Hermes via `hermes --continue avu-tui --oneshot <message>` so repeated messages stay in the same named Avu session instead of starting a fresh conversation each time. If TTS audio is generated, Avu uses explicit `MEDIA:` output when available, otherwise checks the audio cache, then starts an available local player without blocking the TUI.

### OpenClaw

Avu reads from OpenClaw via:

- `openclaw status --json` — backend status
- `openclaw gateway status --json` — gateway state
- `openclaw logs --json --limit 20` — recent gateway events (JSONL format)
- Media cache: `$OPENCLAW_STATE_DIR/media`, `~/.openclaw/media`

Chat input routes via `openclaw agent --message <text>`. OpenClaw decides tool dispatch.

---

## Voice and audio

### How voice works

1. Avu reads Hermes voice config from `~/.hermes/config.yaml` — STT provider, TTS provider, record key.
2. The cockpit shows voice status in the footer bar (e.g., `STT groq · TTS gemini/Kore · key ctrl+b`).
3. Press `w` to arm Avu's keyboard wake/listening state. The default phrase is `hey avu`, and typed wake-prefixed commands are parsed by Avu's intent layer for safe controls.
4. Use Slash mode to send raw backend voice commands: `/voice on`, `/voice status`, `/tts say hello`, `/stt switch groq`.
5. When Hermes generates an audio file in response, Avu detects it and plays it after the backend turn completes.
6. For real microphone recording, press `Ctrl+B` in Avu. Avu temporarily hands the terminal to Hermes' own interactive voice/TUI mode because Hermes owns raw push-to-talk, microphone capture, silence detection, STT, and spoken replies.

### Confirming real voice pickup

Avu does not fake microphone support. A real voice check must pass through Hermes' voice stack:

1. Install Hermes voice support and system audio dependencies (`hermes-agent[voice]`, PortAudio, ffmpeg, and a working STT/TTS provider).
2. Run `avu tui --backend hermes` and press `Ctrl+B`.
3. Inside Hermes voice mode, run `/voice on` if needed.
4. Press Hermes' configured record key (`voice.record_key`, usually `ctrl+b`), speak, then stop speaking.
5. Hermes should show live audio levels, auto-stop after silence, transcribe the utterance, run the normal agent/tool pipeline, and speak the reply back.

Avu's role is to expose readiness, route commands, mirror progress/permission events, and return to the cockpit after the backend voice session exits. Native Avu-owned microphone capture remains optional future work so normal users do not need Rust audio dependencies.

### Troubleshooting

**No audio heard?**
1. Verify Hermes TTS works directly: `hermes --oneshot "/tts say hello"`
2. Check you have an audio player: `command -v ffplay || command -v mpv`
3. Over SSH? Audio plays on the remote machine, not your local speakers.
4. Use CMD mode for manual playback: `: c ffplay -nodisp -autoexit ~/.hermes/audio_cache/<file>.mp3`

**Ctrl+B does not record?**
- Press `Ctrl+B` in Avu to enter Hermes voice mode, then run `/voice on` inside Hermes if needed.
- Press Hermes' configured record key inside Hermes (`voice.record_key`, commonly `ctrl+b`) and speak.
- Exit Hermes to return to Avu.
- If recording still fails, verify directly with `hermes --tui`; Avu does not hardcode or replace Hermes' STT recorder.

**TUI stuck on IDLE?**
- Run `avu status --backend hermes --json` and inspect `~/.hermes/logs/agent.log`.
- Avu maps log events to cockpit modes — look for `voice recording`, `STT`, `TTS`, `tool`, `response` lines.

**Gateway conflict?**
- Run `hermes gateway status`. If a Telegram token is in use by another profile, restart with `hermes --profile <name> gateway restart`.

---

## Development

### Prerequisites

- Rust toolchain (edition 2024)
- Node.js 20+ (for npm wrapper testing)
- A backend CLI on PATH for live testing (optional)

### Build and test

```bash
# Build
cargo build

# Run all tests
cargo test --all-targets --all-features

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Check formatting
cargo fmt --check

# Build release binary
cargo build --release
```

### Local run

```bash
# Status with fake backend
cargo run -- status --backend fake

# Render TUI once with fixture (good for quick testing)
cargo run -- tui --fixture fixtures/approval_destructive.json --once

# Launch live TUI
cargo run -- tui --backend hermes

# Setup wizard
cargo run -- setup --backend fake --non-interactive
```

### Project structure

| Path | Purpose |
|------|---------|
| `src/main.rs` | Entry point and command dispatch |
| `src/cli.rs` | CLI argument definitions (Clap) |
| `src/domain.rs` | Core types: CockpitState, events, capabilities |
| `src/backend.rs` | Backend adapters (Hermes, OpenClaw, Fake) |
| `src/runtime.rs` | Backend runtime probing and log parsing |
| `src/tui.rs` | Ratatui terminal UI and interactive loop |
| `src/engine.rs` | Event pipeline and state projection |
| `src/hud.rs` | Event radar HUD rendering |
| `src/intent.rs` | Intent parsing and approval gating |
| `src/config.rs` | Configuration loading and paths |
| `src/doctor.rs` | Diagnostics and health checks |
| `src/setup.rs` | Interactive setup wizard |
| `src/install.rs` | Install checks, PATH repair, reporting |
| `tests/cli_smoke.rs` | Integration tests for CLI commands |
| `npm/bin/avu.js` | npm wrapper that downloads and runs the binary |
| `npm/install.js` | npm postinstall binary download hook |
| `scripts/install.sh` | Linux/macOS one-line installer |
| `scripts/install.ps1` | Windows PowerShell installer |
| `fixtures/` | Test fixture JSON files |

---

## Contributing

### Getting started

1. Fork the repository.
2. Clone your fork: `git clone https://github.com/<your-username>/avu`
3. Create a feature branch: `git checkout -b feat/my-change`
4. Make your changes.
5. Run the verification checklist below.
6. Push and open a pull request.

### Verification checklist

Before opening a PR, run:

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

When touching TUI behaviour, also run a manual test:

1. `avu tui --backend fake`
2. Press `:` then `c`, run `pwd`.
3. Press `:` then `t`, send a short chat message.
4. Press `:` then `/`, send `/voice on`.
5. Confirm the screen responds, Backspace/Delete work, and the event radar updates.

### Guidelines

- Match existing code style — `cargo fmt` handles formatting.
- No `as any`, `@ts-ignore`, or type suppressions (applies to Node wrapper).
- Keep the `.gitignore` clean (see existing patterns).
- Commit messages follow conventional commits (`feat:`, `fix:`, `chore:`, etc.).

---

## Release flow

Maintainers publish prebuilt binaries with GitHub Releases.

```bash
# Tag must match version in Cargo.toml and package.json
git tag v0.1.0
git push origin v0.1.0
```

CI builds and uploads:

| Platform | Archive |
|----------|---------|
| Linux x86_64 | `avu-x86_64-unknown-linux-musl.tar.gz` |
| Linux aarch64 | `avu-aarch64-unknown-linux-musl.tar.gz` |
| macOS x86_64 | `avu-x86_64-apple-darwin.tar.gz` |
| macOS aarch64 | `avu-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `avu-x86_64-pc-windows-msvc.zip` |
| Windows aarch64 | `avu-aarch64-pc-windows-msvc.zip` |

Plus `SHA256SUMS`, `SHA256SUMS.sig`, `SHA256SUMS.pem`, and SBOM (`*.cdx.json`).

### Verify a release

```bash
curl -fsSLO https://github.com/OkeyAmy/avu/releases/download/v0.1.0/SHA256SUMS
curl -fsSLO https://github.com/OkeyAmy/avu/releases/download/v0.1.0/avu-x86_64-unknown-linux-musl.tar.gz
sha256sum -c SHA256SUMS
tar -xzf avu-x86_64-unknown-linux-musl.tar.gz
./avu --version
```

The npm wrapper verifies checksums automatically on first run. Release provenance can be verified with `gh attestation verify`.

---

## License

MIT
