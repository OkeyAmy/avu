use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "avu",
    version,
    about = "Wake-capable terminal cockpit over Hermes/OpenClaw"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the guided terminal setup checks.
    Setup(SetupArgs),
    /// Diagnose backend, capability, config, and wake readiness.
    Doctor(DoctorArgs),
    /// Print backend and cockpit status.
    Status(StatusArgs),
    /// Print Avu config paths and effective defaults.
    Config(ConfigArgs),
    /// Render the Avu cockpit from a live adapter or fixture.
    Tui(TuiArgs),
    /// Show installation state, check PATH, or repair self-install.
    Install(InstallArgs),
}

#[derive(Debug, Args, Clone)]
pub struct SetupArgs {
    /// Only check missing/invalid required settings.
    #[arg(long)]
    pub quick: bool,
    /// Backend to configure.
    #[arg(long, value_enum, default_value_t = BackendChoice::Auto)]
    pub backend: BackendChoice,
    /// Do not prompt; choose safe defaults and print next steps.
    #[arg(long)]
    pub non_interactive: bool,
}

#[derive(Debug, Args, Clone)]
pub struct DoctorArgs {
    /// Backend to diagnose.
    #[arg(long, value_enum, default_value_t = BackendChoice::Auto)]
    pub backend: BackendChoice,
    /// Emit JSON for automation.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args, Clone)]
pub struct StatusArgs {
    /// Backend to inspect.
    #[arg(long, value_enum, default_value_t = BackendChoice::Auto)]
    pub backend: BackendChoice,
    /// Emit JSON for automation.
    #[arg(long)]
    pub json: bool,
    /// Load state from a fixture instead of a live backend.
    #[arg(long)]
    pub fixture: Option<PathBuf>,
}

#[derive(Debug, Args, Clone, Default)]
pub struct ConfigArgs {
    /// Emit JSON for automation.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args, Clone, Default)]
pub struct TuiArgs {
    /// Load state from a fixture instead of a live backend.
    #[arg(long)]
    pub fixture: Option<PathBuf>,
    /// Render one frame and exit. Useful for tests and CI.
    #[arg(long)]
    pub once: bool,
    /// Backend to inspect when no fixture is supplied.
    #[arg(long, value_enum, default_value_t = BackendChoice::Auto)]
    pub backend: BackendChoice,
}

#[derive(Debug, Args, Clone, Default)]
pub struct InstallArgs {
    /// Only run install checks; exit non-zero on failure.
    #[arg(long)]
    pub check: bool,
    /// Copy avu binary to ~/.local/bin and print PATH repair instructions.
    #[arg(long)]
    pub fix_self: bool,
    /// Emit JSON install report for automation.
    #[arg(long)]
    pub info: bool,
}

#[derive(
    Debug, Copy, Clone, Eq, PartialEq, ValueEnum, serde::Serialize, serde::Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum BackendChoice {
    #[default]
    Auto,
    Fake,
    Hermes,
    Openclaw,
    Remote,
}
