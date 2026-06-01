mod app;
mod backend;
mod cli;
mod config;
mod doctor;
mod domain;
mod engine;
mod hud;
mod intent;
mod runtime;
mod setup;
mod tui;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Tui(cli::TuiArgs::default())) {
        Command::Setup(args) => setup::run(args),
        Command::Doctor(args) => doctor::run(args),
        Command::Status(args) => app::print_status(args),
        Command::Config(args) => config::print_effective(args),
        Command::Tui(args) => tui::run(args),
    }
}
