# Avu Release CI Design

## Goal

Create a GitHub Actions pipeline that makes Avu installable without requiring Rust or Cargo on user machines. The pipeline must build native Avu binaries, package them with the exact asset names expected by the shell installer, PowerShell installer, and npm wrapper, and attach them to GitHub Releases when a version tag is pushed.

## Current install contract

Avu's normal install path is a prebuilt native binary, optionally reached through the Node/npm wrapper. The installers and npm wrapper expect these release assets:

- `avu-x86_64-unknown-linux-musl.tar.gz`
- `avu-aarch64-unknown-linux-musl.tar.gz`
- `avu-x86_64-apple-darwin.tar.gz`
- `avu-aarch64-apple-darwin.tar.gz`
- `avu-x86_64-pc-windows-msvc.zip`
- `avu-aarch64-pc-windows-msvc.zip`

The first production release should support all six names so install behavior is predictable on Linux, macOS, Windows, Intel/AMD64, and ARM64 hosts. If a cross-target becomes unreliable, the release workflow should fail visibly instead of silently producing a partial release.

## Approaches considered

### Option A: GitHub Release binaries only

This approach runs quality checks on pull requests and builds/upload release assets on `v*` tags using only the built-in `GITHUB_TOKEN`. It requires no npm registry token, signing key, or external account setup. It matches the current installer design because users can already install from GitHub URLs.

Trade-off: users who want `npm install -g @okeyamy/avu` from the public npm registry must wait until a later pipeline adds npm publishing.

### Option B: GitHub Release binaries plus npm publishing

This adds `npm publish` after release assets are created. It gives users the cleanest npm command, but it requires an npm account, package ownership, and an `NPM_TOKEN` secret. A broken npm publish should not block binary releases, so this would add more release-state complexity.

Trade-off: better distribution later, more credentials and failure modes now.

### Option C: signed/provenance-heavy releases

This adds artifact signing, provenance attestations, SBOMs, and stricter supply-chain metadata. It is the strongest security posture, but it adds tools and policies before the first public release has a stable binary contract.

Trade-off: best long-term security, slower first release.

## Decision

Use Option C now, but keep it practical and GitHub-native. The release pipeline will publish native archives, `SHA256SUMS`, a CycloneDX SBOM, keyless Sigstore signatures for the checksum manifest, and GitHub artifact attestations for archives and the SBOM. Normal installs remain lightweight: users do not need Rust, Cargo, cosign, or SBOM tooling. Installers verify SHA256 hashes before extraction; provenance and SBOM attestations are available for maintainers and security-conscious users.

## Pipeline architecture

### 1. Continuous integration workflow

The CI workflow runs on every pull request and push to `master`. It validates the repo without creating release assets:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features`
- `cargo build --release`
- `node --check npm/install.js`
- `node --check npm/bin/avu.js`
- `bash -n scripts/install.sh`
- `npm pack --dry-run`

This catches Rust, Node wrapper, shell installer, and npm package mistakes before tags are cut.

### 2. Release workflow

The release workflow runs on `v*` tags and manual dispatch. It has three phases:

1. Validate the tag and package metadata.
2. Build a target matrix for Linux, macOS, and Windows.
3. Create/update the GitHub Release and upload archives plus verification assets.

The workflow should grant only the permissions it needs:

```yaml
permissions:
  contents: write
  id-token: write
  attestations: write
```

Tag-triggered builds are the primary release path. Manual dispatch is allowed only when it builds an existing tag: every checkout in the release workflow must use the resolved tag so a manual run cannot accidentally publish default-branch code under a release version.

### 3. Build matrix

The release matrix maps each target to the asset name the installers already expect:

| OS runner | Rust target | Archive |
| --- | --- | --- |
| `ubuntu-latest` | `x86_64-unknown-linux-musl` | `.tar.gz` |
| `ubuntu-24.04-arm` | `aarch64-unknown-linux-musl` | `.tar.gz` |
| `macos-13` | `x86_64-apple-darwin` | `.tar.gz` |
| `macos-14` | `aarch64-apple-darwin` | `.tar.gz` |
| `windows-latest` | `x86_64-pc-windows-msvc` | `.zip` |
| `windows-latest` | `aarch64-pc-windows-msvc` | `.zip` |

Linux uses musl because `npm/install.js` and `scripts/install.sh` already request `unknown-linux-musl`. macOS uses separate Intel and Apple Silicon runners to avoid brittle cross-linking. Windows ARM64 is included because the installer maps ARM64 to `aarch64-pc-windows-msvc`; if that target proves unsupported on GitHub-hosted Windows runners, we should either fix the target or explicitly remove ARM64 Windows from installer support.

### 4. Packaging contract

Each archive contains exactly one executable at its root:

- Unix archives contain `avu`
- Windows archives contain `avu.exe`

This matches `npm/install.js`, which extracts the archive into `npm/bin/native` and then checks for the executable by name.

### 5. Checksums, signing, SBOM, and attestations

The release workflow writes `SHA256SUMS` covering every archive and the SBOM. It signs that checksum manifest using keyless Sigstore/cosign OIDC, producing:

- `SHA256SUMS`
- `SHA256SUMS.sig`
- `SHA256SUMS.pem`

The workflow also generates `avu-<version>.cdx.json` as a CycloneDX SBOM. GitHub artifact attestations are created for each archive and for the SBOM. These attestations can be verified with GitHub CLI, while the signature can be verified with cosign by maintainers or advanced users.

Installers should download `SHA256SUMS` and verify the selected archive hash before extracting. They should not require cosign verification locally because that would make normal installs heavier and less reliable.

### 6. Version consistency

For tag `v0.1.0`, these files should contain `0.1.0`:

- `Cargo.toml` package version
- `package.json` version

The release workflow should fail if the tag and metadata disagree. That prevents publishing an asset whose binary version and npm wrapper version diverge.

## Error handling

- PR CI failure blocks merge.
- Release build failure blocks release upload.
- If upload fails after a release is created, rerunning the tag workflow should overwrite/replace assets instead of requiring manual cleanup.
- No npm registry publish step exists in this version, so missing npm secrets cannot break the release.
- If SBOM generation, attestation, or checksum signing fails, the release must fail instead of publishing a weaker partial release.

## Testing strategy

Local validation before committing the workflows:

- Parse workflow YAML if tooling is available.
- Run the same local checks as CI: Rust fmt/clippy/test/build, Node syntax checks, shell syntax check, and npm dry pack.

Remote validation after pushing:

- Open a PR and verify the CI workflow passes.
- Push a test tag such as `v0.1.0` and verify all six archives, `avu-0.1.0.cdx.json`, `SHA256SUMS`, `SHA256SUMS.sig`, and `SHA256SUMS.pem` appear on the GitHub Release.
- Verify one archive attestation with `gh attestation verify <archive> -R OkeyAmy/avu`.
- Verify the checksum signature with `cosign verify-blob --certificate SHA256SUMS.pem --signature SHA256SUMS.sig SHA256SUMS`.
- Run `npm install -g github:OkeyAmy/avu#master` on a fresh machine with Node/npm available and verify `avu install --check` passes.

## Out of scope for this iteration

- Publishing to npmjs.com.
- Code signing or notarization.
- Apple notarization, Windows Authenticode, and paid signing certificates.
- Mandatory install-time cosign verification.
