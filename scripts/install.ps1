#Requires -Version 5.1
<#
.SYNOPSIS
  Avu — one-line installer for Windows (PowerShell)

.DESCRIPTION
  Installs Avu on Windows by:
  1. Detecting OS architecture
  2. Detecting existing Hermes/OpenClaw on PATH
  3. Downloading the avu binary or installing the Node/npm wrapper
  4. Installing to a user-local PATH directory
  5. Creating ~/.avu/ config directory structure
  6. Running `avu setup` unless -NoSetup is passed

.EXAMPLE
  iwr -useb https://raw.githubusercontent.com/OkeyAmy/avu/master/scripts/install.ps1 | iex
#>

param(
  [string]$Version = "latest",
  [switch]$NoSetup
)

$ErrorActionPreference = "Stop"

$AvuRepo = "https://github.com/OkeyAmy/avu"
$AvuHome = "$env:USERPROFILE\.avu"
$AvuBinDir = "$env:USERPROFILE\.local\bin"

function Write-Info($msg)  { Write-Host -ForegroundColor Blue  "➜ $msg" }
function Write-Ok($msg)    { Write-Host -ForegroundColor Green "✓ $msg" }
function Write-Warn($msg)  { Write-Host -ForegroundColor Yellow "⚠ $msg" }
function Write-Err($msg)   { Write-Host -ForegroundColor Red   "✗ $msg" }

# ── Detect architecture ────────────────────────────────────────────────────
$Arch = switch -regex ($env:PROCESSOR_ARCHITECTURE) {
  "AMD64|X64" { "x86_64" }
  "ARM64"     { "aarch64" }
  default     { "unknown:$env:PROCESSOR_ARCHITECTURE" }
}

Write-Info "OS:           Windows"
Write-Info "Architecture: $Arch"
Write-Host ""

# ── Detect backends ─────────────────────────────────────────────────────────
$Backends = @()

$HermesCmd = Get-Command hermes -ErrorAction SilentlyContinue
if ($HermesCmd) {
  $Backends += "hermes"
  Write-Ok "Hermes detected: $($HermesCmd.Source)"
} else {
  Write-Warn "Hermes not found on PATH"
}

$OpenClawCmd = Get-Command openclaw -ErrorAction SilentlyContinue
if ($OpenClawCmd) {
  $Backends += "openclaw"
  Write-Ok "OpenClaw detected: $($OpenClawCmd.Source)"
} else {
  Write-Warn "OpenClaw not found on PATH"
}

if ($Backends.Count -eq 0) {
  Write-Warn "No agent backend detected. Avu will run in disconnected mode."
  Write-Info "Install Hermes:  iex (irm https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.ps1)"
  Write-Info "Install OpenClaw: iwr -useb https://openclaw.ai/install.ps1 | iex"
}

Write-Host ""

# ── Detect backend-owned runtimes ──────────────────────────────────────────
Write-Info "Checking backend-owned runtimes..."
$NodeCmd = Get-Command node -ErrorAction SilentlyContinue
if ($NodeCmd) {
  $NodeVersion = & node --version 2>$null
  Write-Ok "node: $NodeVersion ($($NodeCmd.Source))"
} else {
  Write-Warn "node not found on PATH; Hermes/OpenClaw installers can provide it"
}

$NpmCmd = Get-Command npm -ErrorAction SilentlyContinue
if ($NpmCmd) {
  $NpmVersion = & npm --version 2>$null
  Write-Ok "npm: $NpmVersion ($($NpmCmd.Source))"
} else {
  Write-Warn "npm not found on PATH; install Hermes/OpenClaw first or use a prebuilt Avu release"
}

Write-Host ""

# ── Create directory structure ──────────────────────────────────────────────
Write-Info "Creating Avu directory structure at $AvuHome..."
New-Item -ItemType Directory -Force -Path $AvuHome | Out-Null
New-Item -ItemType Directory -Force -Path "$AvuHome\logs" | Out-Null
New-Item -ItemType Directory -Force -Path "$AvuHome\fixtures" | Out-Null
Write-Ok "Created $AvuHome\"
Write-Ok "Created $AvuHome\logs\"
Write-Ok "Created $AvuHome\fixtures\"

# ── Install binary ──────────────────────────────────────────────────────────
Write-Info "Installing avu binary..."

New-Item -ItemType Directory -Force -Path $AvuBinDir | Out-Null

$Suffix = "pc-windows-msvc"
$ArchiveName = "avu-$Arch-$Suffix"

$DownloadUrl = if ($Version -eq "latest") {
  "$AvuRepo/releases/latest/download/$ArchiveName.zip"
} else {
  "$AvuRepo/releases/download/v$Version/$ArchiveName.zip"
}

$ChecksumsUrl = if ($Version -eq "latest") {
  "$AvuRepo/releases/latest/download/SHA256SUMS"
} else {
  "$AvuRepo/releases/download/v$Version/SHA256SUMS"
}

$TmpDir = Join-Path $env:TEMP "avu-install-$(Get-Random)"
New-Item -ItemType Directory -Force -Path $TmpDir | Out-Null

$Downloaded = $false
try {
  Write-Info "Attempting to download pre-built binary..."
  Invoke-WebRequest -Uri $DownloadUrl -OutFile "$TmpDir\avu.zip" -UseBasicParsing
  Invoke-WebRequest -Uri $ChecksumsUrl -OutFile "$TmpDir\SHA256SUMS" -UseBasicParsing
  $ExpectedLine = Get-Content "$TmpDir\SHA256SUMS" | Where-Object { $_ -match "\s$ArchiveName\.zip$" } | Select-Object -First 1
  if (-not $ExpectedLine) {
    throw "SHA256SUMS did not contain $ArchiveName.zip"
  }
  $ExpectedHash = ($ExpectedLine -split "\s+")[0].ToLowerInvariant()
  $ActualHash = (Get-FileHash -Algorithm SHA256 "$TmpDir\avu.zip").Hash.ToLowerInvariant()
  if ($ExpectedHash -ne $ActualHash) {
    throw "Checksum mismatch for $ArchiveName.zip. Expected $ExpectedHash, got $ActualHash"
  }
  Expand-Archive -Path "$TmpDir\avu.zip" -DestinationPath $TmpDir -Force
  Copy-Item "$TmpDir\avu.exe" "$AvuBinDir\avu.exe" -Force
  $Downloaded = $true
  Write-Ok "Downloaded avu to $AvuBinDir\avu.exe"
} catch {
  Write-Warn "Pre-built binary download failed. Trying Node/npm wrapper..."
}

if (-not $Downloaded) {
  $NpmCmd = Get-Command npm -ErrorAction SilentlyContinue
  if ($NpmCmd) {
    Write-Info "Installing through npm wrapper..."
    if ($Version -eq "latest") {
      npm install -g --prefix $AvuBinDir "github:OkeyAmy/avu#master"
    } else {
      $env:AVU_INSTALL_VERSION = $Version
      npm install -g --prefix $AvuBinDir "github:OkeyAmy/avu#v$Version"
    }
    Write-Ok "Installed Avu through npm wrapper"
  } else {
    Write-Err "No Avu prebuilt binary and npm is not available."
    Write-Err "Install Hermes or OpenClaw first so their supported Node/npm runtime is available:"
    Write-Err "  Hermes:  iex (irm https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.ps1)"
    Write-Err "  OpenClaw: iwr -useb https://openclaw.ai/install.ps1 | iex"
    Write-Err "Rust/Cargo is intentionally not required for normal Avu users."
    exit 1
  }
}

Remove-Item -Recurse -Force $TmpDir

# ── Ensure PATH ─────────────────────────────────────────────────────────────
$CurrentPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($CurrentPath -notlike "*$AvuBinDir*") {
  Write-Warn "$AvuBinDir is NOT on user PATH"
  Write-Info "Adding to user PATH..."
  [Environment]::SetEnvironmentVariable("Path", "$AvuBinDir;$CurrentPath", "User")
  $env:Path = "$AvuBinDir;$env:Path"
  Write-Ok "Added $AvuBinDir to user PATH"
  Write-Warn "Open a new terminal for PATH to take effect."
} else {
  Write-Ok "$AvuBinDir is on PATH"
}

# ── Verify ──────────────────────────────────────────────────────────────────
Write-Info "Verifying installation..."
$AvuCmd = Get-Command avu -ErrorAction SilentlyContinue
if ($AvuCmd) {
  $Ver = & avu --version 2>$null
  Write-Ok "avu $Ver installed at $($AvuCmd.Source)"
} else {
  Write-Err "avu not found on PATH after install"
  Write-Err "Open a new terminal and run: avu --version"
  exit 1
}

Write-Host ""
Write-Host -ForegroundColor Green "Avu installed successfully!"
Write-Host ""

# ── Run setup ───────────────────────────────────────────────────────────────
if (-not $NoSetup) {
  Write-Info "Running Avu setup wizard..."
  Write-Host ""
  & avu setup
  if ($LASTEXITCODE -eq 0) {
    Write-Ok "Setup complete."
  } else {
    Write-Warn "Setup exited with an error. Run 'avu setup' manually to retry."
  }
} else {
  Write-Info "Skipping setup (-NoSetup flag). Run 'avu setup' when ready."
}

# ── Next steps ──────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "Next steps:" -ForegroundColor White
Write-Host "  avu doctor              # Diagnose backend, config, and wake readiness"
Write-Host "  avu status              # Show backend and cockpit status"
Write-Host "  avu tui                 # Launch the cockpit"
Write-Host ""

if ($Backends.Count -eq 0) {
  Write-Host "No agent backend detected." -ForegroundColor Yellow
  Write-Host "  Install Hermes:  https://github.com/NousResearch/hermes-agent"
  Write-Host "  Install OpenClaw: https://openclaw.ai"
  Write-Host "  Then re-run: avu setup --quick"
  Write-Host ""
}
