mod cli;
mod commands;
mod config;
mod error;
mod invoice;
mod net;
mod sanitize;

use clap::Parser;
use cli::{Cli, Command};
use error::Result;

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let config = config::load(cli.config.as_deref())?;

    match cli.command {
        Command::Add(args) => commands::add::run(args, &config),
        Command::Insert(args) => commands::insert::run(args, &config),
        Command::Queue(args) => commands::queue::run(args, &config),
        Command::Validate(args) => commands::validate::run(args, &config),
        Command::Report(args) => commands::report::run(args, &config),
        Command::Search(args) => commands::search::run(args, &config),
        Command::Sync(args) => commands::sync::run(args, &config),
    }
}
