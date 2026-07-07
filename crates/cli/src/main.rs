mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};

use commands::replay::{run, ReplayArgs};

#[derive(Parser)]
#[command(name = "engine-cli")]
#[command(about = "Event-driven trading engine CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Replay(ReplayArgs),
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Command::Replay(args) => run(args),
    }
}
