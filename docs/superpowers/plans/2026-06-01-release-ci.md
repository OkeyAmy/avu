# Avu Release CI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add GitHub Actions CI and secure tag-driven release workflows that build, verify, attest, sign, and upload Avu prebuilt binaries for the installers and npm wrapper.

**Architecture:** CI validates Rust code, installer syntax, Node wrapper syntax, and npm package shape on push/PR. The release workflow resolves a version tag, checks out that tag for every job, builds all installer-supported targets, packages root-level executables, generates a CycloneDX SBOM, creates GitHub artifact attestations, signs `SHA256SUMS` with keyless cosign, and uploads final assets to GitHub Releases. Installers verify SHA256 hashes before extracting archives but do not require users to install cosign or SBOM tooling.

**Tech Stack:** GitHub Actions YAML, Rust stable, Cargo, Node.js 22, npm, Bash, PowerShell, GitHub CLI, `actions/checkout`, `actions-rust-lang/setup-rust-toolchain`, `actions/upload-artifact`, `actions/download-artifact`, `actions/attest`, `anchore/sbom-action`, `sigstore/cosign-installer`.

---

## Files

- Create: `.github/workflows/ci.yml` — PR/push validation.
- Create: `.github/workflows/release.yml` — secure release assets, SBOM, attestations, signatures.
- Modify: `npm/install.js` — verify the downloaded archive against `SHA256SUMS` before extraction.
- Modify: `scripts/install.sh` — verify the downloaded archive against `SHA256SUMS` before extraction.
- Modify: `scripts/install.ps1` — verify the downloaded archive against `SHA256SUMS` before extraction.
- Modify: `README.md` — document secure maintainer release flow and verification commands.

## Task 1: Add CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Create workflow directory**

Run: `mkdir -p .github/workflows`

Expected: `.github/workflows` exists.

- [ ] **Step 2: Create `.github/workflows/ci.yml`**

The workflow must run on pull requests and pushes to `master`, with `contents: read`, and jobs for:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features`
- `cargo build --release`
- `node --check npm/install.js`
- `node --check npm/bin/avu.js`
- `bash -n scripts/install.sh`
- PowerShell parser validation for `scripts/install.ps1`
- `npm pack --dry-run`

- [ ] **Step 3: Validate local syntax**

Run: `bash -n scripts/install.sh && node --check npm/install.js && node --check npm/bin/avu.js && npm pack --dry-run`

Expected: exit code 0.

## Task 2: Add secure release workflow

**Files:**
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Create release workflow trigger and permissions**

The workflow must run on `v*` tag pushes and manual dispatch with a required `version` input. It must grant:

```yaml
permissions:
  contents: write
  id-token: write
  attestations: write
```

- [ ] **Step 2: Add version validation**

Resolve `vX.Y.Z` from the tag or manual input, check out that resolved tag, and fail if `Cargo.toml` or `package.json` versions do not match.

- [ ] **Step 3: Add target matrix builds**

Build and package these assets:

- `avu-x86_64-unknown-linux-musl.tar.gz`
- `avu-aarch64-unknown-linux-musl.tar.gz`
- `avu-x86_64-apple-darwin.tar.gz`
- `avu-aarch64-apple-darwin.tar.gz`
- `avu-x86_64-pc-windows-msvc.zip`
- `avu-aarch64-pc-windows-msvc.zip`

Each archive must contain only `avu` or `avu.exe` at archive root.

- [ ] **Step 4: Add provenance attestations**

Run `actions/attest@v4` for every produced archive before uploading it as a workflow artifact.

- [ ] **Step 5: Add SBOM generation and attestation**

Generate `avu-${version}.cdx.json` with `anchore/sbom-action@v0`, upload it as a workflow artifact, then attest it in the publish job against `SHA256SUMS` using `actions/attest@v4` with `subject-checksums` and `sbom-path`.

- [ ] **Step 6: Add checksum signing and release upload**

Download all artifacts, generate `SHA256SUMS`, sign it with keyless `cosign sign-blob --yes --output-signature SHA256SUMS.sig --output-certificate SHA256SUMS.pem SHA256SUMS`, and upload all assets through `gh release create/upload --verify-tag --clobber`.

## Task 3: Add installer checksum verification

**Files:**
- Modify: `npm/install.js`
- Modify: `scripts/install.sh`
- Modify: `scripts/install.ps1`

- [ ] **Step 1: Verify npm archive downloads**

In `npm/install.js`, download `SHA256SUMS`, compute the archive SHA256 with Node `crypto`, find the matching line for the selected archive basename, and fail if the expected and actual hashes differ.

- [ ] **Step 2: Verify shell archive downloads**

In `scripts/install.sh`, download `SHA256SUMS`, compute the archive hash with `sha256sum` or `shasum -a 256`, and fail before extraction if the selected archive does not match.

- [ ] **Step 3: Verify PowerShell archive downloads**

In `scripts/install.ps1`, download `SHA256SUMS`, compute the archive hash with `Get-FileHash -Algorithm SHA256`, and fail before extraction if the selected archive does not match.

## Task 4: Document secure maintainer release flow

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add release documentation**

Document tag creation, produced assets, checksum verification, GitHub attestation verification, and cosign checksum signature verification.

- [ ] **Step 2: Confirm documentation anchor exists**

Run: `grep -n "Maintainer release flow" README.md`

Expected: one matching line.

## Task 5: Local verification

**Files:**
- Check: `.github/workflows/ci.yml`
- Check: `.github/workflows/release.yml`
- Check: `npm/install.js`
- Check: `scripts/install.sh`
- Check: `scripts/install.ps1`
- Check: `README.md`

- [ ] **Step 1: Run local command checks**

Run: `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets --all-features && cargo build --release && node --check npm/install.js && node --check npm/bin/avu.js && bash -n scripts/install.sh && npm pack --dry-run`

Expected: exit code 0.

- [ ] **Step 2: Inspect git diff**

Run: `git diff -- .github npm scripts README.md docs/superpowers`

Expected: CI/release workflows, installer checksum verification, README release docs, spec, and plan changes.
