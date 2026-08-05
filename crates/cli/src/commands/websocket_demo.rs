//! Local paper WebSocket demo — loopback only, no credentials, no live trading.

use std::{
    fs::File,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use clap::Args;
use engine::{
    paper::{
        paper_message_to_runtime_event, MockPaperSession, PaperConvertError, PaperMdMessage,
        PaperReport,
    },
    runtime::{Runtime, RuntimeConfig, RuntimeOutputKind},
    types::Symbol,
};
use tungstenite::{accept, connect, Message};

#[derive(Debug, Args)]
pub struct WebsocketDemoArgs {
    /// Scripted paper MD messages (JSONL).
    #[arg(long, default_value = "data/scenarios/paper_ws_demo.jsonl")]
    pub script: PathBuf,

    /// Bind a localhost WebSocket server and connect a client (loopback only).
    /// Default is an in-process mock adapter with no sockets.
    #[arg(long)]
    pub listen: bool,

    /// Optional bind address (loopback only). Port 0 = ephemeral.
    #[arg(long, default_value = "127.0.0.1:0")]
    pub bind: String,

    /// Print portfolio snapshot to stderr.
    #[arg(long)]
    pub portfolio: bool,
}

pub fn run(args: WebsocketDemoArgs) -> Result<()> {
    let messages = load_script(&args.script)?;
    if args.listen {
        run_loopback(messages, &args.bind, args.portfolio)
    } else {
        run_offline(messages, args.portfolio)
    }
}

fn load_script(path: &PathBuf) -> Result<Vec<PaperMdMessage>> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut out = Vec::new();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| format!("read line {}", i + 1))?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let msg: PaperMdMessage = serde_json::from_str(line)
            .with_context(|| format!("parse paper message line {}", i + 1))?;
        out.push(msg);
    }
    if out.is_empty() {
        bail!("script {} contains no messages", path.display());
    }
    Ok(out)
}

fn run_offline(messages: Vec<PaperMdMessage>, portfolio: bool) -> Result<()> {
    eprintln!("paper websocket demo: offline mock adapter (no network)");
    let mut session = MockPaperSession::from_messages(messages);
    let events = session
        .drain_runtime_events()
        .context("drain mock session")?;
    for report in session.source.reports() {
        if let PaperReport::Disconnected { reason }
        | PaperReport::AdapterError { message: reason } = report
        {
            eprintln!("adapter: {reason}");
        }
    }
    finish_runtime(events, portfolio)
}

fn run_loopback(messages: Vec<PaperMdMessage>, bind: &str, portfolio: bool) -> Result<()> {
    if !(bind.starts_with("127.0.0.1")
        || bind.starts_with("localhost")
        || bind.starts_with("[::1]"))
    {
        bail!("websocket demo refuses non-loopback bind address: {bind}");
    }

    let listener = TcpListener::bind(bind).with_context(|| format!("bind {bind}"))?;
    let addr = listener.local_addr().context("local_addr")?;
    eprintln!("paper websocket demo: listening on ws://{addr} (loopback only)");

    let (ready_tx, ready_rx) = mpsc::channel::<()>();
    let server = thread::spawn(move || -> Result<()> {
        let (stream, peer) = listener.accept().context("accept")?;
        eprintln!("accepted connection from {peer}");
        let _ = ready_tx.send(());
        let mut ws = accept(stream).context("websocket accept")?;
        for msg in &messages {
            let json = serde_json::to_string(msg).context("serialize paper msg")?;
            ws.send(Message::Text(json.into()))
                .context("send paper msg")?;
        }
        ws.close(None).ok();
        // Allow client to drain.
        thread::sleep(Duration::from_millis(20));
        Ok(())
    });

    // Tiny yield so accept is ready; connect is localhost-only.
    thread::sleep(Duration::from_millis(10));
    let url = format!("ws://{addr}");
    let (mut socket, _resp) = connect(&url).with_context(|| format!("connect {url}"))?;
    let _ = ready_rx.recv_timeout(Duration::from_secs(2));

    let mut collected = Vec::new();
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => {
                let msg: PaperMdMessage =
                    serde_json::from_str(&text).context("client parse paper msg")?;
                match paper_message_to_runtime_event(&msg) {
                    Ok(batch) => collected.extend(batch),
                    Err(PaperConvertError::NotAnEvent) => {}
                    Err(e) => bail!("paper convert error: {e}"),
                }
            }
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => {}
        }
    }

    server.join().expect("server thread").context("server")?;
    eprintln!(
        "received {} runtime events via localhost WebSocket",
        collected.len()
    );
    finish_runtime(collected, portfolio)
}

fn finish_runtime(events: Vec<engine::runtime::RuntimeEvent>, portfolio: bool) -> Result<()> {
    let mut symbols = Vec::new();
    for ev in &events {
        if let engine::runtime::RuntimeEvent::NewOrder { order, .. } = ev {
            if !symbols.iter().any(|s: &Symbol| s == &order.symbol) {
                symbols.push(order.symbol.clone());
            }
        }
    }
    if symbols.is_empty() {
        symbols.push(Symbol("AAPL".into()));
    }
    symbols.sort_by(|a, b| a.0.cmp(&b.0));

    let mut runtime = Runtime::new(symbols, RuntimeConfig::default());
    let result = runtime
        .process_events(events)
        .context("runtime process paper events")?;

    let mut trades = 0u64;
    let mut accepted = 0u64;
    for o in &result.outputs {
        match &o.kind {
            RuntimeOutputKind::Trade { .. } => trades += 1,
            RuntimeOutputKind::Accepted { .. } => accepted += 1,
            _ => {}
        }
        serde_json::to_writer(std::io::stdout().lock(), o)?;
        println!();
    }

    let mut err = std::io::stderr().lock();
    writeln!(err, "output_events: {}", result.outputs.len())?;
    writeln!(err, "accepted: {accepted}")?;
    writeln!(err, "trades: {trades}")?;
    writeln!(err, "cash: {}", result.portfolio.cash)?;
    writeln!(err, "equity: {}", result.portfolio.equity)?;
    if portfolio {
        writeln!(err, "portfolio:")?;
        serde_json::to_writer_pretty(&mut err, &result.portfolio)?;
        writeln!(err)?;
    }
    writeln!(
        err,
        "note: paper demo only — no credentials, no live order submission"
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_loads_demo_file() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/scenarios/paper_ws_demo.jsonl");
        let msgs = load_script(&path).expect("load demo script");
        assert!(msgs.len() >= 3);
    }
}
