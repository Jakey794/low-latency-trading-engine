mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};

use commands::{
    run_benchmark_report, run_replay, run_simulate, run_strategy_replay, run_websocket_demo,
    BenchmarkReportArgs, ReplayArgs, SimulateArgs, StrategyReplayArgs, WebsocketDemoArgs,
};

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
    /// Simulate with strategy, optional risk config, and portfolio summary.
    Simulate(SimulateArgs),
    /// Print (or refresh) the measured benchmark report.
    BenchmarkReport(BenchmarkReportArgs),
    /// Local paper WebSocket / mock market-data demo (no live trading).
    WebsocketDemo(WebsocketDemoArgs),
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Command::Replay(args) => run_replay(args),
        Command::StrategyReplay(args) => run_strategy_replay(args),
        Command::Simulate(args) => run_simulate(args),
        Command::BenchmarkReport(args) => run_benchmark_report(args),
        Command::WebsocketDemo(args) => run_websocket_demo(args),
    }
}
