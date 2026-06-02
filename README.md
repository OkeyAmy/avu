# Avu

**Avu is a terminal cockpit for live AI-agent backends.**

Avu sits in front of [Hermes Agent](https://hermes-agent.nousresearch.com/) and OpenClaw. It shows the backend model, gateway health, voice/STT/TTS configuration, recent activity, approvals, and operator controls without inventing fake state. The backend still owns tools, permissions, memory, sessions, voice recording, and message delivery; Avu owns the cockpit UI and routes your input to the backend CLI.

## What Avu does

- Shows real backend status from `hermes status`, `hermes gateway status`, `hermes logs`, `openclaw status`, `openclaw gateway status`, and `openclaw logs`.
- Launches an interactive Ratatui cockpit with live refresh.
- Provides two input paths from the TUI:
  - **CMD**: run local shell commands from the terminal.
  - **Chat**: send text or slash commands to Hermes/OpenClaw.
- Surfaces Hermes voice configuration (`stt`, `tts`, record key, auto-TTS) from `~/.hermes/config.yaml`.
- Detects newly generated Hermes/OpenClaw audio files and tries to play them with an available local player.

## What Avu does not do

- It does **not** hardcode backend tools. Hermes/OpenClaw decide which tools exist and when to call them.
- It does **not** replace Hermes/OpenClaw setup. Install and configure one backend first.
- It does **not** require normal users to install Rust/Cargo. Rust is only for contributors.
- It does **not** fake approvals, tools, model names, or voice status in live mode.

## Architecture

```text
You (terminal)
  │
  ▼
Avu cockpit (Rust + ratatui)
  │
  ├── Hermes adapter
  │     ├── hermes status
  │     ├── hermes gateway status
  │     ├── hermes logs agent
  │     ├── ~/.hermes/config.yaml
  │     └── ~/.hermes/audio_cache + ~/.hermes/cache/audio
  │
  ├── OpenClaw adapter
  │     ├── openclaw status --json
  │     ├── openclaw gateway status --json
  │     ├── openclaw logs --json
  │     └── ~/.openclaw/media or $OPENCLAW_STATE_DIR/media
  │
  └── Local operator commands
        └── shell command execution in CMD mode
```

## Install

### Linux/macOS/WSL2

```bash
curl -fsSL https://raw.githubusercontent.com/OkeyAmy/avu/master/scripts/install.sh | bash
```

### Windows PowerShell

```powershell
iwr -useb https://raw.githubusercontent.com/OkeyAmy/avu/master/scripts/install.ps1 | iex
```

### npm wrapper

```bash
npm install -g github:OkeyAmy/avu#master
avu install --check
```

The npm wrapper downloads a prebuilt native binary from GitHub Releases on first run and verifies it against `SHA256SUMS`.

## Requirements

Normal users need:

1. `avu` installed.
2. One backend CLI on `PATH`:
   - `hermes`, or
   - `openclaw`.
3. Backend-specific dependencies installed by that backend.

Avu itself does not require Python, Rust, Cargo, PortAudio, ffmpeg, STT, or TTS dependencies at install time. Hermes/OpenClaw own those layers.

## First run

```bash
avu install --check
avu setup
avu doctor
avu status --backend auto
avu tui --backend auto
```

If Avu reports `command was not found on PATH`, open a fresh terminal after installing Hermes/OpenClaw, or run the backend installer’s PATH repair command.

## Commands

| Command | Purpose |
| --- | --- |
| `avu status` | Print live backend/cockpit status. |
| `avu status --json` | Machine-readable status. |
| `avu tui` | Launch the live cockpit. |
| `avu tui --once` | Render one frame for tests/CI. |
| `avu setup` | Run guided setup checks. |
| `avu doctor` | Diagnose backend/config/gateway readiness. |
| `avu config` | Show Avu config paths/defaults. |
| `avu install --check` | Verify installation and PATH. |
| `avu install --fix-self` | Copy Avu into `~/.local/bin` and print PATH repair instructions. |

## TUI controls

| Key | Action |
| --- | --- |
| `:` | Open input picker. Choose CMD or Chat. |
| `c` after `:` | Enter **CMD** mode. Runs a local shell command on Enter. |
| `t` after `:` | Enter **Chat** mode. Sends text to Hermes/OpenClaw on Enter. |
| `/` after `:` | Enter Chat mode with `/` prefilled for backend slash commands. |
| `Backspace` / `Delete` | Delete the last typed character in CMD/Chat mode. |
| `Esc` | Cancel current input or quit when not typing. |
| `s` | Refresh backend status immediately. |
| `m` | Show voice command help in the activity log. |
| `q` | Quit. |

Examples inside the TUI:

```text
: c pwd                         # run local shell command
: c ls ~/.hermes/audio_cache    # inspect local Hermes audio files
: t hello Hermes                # send a chat message to the backend
: /voice on                     # send backend slash command
: /tts say hello from Avu       # ask Hermes to generate TTS audio
```

## Backend integration

### Hermes

Avu reads:

- `hermes status` for model/provider and component state.
- `hermes gateway status` for gateway service/profile conflicts.
- `hermes logs agent --lines 30` for recent activity.
- `~/.hermes/config.yaml` for `voice`, `stt`, and `tts` settings.
- Hermes audio cache paths for generated audio:
  - `$HERMES_AUDIO_CACHE_DIR` if set,
  - `~/.hermes/audio_cache`,
  - `~/.hermes/cache/audio`.

Hermes voice facts from upstream docs:

- CLI TTS output is saved as MP3 in `~/.hermes/audio_cache/`.
- CLI voice mode is enabled inside Hermes with `/voice on`.
- Record key defaults to `voice.record_key` in `~/.hermes/config.yaml` (commonly `ctrl+b`).
- STT providers are configured under `stt:` (`local`, `groq`, `openai`, `mistral`, `xai`, or command/plugin providers).
- TTS providers are configured under `tts:` (`edge`, `elevenlabs`, `openai`, `gemini`, `xai`, `neutts`, `piper`, custom command providers, etc.).

Avu sends Chat input to Hermes with `hermes --oneshot <message>`. If Hermes creates a new audio file while handling the prompt, Avu tries to play it with an available player (`ffplay`, `mpv`, `paplay`, `aplay`, `xdg-open`, `afplay`, or Windows PowerShell playback).

### OpenClaw

Avu reads:

- `openclaw gateway status --json` for gateway state when available.
- `openclaw status --json` for backend status.
- `openclaw logs --json --limit 20` for recent events.
- OpenClaw media cache paths:
  - `$OPENCLAW_STATE_DIR/media`,
  - `~/.openclaw/media`.

OpenClaw gateway docs recommend:

```bash
openclaw status
openclaw gateway status
openclaw logs --follow
openclaw doctor
openclaw gateway restart
```

Avu does not hardcode OpenClaw tools. Chat input routes through the OpenClaw CLI surface and OpenClaw decides how to invoke tools.

## Voice and audio troubleshooting

### I cannot hear voice

1. Confirm Hermes can generate TTS directly:

   ```bash
   hermes --oneshot "/tts say hello from Hermes"
   ls -lt ~/.hermes/audio_cache | head
   ```

2. Confirm you have an audio player:

   ```bash
   command -v ffplay || command -v mpv || command -v paplay || command -v aplay || command -v xdg-open
   ```

3. Launch Avu from the same desktop/session that has audio output. If you run Avu over SSH, audio playback happens on the remote machine, not your local speakers.

4. Check Hermes provider config and keys:

   ```bash
   hermes status
   hermes logs --since 10m -n 100
   ```

5. If Hermes generates MP3s but Avu does not play them, use CMD mode to play manually and inspect player errors:

   ```text
   : c ffplay -nodisp -autoexit ~/.hermes/audio_cache/latest-file.mp3
   ```

### TUI says IDLE forever

Avu should auto-refresh every two seconds when you are not typing. If it still shows IDLE:

```bash
avu status --backend hermes --json
hermes logs agent --lines 30
hermes gateway status
```

Look for recent `voice recording`, `STT`, `TTS`, `tool`, `response`, or `processing` lines. Avu maps those log events to LISTENING, PROCESSING, SPEAKING, TOOL ACTIVE, and ERROR states.

### Gateway conflict

Hermes can have multiple profiles. If a gateway reports that a Telegram token is already in use, Avu does not assume a profile name. It reads `hermes gateway status` and reports the running profile, then suggests a profile-specific restart command such as:

```bash
hermes --profile <reported-profile> gateway restart
```

If the default profile should not own that token, keep the default gateway stopped.

## Development

### Build from source

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

### Verification checklist for contributors

Before pushing:

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

When touching TUI behavior, also run a real PTY/manual test:

1. `avu tui --backend hermes`
2. Press `:` then `c`, run `pwd`.
3. Press `:` then `t`, send a short chat message.
4. Press `:` then `/`, send `/tts say hello from Avu`.
5. Confirm the screen responds, Backspace/Delete work, and audio files appear if Hermes TTS succeeds.

## Release flow

Maintainers publish prebuilt binaries with GitHub Releases. The npm wrapper downloads:

- `avu-x86_64-unknown-linux-musl.tar.gz`
- `SHA256SUMS`

Other target assets may be added by CI:

- `avu-aarch64-unknown-linux-musl.tar.gz`
- `avu-x86_64-apple-darwin.tar.gz`
- `avu-aarch64-apple-darwin.tar.gz`
- Windows zip assets
- SBOM/checksum/signature artifacts

Verify a release download:

```bash
curl -fsSLO https://github.com/OkeyAmy/avu/releases/download/v0.1.0/SHA256SUMS
curl -fsSLO https://github.com/OkeyAmy/avu/releases/download/v0.1.0/avu-x86_64-unknown-linux-musl.tar.gz
sha256sum -c SHA256SUMS
tar -xzf avu-x86_64-unknown-linux-musl.tar.gz
./avu --version
```

## Security notes

- CMD mode runs shell commands on your local machine. Use it the same way you would use a terminal.
- Chat mode sends text to your configured backend. Hermes/OpenClaw own tool approvals and permission policy.
- Avu status output may include paths and backend state, but it should not print secret values. If you see a secret leak, treat it as a bug.

## License

See repository license metadata.
