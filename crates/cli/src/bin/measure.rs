//! Release measurement harness — produces genuine latency/throughput JSON.
//!
//! Wall-clock timing is intentionally confined to this binary (reporting only).

use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use clap::Parser;
use engine::{
    events::{CancelOrderEvent, ExecutionReport, InputEvent, NewOrderEvent},
    matching::MatchingEngine,
    metrics::{throughput_report, timed, LatencyCollector},
    replay::{parse_jsonl, ReplayDriver},
    runtime::{Runtime, RuntimeConfig, RuntimeEvent},
    strategy::{MarketMakingConfig, MarketMakingStrategy},
    types::{Order, OrderType, PriceTicks, Qty, Side, Symbol},
};
use serde::Serialize;
use serde_json::json;

#[derive(Parser, Debug)]
#[command(
    name = "measure",
    about = "Collect genuine engine latency/throughput measurements"
)]
struct Args {
    /// Output JSON path (default: docs/benchmarks/latest.json)
    #[arg(long, default_value = "docs/benchmarks/latest.json")]
    output: PathBuf,

    /// Also write out/bench_summary.json for chart scripts
    #[arg(long, default_value = "out/bench_summary.json")]
    summary: PathBuf,

    /// Iterations for micro hot-path timings
    #[arg(long, default_value_t = 50_000)]
    micro_iters: u64,
}

#[derive(Serialize)]
struct WorkloadResult {
    name: String,
    description: String,
    events: u64,
    sample_count: u64,
    latency: Option<serde_json::Value>,
    throughput: serde_json::Value,
}

fn symbol() -> Symbol {
    Symbol("AAPL".into())
}

fn limit(id: u64, side: Side, px: i64, qty: u64) -> Order {
    Order {
        order_id: id,
        symbol: symbol(),
        side,
        order_type: OrderType::Limit,
        price: Some(PriceTicks(px)),
        qty: Qty(qty),
        timestamp_ns: id,
        strategy_id: None,
    }
}

fn synthetic_workload(n: u64) -> Vec<InputEvent> {
    let mut events = Vec::with_capacity(n as usize);
    for i in 0..n {
        if i % 10 == 9 {
            let target = i.saturating_sub(5);
            events.push(InputEvent::Cancel(CancelOrderEvent {
                seq: i + 1,
                order_id: target + 1,
                symbol: symbol(),
                timestamp_ns: i + 1,
            }));
        } else {
            let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
            let px = 100 + (i % 20) as i64 - 10;
            events.push(InputEvent::NewOrder(NewOrderEvent {
                seq: i + 1,
                order: limit(i + 1, side, px, 1 + (i % 3)),
            }));
        }
    }
    events
}

fn measure_micro(
    name: &str,
    description: &str,
    iters: u64,
    mut op: impl FnMut(),
) -> WorkloadResult {
    // Warmup
    for _ in 0..iters.min(1_000) {
        op();
    }
    let mut latency = LatencyCollector::new();
    let start = Instant::now();
    for _ in 0..iters {
        let t0 = Instant::now();
        op();
        latency.record_duration(t0.elapsed());
    }
    let elapsed = start.elapsed();
    let report = latency.report();
    WorkloadResult {
        name: name.to_string(),
        description: description.to_string(),
        events: iters,
        sample_count: report.samples,
        latency: Some(json!({
            "p50_ns": report.p50_ns,
            "p90_ns": report.p90_ns,
            "p95_ns": report.p95_ns,
            "p99_ns": report.p99_ns,
            "p999_ns": report.p999_ns,
            "max_ns": report.max_ns,
            "mean_ns": report.mean_ns,
            "samples": report.samples,
        })),
        throughput: json!(throughput_report(iters, 0, 0, elapsed)),
    }
}

fn measure_workload(name: &str, description: &str, n: u64) -> WorkloadResult {
    let events = synthetic_workload(n);
    let mut engine = MatchingEngine::new(symbol());
    let mut latency = LatencyCollector::new();
    let mut trades = 0u64;
    let mut cancels = 0u64;

    let (_, elapsed) = timed(|| {
        for event in events {
            let is_cancel = matches!(event, InputEvent::Cancel(_));
            let (result, d) = timed(|| engine.process_event(event));
            latency.record_duration(d);
            if is_cancel {
                cancels += 1;
            }
            for r in &result {
                if matches!(
                    r,
                    ExecutionReport::Filled { .. } | ExecutionReport::PartiallyFilled { .. }
                ) {
                    trades += 1;
                }
            }
        }
    });

    let report = latency.report();
    WorkloadResult {
        name: name.to_string(),
        description: description.to_string(),
        events: n,
        sample_count: report.samples,
        latency: Some(json!({
            "p50_ns": report.p50_ns,
            "p90_ns": report.p90_ns,
            "p95_ns": report.p95_ns,
            "p99_ns": report.p99_ns,
            "p999_ns": report.p999_ns,
            "max_ns": report.max_ns,
            "mean_ns": report.mean_ns,
            "samples": report.samples,
        })),
        throughput: json!(throughput_report(n, trades, cancels, elapsed)),
    }
}

fn measure_jsonl_parse_and_replay() -> (WorkloadResult, WorkloadResult) {
    let path = "data/scenarios/basic_cross.jsonl";
    let data = fs::read_to_string(path).expect("scenario");
    let iters = 5_000u64;

    // Parse-only
    let mut parse_lat = LatencyCollector::new();
    let parse_start = Instant::now();
    for _ in 0..iters {
        let t0 = Instant::now();
        let events = parse_jsonl(std::io::Cursor::new(data.as_bytes())).unwrap();
        parse_lat.record_duration(t0.elapsed());
        std::hint::black_box(events);
    }
    let parse_elapsed = parse_start.elapsed();
    let parse_report = parse_lat.report();

    // Replay-only (parse once outside)
    let events = parse_jsonl(std::io::Cursor::new(data.as_bytes())).unwrap();
    let mut replay_lat = LatencyCollector::new();
    let replay_start = Instant::now();
    for _ in 0..iters {
        let t0 = Instant::now();
        let mut driver = ReplayDriver::new(MatchingEngine::new(symbol()));
        let result = driver.replay_events(events.clone()).unwrap();
        replay_lat.record_duration(t0.elapsed());
        std::hint::black_box(result);
    }
    let replay_elapsed = replay_start.elapsed();
    let replay_report = replay_lat.report();

    (
        WorkloadResult {
            name: "jsonl_parse_only".into(),
            description: "Parse basic_cross.jsonl only (no matching)".into(),
            events: iters,
            sample_count: parse_report.samples,
            latency: Some(json!({
                "p50_ns": parse_report.p50_ns,
                "p90_ns": parse_report.p90_ns,
                "p95_ns": parse_report.p95_ns,
                "p99_ns": parse_report.p99_ns,
                "p999_ns": parse_report.p999_ns,
                "max_ns": parse_report.max_ns,
                "mean_ns": parse_report.mean_ns,
                "samples": parse_report.samples,
            })),
            throughput: json!(throughput_report(iters, 0, 0, parse_elapsed)),
        },
        WorkloadResult {
            name: "jsonl_replay_only".into(),
            description: "Replay pre-parsed basic_cross events through MatchingEngine".into(),
            events: iters,
            sample_count: replay_report.samples,
            latency: Some(json!({
                "p50_ns": replay_report.p50_ns,
                "p90_ns": replay_report.p90_ns,
                "p95_ns": replay_report.p95_ns,
                "p99_ns": replay_report.p99_ns,
                "p999_ns": replay_report.p999_ns,
                "max_ns": replay_report.max_ns,
                "mean_ns": replay_report.mean_ns,
                "samples": replay_report.samples,
            })),
            throughput: json!(throughput_report(iters, 0, 0, replay_elapsed)),
        },
    )
}

fn measure_strategy_runtime() -> WorkloadResult {
    let iters = 2_000u64;
    let mut latency = LatencyCollector::new();
    let start = Instant::now();
    for i in 0..iters {
        let t0 = Instant::now();
        let mut rt = Runtime::new(vec![symbol()], RuntimeConfig::default());
        rt.add_strategy(Box::new(MarketMakingStrategy::new(
            1,
            MarketMakingConfig::default(),
        )));
        let events = vec![
            RuntimeEvent::NewOrder {
                seq: 1,
                ts_ns: 100,
                order: limit(1 + i * 10, Side::Buy, 100, 10),
            },
            RuntimeEvent::NewOrder {
                seq: 2,
                ts_ns: 200,
                order: limit(2 + i * 10, Side::Sell, 110, 10),
            },
        ];
        let _ = std::hint::black_box(rt.process_events(events).unwrap());
        latency.record_duration(t0.elapsed());
    }
    let elapsed = start.elapsed();
    let report = latency.report();
    WorkloadResult {
        name: "strategy_runtime_seed".into(),
        description: "Market-making strategy on two-sided seed book".into(),
        events: iters,
        sample_count: report.samples,
        latency: Some(json!({
            "p50_ns": report.p50_ns,
            "p90_ns": report.p90_ns,
            "p95_ns": report.p95_ns,
            "p99_ns": report.p99_ns,
            "p999_ns": report.p999_ns,
            "max_ns": report.max_ns,
            "mean_ns": report.mean_ns,
            "samples": report.samples,
        })),
        throughput: json!(throughput_report(iters, 0, 0, elapsed)),
    }
}

fn measure_multi_symbol() -> WorkloadResult {
    let iters = 1_000u64;
    let mut latency = LatencyCollector::new();
    let start = Instant::now();
    for _ in 0..iters {
        let t0 = Instant::now();
        let mut rt = Runtime::new(
            vec![Symbol("AAPL".into()), Symbol("MSFT".into())],
            RuntimeConfig::default(),
        );
        let mut events = Vec::new();
        for i in 0..200u64 {
            let sym = if i % 2 == 0 { "AAPL" } else { "MSFT" };
            events.push(RuntimeEvent::NewOrder {
                seq: i + 1,
                ts_ns: i + 1,
                order: Order {
                    order_id: i + 1,
                    symbol: Symbol(sym.into()),
                    side: if i % 4 < 2 { Side::Buy } else { Side::Sell },
                    order_type: OrderType::Limit,
                    price: Some(PriceTicks(100 + (i % 10) as i64)),
                    qty: Qty(1),
                    timestamp_ns: i + 1,
                    strategy_id: None,
                },
            });
        }
        let _ = std::hint::black_box(rt.process_events(events).unwrap());
        latency.record_duration(t0.elapsed());
    }
    let elapsed = start.elapsed();
    let report = latency.report();
    WorkloadResult {
        name: "multi_symbol_interleaved".into(),
        description: "200 interleaved AAPL/MSFT orders via Runtime".into(),
        events: iters,
        sample_count: report.samples,
        latency: Some(json!({
            "p50_ns": report.p50_ns,
            "p90_ns": report.p90_ns,
            "p95_ns": report.p95_ns,
            "p99_ns": report.p99_ns,
            "p999_ns": report.p999_ns,
            "max_ns": report.max_ns,
            "mean_ns": report.mean_ns,
            "samples": report.samples,
        })),
        throughput: json!(throughput_report(iters * 200, 0, 0, elapsed)),
    }
}

fn shell_capture(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn environment_metadata() -> serde_json::Value {
    let rustc = shell_capture("rustc", &["--version"]);
    let cpu = shell_capture("sysctl", &["-n", "machdep.cpu.brand_string"])
        .or_else(|| shell_capture("uname", &["-m"]));
    let mem_bytes =
        shell_capture("sysctl", &["-n", "hw.memsize"]).and_then(|s| s.parse::<u64>().ok());
    let os = shell_capture("uname", &["-srm"]);
    let macos = shell_capture("sw_vers", &["-productVersion"]);
    let date_utc = shell_capture("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]);

    json!({
        "date_utc": date_utc,
        "machine_model": shell_capture("sysctl", &["-n", "hw.model"]),
        "cpu": cpu,
        "ram_bytes": mem_bytes,
        "ram_gib": mem_bytes.map(|b| (b as f64) / (1024.0 * 1024.0 * 1024.0)),
        "operating_system": os,
        "macos_version": macos,
        "rust_version": rustc,
        "build_profile": "release",
        "benchmark_configuration": {
            "harness": "engine-cli measure (hdrhistogram)",
            "criterion_full": "BENCH_FULL=1 cargo bench (optional companion)",
            "notes": "Wall-clock used only in this measurement binary; deterministic engine paths remain clock-free."
        }
    })
}

fn run_python_baseline(events: u64) -> Option<serde_json::Value> {
    let py = if PathBuf::from(".venv/bin/python").exists() {
        ".venv/bin/python"
    } else {
        "python3"
    };
    let out = Command::new(py)
        .args(["python/baseline_lob.py", "--events", &events.to_string()])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

fn main() -> Result<()> {
    let args = Args::parse();
    let iters = args.micro_iters;

    eprintln!("Collecting measurements (release harness)...");

    let mut workloads = Vec::new();

    workloads.push(measure_micro(
        "add_non_crossing_limit",
        "Insert one non-crossing buy limit order on a fresh book",
        iters,
        || {
            let mut engine = MatchingEngine::new(symbol());
            let _ =
                std::hint::black_box(engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                    seq: 1,
                    order: limit(1, Side::Buy, 100, 1),
                })));
        },
    ));

    workloads.push(measure_micro(
        "cancel_resting_order",
        "Cancel one resting order (setup outside timed section per iter via rebuild)",
        iters,
        || {
            let mut engine = MatchingEngine::new(symbol());
            engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                seq: 1,
                order: limit(1, Side::Buy, 100, 1),
            }));
            let t0 = Instant::now();
            let reports = engine.process_event(InputEvent::Cancel(CancelOrderEvent {
                seq: 2,
                order_id: 1,
                symbol: symbol(),
                timestamp_ns: 2,
            }));
            // Include only cancel in black_box; setup cost is in same op for simplicity.
            // Prefer dedicated timed cancel after setup:
            let _ = (std::hint::black_box(reports), t0);
        },
    ));

    // Better cancel measurement: setup outside
    {
        let cancel_iters = iters;
        let mut latency = LatencyCollector::new();
        let start = Instant::now();
        for i in 0..cancel_iters {
            let mut engine = MatchingEngine::new(symbol());
            engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                seq: 1,
                order: limit(1, Side::Buy, 100, 1),
            }));
            let t0 = Instant::now();
            let reports = engine.process_event(InputEvent::Cancel(CancelOrderEvent {
                seq: 2,
                order_id: 1,
                symbol: symbol(),
                timestamp_ns: 2 + i,
            }));
            latency.record_duration(t0.elapsed());
            std::hint::black_box(reports);
        }
        let elapsed = start.elapsed();
        let report = latency.report();
        // Replace the weaker cancel entry
        workloads.pop();
        workloads.push(WorkloadResult {
            name: "cancel_resting_order".into(),
            description: "Cancel one resting order; setup excluded from latency sample".into(),
            events: cancel_iters,
            sample_count: report.samples,
            latency: Some(json!({
                "p50_ns": report.p50_ns,
                "p90_ns": report.p90_ns,
                "p95_ns": report.p95_ns,
                "p99_ns": report.p99_ns,
                "p999_ns": report.p999_ns,
                "max_ns": report.max_ns,
                "mean_ns": report.mean_ns,
                "samples": report.samples,
            })),
            throughput: json!(throughput_report(cancel_iters, 0, cancel_iters, elapsed)),
        });
    }

    // full match / partial / sweep with setup excluded
    for (name, desc, setup_side, setup_px, setup_qty, agres_side, agres_px, agres_qty) in [
        (
            "full_match",
            "Aggressive buy fully fills resting sell",
            Side::Sell,
            100i64,
            5u64,
            Side::Buy,
            100i64,
            5u64,
        ),
        (
            "partial_fill",
            "Aggressive buy partially fills larger resting sell",
            Side::Sell,
            100,
            10,
            Side::Buy,
            100,
            3,
        ),
    ] {
        let mut latency = LatencyCollector::new();
        let start = Instant::now();
        for i in 0..iters {
            let mut engine = MatchingEngine::new(symbol());
            engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                seq: 1,
                order: limit(1, setup_side, setup_px, setup_qty),
            }));
            let t0 = Instant::now();
            let reports = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                seq: 2,
                order: limit(2 + i, agres_side, agres_px, agres_qty),
            }));
            latency.record_duration(t0.elapsed());
            std::hint::black_box(reports);
        }
        let elapsed = start.elapsed();
        let report = latency.report();
        workloads.push(WorkloadResult {
            name: name.into(),
            description: desc.into(),
            events: iters,
            sample_count: report.samples,
            latency: Some(json!({
                "p50_ns": report.p50_ns,
                "p90_ns": report.p90_ns,
                "p95_ns": report.p95_ns,
                "p99_ns": report.p99_ns,
                "p999_ns": report.p999_ns,
                "max_ns": report.max_ns,
                "mean_ns": report.mean_ns,
                "samples": report.samples,
            })),
            throughput: json!(throughput_report(iters, iters, 0, elapsed)),
        });
    }

    // multi-level sweep
    {
        let mut latency = LatencyCollector::new();
        let start = Instant::now();
        for i in 0..iters {
            let mut engine = MatchingEngine::new(symbol());
            for j in 0..5u64 {
                engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                    seq: j + 1,
                    order: limit(j + 1, Side::Sell, 100 + j as i64, 2),
                }));
            }
            let t0 = Instant::now();
            let reports = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                seq: 100,
                order: limit(100 + i, Side::Buy, 110, 20),
            }));
            latency.record_duration(t0.elapsed());
            std::hint::black_box(reports);
        }
        let elapsed = start.elapsed();
        let report = latency.report();
        workloads.push(WorkloadResult {
            name: "multi_level_sweep".into(),
            description: "Buy sweeps five ask levels".into(),
            events: iters,
            sample_count: report.samples,
            latency: Some(json!({
                "p50_ns": report.p50_ns,
                "p90_ns": report.p90_ns,
                "p95_ns": report.p95_ns,
                "p99_ns": report.p99_ns,
                "p999_ns": report.p999_ns,
                "max_ns": report.max_ns,
                "mean_ns": report.mean_ns,
                "samples": report.samples,
            })),
            throughput: json!(throughput_report(iters, iters, 0, elapsed)),
        });
    }

    workloads.push(measure_workload(
        "core_engine_10k",
        "10,000-event synthetic deterministic engine workload",
        10_000,
    ));
    workloads.push(measure_workload(
        "core_engine_100k",
        "100,000-event synthetic deterministic engine workload",
        100_000,
    ));

    let (parse_w, replay_w) = measure_jsonl_parse_and_replay();
    workloads.push(parse_w);
    workloads.push(replay_w);
    workloads.push(measure_strategy_runtime());
    workloads.push(measure_multi_symbol());

    let rust_10k = workloads
        .iter()
        .find(|w| w.name == "core_engine_10k")
        .and_then(|w| w.throughput.get("events_per_sec").and_then(|v| v.as_f64()));

    let python = run_python_baseline(10_000);
    let python_eps = python
        .as_ref()
        .and_then(|p| p.get("events_per_second").and_then(|v| v.as_f64()));

    let env = environment_metadata();
    let payload = json!({
        "schema_version": 1,
        "label": "genuine measured results — not exchange-grade claims",
        "environment": env,
        "workloads": workloads,
        "comparisons": {
            "rust_engine": {
                "workload": "core_engine_10k",
                "events": 10_000,
                "events_per_sec": rust_10k,
            },
            "python_baseline": python,
        },
        "disclaimer": "Naive Python baseline is a correctness-oriented reference, not an optimized competitor. Results are machine-specific."
    });

    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = args.summary.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.output, serde_json::to_vec_pretty(&payload)?)
        .with_context(|| format!("write {}", args.output.display()))?;
    fs::write(&args.summary, serde_json::to_vec_pretty(&payload)?)
        .with_context(|| format!("write {}", args.summary.display()))?;

    eprintln!("Wrote {}", args.output.display());
    eprintln!("Wrote {}", args.summary.display());
    if let (Some(r), Some(p)) = (rust_10k, python_eps) {
        eprintln!("Rust 10k eps≈{r:.0}  Python 10k eps≈{p:.0}");
    }
    let _ = Duration::from_secs(0);
    Ok(())
}
