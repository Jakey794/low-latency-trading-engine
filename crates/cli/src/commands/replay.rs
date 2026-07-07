use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::Args;
use engine::{
    matching::MatchingEngine,
    replay::{parse_jsonl, ReplayDriver, ReplayEventKind, ReplayOutputEvent, ReplaySummary},
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
}

pub fn run(args: ReplayArgs) -> Result<()> {
    let input = File::open(&args.path)
        .with_context(|| format!("failed to open replay input {}", args.path.display()))?;
    let events = parse_jsonl(BufReader::new(input))
        .with_context(|| format!("failed to parse replay input {}", args.path.display()))?;
    let symbol = events
        .first()
        .map(|event| match &event.kind {
            ReplayEventKind::NewOrder { order } => order.symbol.clone(),
            ReplayEventKind::Cancel { symbol, .. } => symbol.clone(),
        })
        .with_context(|| format!("replay input {} contains no events", args.path.display()))?;

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
            write_outputs(&mut writer, &result.outputs)?;
        }
    }

    let stderr = io::stderr();
    let mut diagnostics = stderr.lock();
    write_summary(&mut diagnostics, &result.summary)?;
    if args.book {
        writeln!(diagnostics, "book:")?;
        serde_json::to_writer_pretty(
            &mut diagnostics,
            &driver.engine().book().snapshot(usize::MAX),
        )
        .context("failed to serialize final book snapshot")?;
        writeln!(diagnostics)?;
    }

    Ok(())
}

fn write_outputs_file(path: &Path, outputs: &[ReplayOutputEvent]) -> Result<()> {
    let file = File::create(path)
        .with_context(|| format!("failed to create replay output {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    write_outputs(&mut writer, outputs)
        .with_context(|| format!("failed to write replay output {}", path.display()))?;
    writer
        .flush()
        .with_context(|| format!("failed to flush replay output {}", path.display()))
}

fn write_outputs<W: Write>(writer: &mut W, outputs: &[ReplayOutputEvent]) -> Result<()> {
    for event in outputs {
        serde_json::to_writer(&mut *writer, event).context("failed to serialize replay output")?;
        writeln!(writer).context("failed to write replay output")?;
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
    writer.flush().context("failed to flush replay summary")
}
