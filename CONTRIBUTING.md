# Contributing to Avu

Thanks for your interest in contributing! Avu is a wake-capable terminal cockpit for Hermes and OpenClaw AI agent backends. This guide covers how to set up for development, make changes, and get them merged.

---

## Quick start

```bash
# Fork and clone the repo
git clone https://github.com/OkeyAmy/avu
cd avu

# Build
cargo build

# Run all tests
cargo test --all-targets --all-features

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Check formatting
cargo fmt --check

# Build release
cargo build --release
```

---

## Prerequisites

| Requirement | Purpose |
|-------------|---------|
| Rust toolchain (edition 2024) | Build the Rust binary |
| Node.js 20+ | npm wrapper testing (optional) |
| Backend CLI (`hermes` or `openclaw` on PATH) | Live backend testing (optional) |

---

## Project structure

| Path | Purpose |
|------|---------|
| `src/main.rs` | Entry point and command dispatch |
| `src/cli.rs` | CLI argument definitions (Clap) |
| `src/domain.rs` | Core types: CockpitState, events, capabilities |
| `src/backend.rs` | Backend adapters (Hermes, OpenClaw, Fake) |
| `src/backend_events.rs` | Backend turn context and event classification |
| `src/runtime.rs` | Backend runtime probing and log parsing |
| `src/tui.rs` | Ratatui terminal UI and interactive loop |
| `src/engine.rs` | Event pipeline and state projection |
| `src/hud.rs` | Event radar HUD rendering |
| `src/intent.rs` | Intent parsing and approval gating |
| `src/voice.rs` | Voice session state machine |
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

## Making changes

### Branch conventions

Use descriptive branch names with a conventional prefix:

```
feat/my-change       # New feature
fix/my-bugfix        # Bug fix
docs/my-doc-change   # Documentation
refactor/my-rename   # Refactoring
test/my-test-add     # Test additions
```

### Commit messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(tui): add Slash mode, visible cursor, CLI routing
fix(backend): use ALSA fallback for Linux audio
docs: document Avu voice handoff
test(tui): cover backend slash input
ci: set release repository for gh
```

Keep the subject line under 72 characters. Use the body to explain *why* when it isn't obvious.

### Code style

- `cargo fmt` handles formatting — run it before committing.
- `cargo clippy` must pass with no warnings (`-D warnings`).
- No `as any`, `@ts-ignore`, or type suppressions (applies to Node wrapper).
- No empty catch blocks, unwrap-heavy patterns, or shotgun debugging.
- Match existing patterns when extending the codebase.

---

## Testing

### Run everything

```bash
cargo test --all-targets --all-features
```

### TUI tests

The TUI tests render to an in-memory buffer (no terminal needed). Use `--backend fake` or `--fixture` for visual smoke tests:

```bash
cargo run -- tui --fixture fixtures/approval_destructive.json --once
cargo run -- tui --backend fake --once
```

### Manual TUI check

When touching TUI behaviour, also verify interactively:

1. `cargo run -- tui --backend fake`
2. Press `:` then `c`, run `pwd`.
3. Press `:` then `t`, send a short chat message.
4. Press `:` then `/`, send `/voice on`.
5. Confirm: screen responds, Backspace/Delete work, event radar updates.

---

## Pull request checklist

Before opening a PR:

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

## Release flow

Maintainers publish prebuilt binaries with GitHub Releases.

```bash
# Tag must match version in Cargo.toml and package.json
git tag v0.1.0
git push origin v0.1.0
```

CI builds and uploads archives for Linux (x86_64, aarch64), macOS (x86_64, aarch64), and Windows (x86_64, aarch64), plus SHA256SUMS, signature, and SBOM.

---

## Getting help

- Open a [GitHub issue](https://github.com/OkeyAmy/avu/issues) for bugs or feature requests.
- For Hermes-specific questions, see the Hermes Agent documentation at [hermes-agent.nousresearch.com](https://hermes-agent.nousresearch.com/).
