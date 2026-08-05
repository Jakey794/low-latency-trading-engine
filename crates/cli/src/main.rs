mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};

use commands::{run_replay, run_strategy_replay, ReplayArgs, StrategyReplayArgs};

#[derive(Parser)]
#[command(name = "engine-cli")]
#[command(about = "Event-driven trading engine CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Replay JSONL events through the matching engine (Week 5 path).
    Replay(ReplayArgs),
    /// Replay JSONL events through the runtime with a built-in strategy.
    StrategyReplay(StrategyReplayArgs),
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Command::Replay(args) => run_replay(args),
        Command::StrategyReplay(args) => run_strategy_replay(args),
    }
}
