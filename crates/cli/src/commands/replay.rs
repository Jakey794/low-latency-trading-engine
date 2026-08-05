use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::Args;
use engine::{
    matching::MatchingEngine,
    replay::{parse_jsonl, ReplayDriver, ReplayEventKind, ReplayOutputEvent, ReplaySummary},
    runtime::{Runtime, RuntimeConfig, RuntimeEvent, RuntimeOutput},
    types::Symbol,
};

#[derive(Debug, Args)]
pub struct ReplayArgs {
    /// JSONL replay input file.
    pub path: PathBuf,

    /// Write replay output events as JSONL instead of printing them to stdout.
    #[arg(long, value_name = "PATH", conflicts_with = "summary_only")]
    pub output: Option<PathBuf>,

    /// Print only the replay summary.
    #[arg(long, conflicts_with_all = ["output", "book"])]
    pub summary_only: bool,

    /// Print the final full-depth order book snapshot as JSON.
    #[arg(long)]
    pub book: bool,

    /// Force multi-symbol runtime path even for a single symbol.
    #[arg(long)]
    pub multi: bool,
}

pub fn run(args: ReplayArgs) -> Result<()> {
    let input = File::open(&args.path)
        .with_context(|| format!("failed to open replay input {}", args.path.display()))?;
    let events = parse_jsonl(BufReader::new(input))
        .with_context(|| format!("failed to parse replay input {}", args.path.display()))?;

    let mut symbols = BTreeSet::new();
    for event in &events {
        match &event.kind {
            ReplayEventKind::NewOrder { order } => {
                symbols.insert(order.symbol.0.clone());
            }
            ReplayEventKind::Cancel { symbol, .. } => {
                symbols.insert(symbol.0.clone());
            }
        }
    }
    if symbols.is_empty() {
        anyhow::bail!("replay input {} contains no events", args.path.display());
    }

    if args.multi || symbols.len() > 1 {
        return run_multi(args, events, symbols);
    }

    let symbol = Symbol(symbols.into_iter().next().unwrap());
    let mut driver = ReplayDriver::new(MatchingEngine::new(symbol));
    let result = driver
        .replay_events(events)
        .with_context(|| format!("failed to replay {}", args.path.display()))?;

    if args.summary_only {
        let stdout = io::stdout();
        let mut writer = stdout.lock();
        write_summary(&mut writer, &result.summary)?;
        return Ok(());
    }

    match &args.output {
        Some(path) => write_outputs_file(path, &result.outputs)?,
        None => {
            let stdout = io::stdout();
            let mut writer = stdout.lock();
            write_replay_outputs(&mut writer, &result.outputs)?;
        }
    }

    let stderr = io::stderr();
    let mut diagnostics = stderr.lock();
    write_summary(&mut diagnostics, &result.summary)?;
    if args.book {
        writeln!(diagnostics, "book:")?;
        serde_json::to_writer_pretty(&mut diagnostics, &result.final_book)
            .context("failed to serialize final book snapshot")?;
        writeln!(diagnostics)?;
    }

    Ok(())
}

fn run_multi(
    args: ReplayArgs,
    events: Vec<engine::replay::ReplayEvent>,
    symbols: BTreeSet<String>,
) -> Result<()> {
    let symbol_list: Vec<Symbol> = symbols.into_iter().map(Symbol).collect();
    let mut runtime = Runtime::new(symbol_list, RuntimeConfig::default());
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
        .with_context(|| format!("failed to multi-symbol replay {}", args.path.display()))?;

    if args.summary_only {
        writeln!(io::stdout(), "output_events: {}", result.outputs.len())?;
        writeln!(io::stdout(), "cash: {}", result.portfolio.cash)?;
        writeln!(io::stdout(), "equity: {}", result.portfolio.equity)?;
        writeln!(io::stdout(), "books: {}", result.books.len())?;
        return Ok(());
    }

    match &args.output {
        Some(path) => write_runtime_outputs_file(path, &result.outputs)?,
        None => write_runtime_outputs(&mut io::stdout().lock(), &result.outputs)?,
    }

    let mut diagnostics = io::stderr().lock();
    writeln!(diagnostics, "output_events: {}", result.outputs.len())?;
    writeln!(diagnostics, "cash: {}", result.portfolio.cash)?;
    writeln!(diagnostics, "equity: {}", result.portfolio.equity)?;
    if args.book {
        writeln!(diagnostics, "books:")?;
        serde_json::to_writer_pretty(&mut diagnostics, &result.books)?;
        writeln!(diagnostics)?;
    }
    Ok(())
}

fn write_outputs_file(path: &Path, outputs: &[ReplayOutputEvent]) -> Result<()> {
    let file = File::create(path)
        .with_context(|| format!("failed to create replay output {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    write_replay_outputs(&mut writer, outputs)
        .with_context(|| format!("failed to write replay output {}", path.display()))?;
    writer
        .flush()
        .with_context(|| format!("failed to flush replay output {}", path.display()))
}

fn write_runtime_outputs_file(path: &Path, outputs: &[RuntimeOutput]) -> Result<()> {
    let file = File::create(path)
        .with_context(|| format!("failed to create replay output {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    write_runtime_outputs(&mut writer, outputs)?;
    writer.flush()?;
    Ok(())
}

fn write_replay_outputs<W: Write>(writer: &mut W, outputs: &[ReplayOutputEvent]) -> Result<()> {
    for event in outputs {
        serde_json::to_writer(&mut *writer, event).context("failed to serialize replay output")?;
        writeln!(writer).context("failed to write replay output")?;
    }
    Ok(())
}

fn write_runtime_outputs<W: Write>(writer: &mut W, outputs: &[RuntimeOutput]) -> Result<()> {
    for event in outputs {
        serde_json::to_writer(&mut *writer, event)?;
        writeln!(writer)?;
    }
    Ok(())
}

fn write_summary<W: Write>(writer: &mut W, summary: &ReplaySummary) -> Result<()> {
    writeln!(writer, "input_events: {}", summary.input_events)?;
    writeln!(writer, "output_events: {}", summary.output_events)?;
    writeln!(writer, "accepted: {}", summary.accepted)?;
    writeln!(writer, "rejected: {}", summary.rejected)?;
    writeln!(writer, "trades: {}", summary.trades)?;
    writeln!(writer, "cancelled: {}", summary.cancelled)?;
    writeln!(writer, "expired: {}", summary.expired)?;
    writeln!(
        writer,
        "final_resting_orders: {}",
        summary.final_resting_orders
    )?;
    writeln!(writer, "final_bid_levels: {}", summary.final_bid_levels)?;
    writeln!(writer, "final_ask_levels: {}", summary.final_ask_levels)?;
    writer.flush().context("failed to flush replay summary")
}
