mod adapters;
mod cli;
mod commands;
mod config;
mod error;
mod invoice;
mod ports;
mod sanitize;
#[cfg(test)]
mod testing;

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
    let env = adapters::env::StdEnv;
    let config = config::load(cli.config.as_deref(), &env)?;
    let platform = adapters::prod::ProdPlatform::new(&config);

    match cli.command {
        Command::Add(args) => commands::add::run(args, &config, &platform),
        Command::Insert(args) => commands::insert::run(args, &config, &platform),
        Command::Queue(args) => commands::queue::run(args, &config, &platform),
        Command::Validate(args) => commands::validate::run(args, &config, &platform),
        Command::Report(args) => commands::report::run(args, &config, &platform),
        Command::Search(args) => commands::search::run(args, &config, &platform),
        Command::Sync(args) => commands::sync::run(args, &config, &platform),
    }
}
