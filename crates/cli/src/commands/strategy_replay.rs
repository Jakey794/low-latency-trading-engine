use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use clap::Args;
use engine::{
    replay::{parse_jsonl, ReplayEventKind},
    risk::RiskLimits,
    runtime::{Runtime, RuntimeConfig, RuntimeEvent, RuntimeOutput, RuntimeResult},
    strategy::create_builtin,
    types::Symbol,
};

#[derive(Debug, Args)]
pub struct StrategyReplayArgs {
    /// JSONL replay input file.
    pub path: PathBuf,

    /// Built-in strategy: `market_making` or `momentum`.
    #[arg(long)]
    pub strategy: String,

    /// Strategy id used for generated orders.
    #[arg(long, default_value_t = 1)]
    pub strategy_id: u64,

    /// Starting cash for the portfolio.
    #[arg(long, default_value_t = 1_000_000)]
    pub starting_cash: i128,

    /// Optional max order quantity risk limit.
    #[arg(long)]
    pub max_order_qty: Option<u64>,

    /// Write runtime output events as JSONL instead of stdout.
    #[arg(long, value_name = "PATH", conflicts_with = "summary_only")]
    pub output: Option<PathBuf>,

    /// Print only a brief summary.
    #[arg(long, conflicts_with_all = ["output", "portfolio"])]
    pub summary_only: bool,

    /// Print final portfolio snapshot as JSON to stderr.
    #[arg(long)]
    pub portfolio: bool,
}

pub fn run(args: StrategyReplayArgs) -> Result<()> {
    let strategy = create_builtin(&args.strategy, args.strategy_id)
        .with_context(|| format!("unknown strategy '{}'", args.strategy))?;

    let input = File::open(&args.path)
        .with_context(|| format!("failed to open {}", args.path.display()))?;
    let events = parse_jsonl(BufReader::new(input))
        .with_context(|| format!("failed to parse {}", args.path.display()))?;

    let mut symbols: Vec<Symbol> = Vec::new();
    for event in &events {
        let symbol = match &event.kind {
            ReplayEventKind::NewOrder { order } => order.symbol.clone(),
            ReplayEventKind::Cancel { symbol, .. } => symbol.clone(),
        };
        if !symbols.iter().any(|s| s == &symbol) {
            symbols.push(symbol);
        }
    }
    if symbols.is_empty() {
        bail!("replay input {} contains no events", args.path.display());
    }
    symbols.sort_by(|a, b| a.0.cmp(&b.0));

    let config = RuntimeConfig {
        starting_cash: args.starting_cash,
        risk_limits: RiskLimits {
            max_order_qty: args.max_order_qty,
            ..RiskLimits::default()
        },
        ..RuntimeConfig::default()
    };

    let mut runtime = Runtime::new(symbols, config);
    runtime.add_strategy(strategy);

    let runtime_events: Vec<RuntimeEvent> = events
        .into_iter()
        .map(|e| match e.kind {
            ReplayEventKind::NewOrder { order } => RuntimeEvent::NewOrder {
                seq: e.seq,
                ts_ns: e.ts_ns,
                order,
            },
            ReplayEventKind::Cancel { order_id, symbol } => RuntimeEvent::Cancel {
                seq: e.seq,
                ts_ns: e.ts_ns,
                order_id,
                symbol,
            },
        })
        .collect();

    let result = runtime
        .process_events(runtime_events)
        .with_context(|| format!("failed to run strategy replay {}", args.path.display()))?;

    if args.summary_only {
        write_summary(&mut io::stdout().lock(), &result)?;
        return Ok(());
    }

    match &args.output {
        Some(path) => write_outputs_file(path, &result.outputs)?,
        None => write_outputs(&mut io::stdout().lock(), &result.outputs)?,
    }

    let mut diagnostics = io::stderr().lock();
    write_summary(&mut diagnostics, &result)?;
    if args.portfolio {
        writeln!(diagnostics, "portfolio:")?;
        serde_json::to_writer_pretty(&mut diagnostics, &result.portfolio)?;
        writeln!(diagnostics)?;
    }

    Ok(())
}

fn write_outputs_file(path: &Path, outputs: &[RuntimeOutput]) -> Result<()> {
    let file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    write_outputs(&mut writer, outputs)?;
    writer.flush()?;
    Ok(())
}

fn write_outputs<W: Write>(writer: &mut W, outputs: &[RuntimeOutput]) -> Result<()> {
    for event in outputs {
        serde_json::to_writer(&mut *writer, event)?;
        writeln!(writer)?;
    }
    Ok(())
}

fn write_summary<W: Write>(writer: &mut W, result: &RuntimeResult) -> Result<()> {
    let mut accepted = 0u64;
    let mut rejected = 0u64;
    let mut risk_rejected = 0u64;
    let mut trades = 0u64;
    let mut cancelled = 0u64;
    for o in &result.outputs {
        use engine::runtime::RuntimeOutputKind::*;
        match &o.kind {
            Accepted { .. } => accepted += 1,
            Rejected { .. } => rejected += 1,
            RiskRejected { .. } => risk_rejected += 1,
            Trade { .. } => trades += 1,
            Cancelled { .. } => cancelled += 1,
            _ => {}
        }
    }
    writeln!(writer, "output_events: {}", result.outputs.len())?;
    writeln!(writer, "accepted: {accepted}")?;
    writeln!(writer, "rejected: {rejected}")?;
    writeln!(writer, "risk_rejected: {risk_rejected}")?;
    writeln!(writer, "trades: {trades}")?;
    writeln!(writer, "cancelled: {cancelled}")?;
    writeln!(writer, "cash: {}", result.portfolio.cash)?;
    writeln!(writer, "realized_pnl: {}", result.portfolio.realized_pnl)?;
    writeln!(writer, "equity: {}", result.portfolio.equity)?;
    writer.flush()?;
    Ok(())
}
