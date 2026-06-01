#!/usr/bin/env bash
# =============================================================================
# Avu — one-line installer for Linux, macOS, and WSL2
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/OkeyAmy/avu/master/scripts/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/OkeyAmy/avu/master/scripts/install.sh | bash -s -- --no-setup
#   curl -fsSL https://raw.githubusercontent.com/OkeyAmy/avu/master/scripts/install.sh | bash -s -- --version 0.2.0
#
# What this script does:
#   1. Detects OS, architecture, and shell
#   2. Detects existing Hermes/OpenClaw installations on PATH
#   3. Downloads the avu binary or installs the Node/npm wrapper
#   4. Installs to ~/.local/bin/avu (or a user-local PATH directory)
#   5. Ensures ~/.local/bin is on PATH (prints shell-specific repair if not)
#   6. Creates ~/.avu/ config directory structure
#   7. Runs `avu setup` unless --no-setup is passed
# =============================================================================

set -euo pipefail

AVU_REPO="https://github.com/OkeyAmy/avu"
AVU_BIN_DIR="${HOME}/.local/bin"
AVU_HOME="${HOME}/.avu"
AVU_VERSION="${AVU_INSTALL_VERSION:-latest}"
SKIP_SETUP=false

# ─── Colors ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
RESET='\033[0m'

info()  { printf "${BLUE}➜${RESET} %s\n" "$*"; }
ok()    { printf "${GREEN}✓${RESET} %s\n" "$*"; }
warn()  { printf "${YELLOW}⚠${RESET} %s\n" "$*"; }
err()   { printf "${RED}✗${RESET} %s\n" "$*" >&2; }

# ─── Argument parsing ────────────────────────────────────────────────────────
for arg in "$@"; do
  case "$arg" in
    --no-setup)  SKIP_SETUP=true ;;
    --version=*) AVU_VERSION="${arg#--version=}" ;;
    --help|-h)
      echo "Usage: install.sh [OPTIONS]"
      echo ""
      echo "Options:"
      echo "  --no-setup       Skip running 'avu setup' after install"
      echo "  --version=VER    Install specific version (default: latest)"
      echo "  --help           Show this help message"
      exit 0
      ;;
    *)
      err "Unknown argument: $arg"
      err "Run with --help for usage."
      exit 1
      ;;
  esac
done

# ─── Step 1: Detect OS ──────────────────────────────────────────────────────
detect_os() {
  local uname_out
  uname_out="$(uname -s)"

  case "${uname_out}" in
    Linux*)
      # Check for WSL2
      if grep -qi microsoft /proc/version 2>/dev/null; then
        echo "wsl2"
      else
        echo "linux"
      fi
      ;;
    Darwin*)  echo "macos" ;;
    MINGW*|MSYS*|CYGWIN*) echo "windows_git_bash" ;;
    *)        echo "unknown:${uname_out}" ;;
  esac
}

# ─── Step 2: Detect architecture ─────────────────────────────────────────────
detect_arch() {
  local arch
  arch="$(uname -m)"

  case "${arch}" in
    x86_64|amd64)   echo "x86_64" ;;
    aarch64|arm64)  echo "aarch64" ;;
    armv7l)         echo "armv7" ;;
    *)              echo "unknown:${arch}" ;;
  esac
}

# ─── Step 3: Detect shell ────────────────────────────────────────────────────
detect_shell() {
  local shell_name
  shell_name="$(basename "${SHELL:-/bin/bash}")"

  case "${shell_name}" in
    bash)  echo "bash" ;;
    zsh)   echo "zsh" ;;
    fish)  echo "fish" ;;
    *)     echo "other:${shell_name}" ;;
  esac
}

# ─── Step 4: Detect existing backends ────────────────────────────────────────
detect_backends() {
  local backends=""

  if command -v hermes &>/dev/null; then
    backends="${backends}hermes "
    ok "Hermes detected on PATH: $(command -v hermes)"
  else
    warn "Hermes not found on PATH"
  fi

  if command -v openclaw &>/dev/null; then
    backends="${backends}openclaw "
    ok "OpenClaw detected on PATH: $(command -v openclaw)"
  else
    warn "OpenClaw not found on PATH"
  fi

  if [ -z "${backends}" ]; then
    warn "No agent backend detected. Avu will run in disconnected mode."
    info "Install Hermes:  curl -fsSL https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.sh | bash"
    info "Install OpenClaw: curl -fsSL https://openclaw.ai/install.sh | bash"
  fi

  printf "%s" "${backends}" | xargs
}

# ─── Step 5: Detect backend-owned runtimes ────────────────────────────────────
detect_deps() {
  info "Checking backend-owned runtimes..."

  if command -v node &>/dev/null; then
    ok "node: $(node --version 2>/dev/null) ($(command -v node))"
  else
    warn "node not found on PATH; Hermes/OpenClaw installers can provide it"
  fi

  if command -v npm &>/dev/null; then
    ok "npm: $(npm --version 2>/dev/null) ($(command -v npm))"
  else
    warn "npm not found on PATH; install Hermes/OpenClaw first or use a prebuilt Avu release"
  fi
}

# ─── Step 6: Create directory structure ───────────────────────────────────────
create_dirs() {
  info "Creating Avu directory structure at ${AVU_HOME}..."

  mkdir -p "${AVU_HOME}"
  mkdir -p "${AVU_HOME}/logs"
  mkdir -p "${AVU_HOME}/fixtures"

  ok "Created ${AVU_HOME}/"
  ok "Created ${AVU_HOME}/logs/"
  ok "Created ${AVU_HOME}/fixtures/"
}

sha256_file() {
  local file="$1"
  if command -v sha256sum &>/dev/null; then
    sha256sum "${file}" | awk '{print $1}'
  elif command -v shasum &>/dev/null; then
    shasum -a 256 "${file}" | awk '{print $1}'
  else
    err "Cannot verify checksum: sha256sum or shasum is required."
    return 1
  fi
}

verify_archive() {
  local archive_path="$1"
  local archive_name="$2"
  local checksums_path="$3"
  local expected actual

  expected="$(awk -v name="${archive_name}" '$2 == name {print $1}' "${checksums_path}")"
  if [ -z "${expected}" ]; then
    err "SHA256SUMS did not contain ${archive_name}"
    return 1
  fi

  actual="$(sha256_file "${archive_path}")"
  if [ "${expected}" != "${actual}" ]; then
    err "Checksum mismatch for ${archive_name}"
    err "Expected: ${expected}"
    err "Actual:   ${actual}"
    return 1
  fi
}

append_path_once() {
  local config_file="$1"
  local line="$2"
  local marker="$3"

  mkdir -p "$(dirname "${config_file}")"
  touch "${config_file}"

  if grep -v '^[[:space:]]*#' "${config_file}" 2>/dev/null | grep -Fq "${AVU_BIN_DIR}"; then
    ok "${AVU_BIN_DIR} already configured in ${config_file}"
    return 0
  fi

  {
    printf '\n# %s\n' "${marker}"
    printf '%s\n' "${line}"
  } >> "${config_file}"
  ok "Added ${AVU_BIN_DIR} to PATH in ${config_file}"
}

# ─── Step 7: Install binary ──────────────────────────────────────────────────
install_binary() {
  info "Installing avu binary..."

  # Ensure bin directory exists
  mkdir -p "${AVU_BIN_DIR}"

  # Try to download a pre-built release first; fall back to the Node/npm wrapper
  local os arch suffix
  os="$(detect_os)"
  arch="$(detect_arch)"

  case "${os}" in
    linux|wsl2)  suffix="unknown-linux-musl" ;;
    macos)       suffix="apple-darwin" ;;
    *)           suffix="" ;;
  esac

  local archive_base="avu-${arch}-${suffix}"
  local archive_name="${archive_base}.tar.gz"
  local download_url="${AVU_REPO}/releases/download/v${AVU_VERSION}/${archive_name}"
  local checksums_url="${AVU_REPO}/releases/download/v${AVU_VERSION}/SHA256SUMS"
  local tmp_dir
  tmp_dir="$(mktemp -d)"

  if [ "${AVU_VERSION}" = "latest" ]; then
    download_url="${AVU_REPO}/releases/latest/download/${archive_name}"
    checksums_url="${AVU_REPO}/releases/latest/download/SHA256SUMS"
  fi

  if command -v curl &>/dev/null; then
    info "Attempting to download pre-built binary..."
    if curl -fsSL "${download_url}" -o "${tmp_dir}/${archive_name}" 2>/dev/null; then
      curl -fsSL "${checksums_url}" -o "${tmp_dir}/SHA256SUMS"
      verify_archive "${tmp_dir}/${archive_name}" "${archive_name}" "${tmp_dir}/SHA256SUMS"
      tar -xzf "${tmp_dir}/${archive_name}" -C "${tmp_dir}"
      mv "${tmp_dir}/avu" "${AVU_BIN_DIR}/avu"
      chmod +x "${AVU_BIN_DIR}/avu"
      ok "Downloaded avu to ${AVU_BIN_DIR}/avu"
      rm -rf "${tmp_dir}"
      return 0
    fi
  fi

  warn "Pre-built binary not available for ${os}/${arch}. Trying Node/npm wrapper..."
  if command -v npm &>/dev/null; then
    if [ "${AVU_VERSION}" = "latest" ]; then
      npm install -g --prefix "${HOME}/.local" "github:OkeyAmy/avu#master"
    else
      AVU_INSTALL_VERSION="${AVU_VERSION}" npm install -g --prefix "${HOME}/.local" "github:OkeyAmy/avu#v${AVU_VERSION}"
    fi
    ok "Installed Avu through npm wrapper"
  else
    err "No Avu prebuilt binary and npm is not available."
    err "Install Hermes or OpenClaw first so their supported Node/npm runtime is available:"
    err "  Hermes:  curl -fsSL https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.sh | bash"
    err "  OpenClaw: curl -fsSL https://openclaw.ai/install.sh | bash"
    err "Rust/Cargo is intentionally not required for normal Avu users."
    exit 1
  fi

  rm -rf "${tmp_dir}"
}

# ─── Step 8: Ensure PATH ─────────────────────────────────────────────────────
ensure_path() {
  if echo ":${PATH}:" | grep -q ":${AVU_BIN_DIR}:"; then
    ok "${AVU_BIN_DIR} is on PATH"
    return 0
  fi

  local shell_type
  shell_type="$(detect_shell)"

  warn "${AVU_BIN_DIR} is NOT on PATH"
  info "Adding ${AVU_BIN_DIR} to PATH for current session..."

  export PATH="${AVU_BIN_DIR}:${PATH}"

  local path_line
  path_line="export PATH=\"${AVU_BIN_DIR}:\$PATH\""

  case "${shell_type}" in
    bash)
      append_path_once "${HOME}/.bashrc" "${path_line}" "Avu — ensure ${AVU_BIN_DIR} is on PATH"
      append_path_once "${HOME}/.profile" "${path_line}" "Avu — ensure ${AVU_BIN_DIR} is on PATH for login shells"
      ;;
    zsh)
      append_path_once "${HOME}/.zshrc" "${path_line}" "Avu — ensure ${AVU_BIN_DIR} is on PATH"
      append_path_once "${HOME}/.zprofile" "${path_line}" "Avu — ensure ${AVU_BIN_DIR} is on PATH for login shells"
      ;;
    fish)
      append_path_once "${HOME}/.config/fish/config.fish" "fish_add_path ${AVU_BIN_DIR}" "Avu — ensure ${AVU_BIN_DIR} is on PATH"
      ;;
    *)
      append_path_once "${HOME}/.profile" "${path_line}" "Avu — ensure ${AVU_BIN_DIR} is on PATH"
      ;;
  esac

  echo ""
  warn "Open a new terminal or run: export PATH=\"${AVU_BIN_DIR}:\$PATH\""
}

# ─── Step 9: Verify installation ─────────────────────────────────────────────
verify_install() {
  info "Verifying installation..."

  if command -v avu &>/dev/null; then
    local version
    version="$(avu --version 2>/dev/null || echo "unknown")"
    ok "avu ${version} installed at $(command -v avu)"
  else
    err "avu not found on PATH after install"
    err "Try: source ~/.bashrc (or ~/.zshrc) and then: avu --version"
    exit 1
  fi
}

# ─── Step 10: Run setup ──────────────────────────────────────────────────────
run_setup() {
  if [ "${SKIP_SETUP}" = true ]; then
    info "Skipping setup (--no-setup flag)."
    info "Run 'avu setup' when ready."
    return 0
  fi

  info "Running Avu setup wizard..."
  echo ""
  if avu setup; then
    ok "Setup complete."
  else
    warn "Setup exited with an error. Run 'avu setup' manually to retry."
  fi
}

# ─── Main ─────────────────────────────────────────────────────────────────────
main() {
  echo ""
  printf "${BOLD}Avu — Wake-capable terminal cockpit${RESET}"
  echo ""
  echo ""

  # Detect environment
  local os arch shell_type backends
  os="$(detect_os)"
  arch="$(detect_arch)"
  shell_type="$(detect_shell)"

  info "OS:          ${os}"
  info "Architecture: ${arch}"
  info "Shell:       ${shell_type}"
  echo ""

  # Detect backends and deps
  backends="$(detect_backends)"
  echo ""
  detect_deps
  echo ""

  # Install
  create_dirs
  install_binary
  ensure_path
  verify_install

  echo ""
  printf "${GREEN}${BOLD}Avu installed successfully!${RESET}"
  echo ""

  # Run setup
  run_setup

  # Print next steps
  echo ""
  printf "${BOLD}Next steps:${RESET}"
  echo ""
  echo "  avu doctor              # Diagnose backend, config, and wake readiness"
  echo "  avu status              # Show backend and cockpit status"
  echo "  avu tui                 # Launch the cockpit"
  echo ""

  if [ -z "${backends}" ]; then
    printf "${YELLOW}${BOLD}No agent backend detected.${RESET}"
    echo ""
    echo "  Install Hermes:  https://github.com/NousResearch/hermes-agent"
    echo "  Install OpenClaw: https://openclaw.ai"
    echo "  Then re-run: avu setup --quick"
    echo ""
  fi
}

main "$@"
