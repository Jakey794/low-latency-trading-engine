//! Criterion benchmarks for the matching engine and runtime hot paths.
//!
//! Setup/allocation is kept outside measured iterations where practical.
//! Results are environment-specific; do not treat them as exchange-grade latency.

use std::hint::black_box;

use criterion::{criterion_group, BenchmarkId, Criterion, Throughput};
use engine::{
    events::{CancelOrderEvent, InputEvent, NewOrderEvent},
    matching::MatchingEngine,
    metrics::{throughput_report, timed, LatencyCollector, MetricsSummary},
    replay::{parse_jsonl, ReplayDriver},
    runtime::{Runtime, RuntimeConfig, RuntimeEvent},
    strategy::{MarketMakingConfig, MarketMakingStrategy},
    types::{Order, OrderType, PriceTicks, Qty, Side, Symbol},
};

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

fn bench_add_cancel_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("hot_path");

    group.bench_function("add_non_crossing_limit", |b| {
        b.iter_with_setup(
            || MatchingEngine::new(symbol()),
            |mut engine| {
                let reports = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                    seq: 1,
                    order: limit(1, Side::Buy, 100, 1),
                }));
                black_box(reports);
            },
        );
    });

    group.bench_function("cancel_resting_order", |b| {
        b.iter_with_setup(
            || {
                let mut engine = MatchingEngine::new(symbol());
                engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                    seq: 1,
                    order: limit(1, Side::Buy, 100, 1),
                }));
                engine
            },
            |mut engine| {
                let reports = engine.process_event(InputEvent::Cancel(CancelOrderEvent {
                    seq: 2,
                    order_id: 1,
                    symbol: symbol(),
                    timestamp_ns: 2,
                }));
                black_box(reports);
            },
        );
    });

    group.bench_function("full_match", |b| {
        b.iter_with_setup(
            || {
                let mut engine = MatchingEngine::new(symbol());
                engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                    seq: 1,
                    order: limit(1, Side::Sell, 100, 5),
                }));
                engine
            },
            |mut engine| {
                let reports = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                    seq: 2,
                    order: limit(2, Side::Buy, 100, 5),
                }));
                black_box(reports);
            },
        );
    });

    group.bench_function("partial_fill", |b| {
        b.iter_with_setup(
            || {
                let mut engine = MatchingEngine::new(symbol());
                engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                    seq: 1,
                    order: limit(1, Side::Sell, 100, 10),
                }));
                engine
            },
            |mut engine| {
                let reports = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                    seq: 2,
                    order: limit(2, Side::Buy, 100, 3),
                }));
                black_box(reports);
            },
        );
    });

    group.bench_function("multi_level_sweep", |b| {
        b.iter_with_setup(
            || {
                let mut engine = MatchingEngine::new(symbol());
                for i in 0..5u64 {
                    engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                        seq: i + 1,
                        order: limit(i + 1, Side::Sell, 100 + i as i64, 2),
                    }));
                }
                engine
            },
            |mut engine| {
                let reports = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                    seq: 100,
                    order: limit(100, Side::Buy, 110, 20),
                }));
                black_box(reports);
            },
        );
    });

    group.finish();
}

fn synthetic_workload(n: u64) -> Vec<InputEvent> {
    let mut events = Vec::with_capacity(n as usize);
    for i in 0..n {
        if i % 10 == 9 {
            // cancel a prior resting order when possible
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

fn run_workload(n: u64) -> MetricsSummary {
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
                use engine::events::ExecutionReport::*;
                if matches!(r, Filled { .. } | PartiallyFilled { .. }) {
                    trades += 1;
                }
            }
            black_box(result);
        }
    });

    MetricsSummary {
        workload: format!("{n}_events"),
        latency: Some(latency.report()),
        throughput: throughput_report(n, trades, cancels, elapsed),
    }
}

fn bench_workloads(c: &mut Criterion) {
    let mut group = c.benchmark_group("workloads");
    let sizes: &[u64] = if std::env::var_os("BENCH_FULL").is_some() {
        &[10_000u64, 100_000u64]
    } else {
        &[100u64]
    };
    for &n in sizes {
        group.throughput(Throughput::Elements(n));
        group.bench_with_input(BenchmarkId::new("core_engine", n), &n, |b, &n| {
            b.iter(|| {
                let events = synthetic_workload(n);
                let mut engine = MatchingEngine::new(symbol());
                for event in events {
                    black_box(engine.process_event(event));
                }
            });
        });
    }
    group.finish();
}

fn bench_jsonl_replay(c: &mut Criterion) {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/scenarios/basic_cross.jsonl"
    );
    let data = std::fs::read_to_string(path).expect("scenario");
    c.benchmark_group("replay")
        .bench_function("jsonl_parse_and_replay", |b| {
            b.iter(|| {
                let events = parse_jsonl(std::io::Cursor::new(data.as_bytes())).unwrap();
                let mut driver = ReplayDriver::new(MatchingEngine::new(symbol()));
                black_box(driver.replay_events(events).unwrap());
            });
        });
}

fn bench_strategy_runtime(c: &mut Criterion) {
    c.bench_function("strategy_runtime_seed", |b| {
        b.iter(|| {
            let mut rt = Runtime::new(vec![symbol()], RuntimeConfig::default());
            rt.add_strategy(Box::new(MarketMakingStrategy::new(
                1,
                MarketMakingConfig::default(),
            )));
            let events = vec![
                RuntimeEvent::NewOrder {
                    seq: 1,
                    ts_ns: 100,
                    order: limit(1, Side::Buy, 100, 10),
                },
                RuntimeEvent::NewOrder {
                    seq: 2,
                    ts_ns: 200,
                    order: limit(2, Side::Sell, 110, 10),
                },
            ];
            black_box(rt.process_events(events).unwrap());
        });
    });
}

fn bench_multi_symbol(c: &mut Criterion) {
    c.bench_function("multi_symbol_interleaved", |b| {
        b.iter(|| {
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
            black_box(rt.process_events(events).unwrap());
        });
    });
}

/// Write a machine-readable summary under out/ when run as a helper.
#[allow(dead_code)]
fn write_summary_sample() {
    let summary = run_workload(10_000);
    let _ = std::fs::create_dir_all("out");
    if let Ok(f) = std::fs::File::create("out/bench_summary.json") {
        let _ = serde_json::to_writer_pretty(f, &summary);
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10).warm_up_time(std::time::Duration::from_millis(100));
    targets = bench_add_cancel_match, bench_workloads, bench_jsonl_replay, bench_strategy_runtime, bench_multi_symbol
}

fn main() {
    // `cargo test --all-targets` executes [[bench]] binaries and would otherwise
    // run full Criterion measurements for minutes. Require an explicit opt-in.
    if std::env::var_os("BENCH_FULL").is_none() {
        eprintln!(
            "engine_hot_path: smoke ok (set BENCH_FULL=1 for Criterion measurements; or use cargo bench with BENCH_FULL=1)"
        );
        return;
    }
    benches();
}
