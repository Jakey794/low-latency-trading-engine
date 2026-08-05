//! Strategy demonstration scenario tests.

use engine::{
    replay::{parse_jsonl, ReplayEventKind},
    runtime::{Runtime, RuntimeConfig, RuntimeEvent, RuntimeOutputKind},
    strategy::{MarketMakingConfig, MarketMakingStrategy, MomentumConfig, MomentumStrategy},
    types::Symbol,
};
use std::{fs::File, io::BufReader, path::PathBuf};

fn repo_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn load_runtime_events(path: &str) -> Vec<RuntimeEvent> {
    let file = File::open(repo_path(path)).expect("scenario exists");
    let events = parse_jsonl(BufReader::new(file)).expect("parse");
    events
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
        .collect()
}

#[test]
fn market_making_is_deterministic_and_quotes() {
    fn run() -> String {
        let mut rt = Runtime::new(vec![Symbol("AAPL".into())], RuntimeConfig::default());
        rt.add_strategy(Box::new(MarketMakingStrategy::new(
            1,
            MarketMakingConfig::default(),
        )));
        let result = rt
            .process_events(load_runtime_events(
                "data/scenarios/market_making_seed.jsonl",
            ))
            .unwrap();
        result
            .outputs
            .iter()
            .map(|o| serde_json::to_string(o).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    }
    let a = run();
    let b = run();
    assert_eq!(a, b);
    assert!(a.contains("1000000") || a.contains("accepted"));
}

#[test]
fn momentum_triggers_without_runaway() {
    let mut rt = Runtime::new(
        vec![Symbol("AAPL".into())],
        RuntimeConfig {
            max_strategy_commands_per_event: 4,
            ..RuntimeConfig::default()
        },
    );
    rt.add_strategy(Box::new(MomentumStrategy::new(
        1,
        MomentumConfig {
            window_size: 3,
            threshold_ticks: 10,
            cooldown_seqs: 1,
            order_qty: 1,
            max_abs_position: 3,
        },
    )));
    let result = rt
        .process_events(load_runtime_events("data/scenarios/momentum_seed.jsonl"))
        .unwrap();

    let strategy_orders = result
        .outputs
        .iter()
        .filter(
            |o| matches!(o.kind, RuntimeOutputKind::Accepted { order_id } if order_id >= 1_000_000),
        )
        .count();
    assert!(strategy_orders <= 4);
    // No dropped-command flood.
    assert!(!result.outputs.iter().any(|o| matches!(
        o.kind,
        RuntimeOutputKind::StrategyCommandsDropped { dropped, .. } if dropped > 0
    )));

    let again = {
        let mut rt2 = Runtime::new(
            vec![Symbol("AAPL".into())],
            RuntimeConfig {
                max_strategy_commands_per_event: 4,
                ..RuntimeConfig::default()
            },
        );
        rt2.add_strategy(Box::new(MomentumStrategy::new(
            1,
            MomentumConfig {
                window_size: 3,
                threshold_ticks: 10,
                cooldown_seqs: 1,
                order_qty: 1,
                max_abs_position: 3,
            },
        )));
        rt2.process_events(load_runtime_events("data/scenarios/momentum_seed.jsonl"))
            .unwrap()
            .outputs
    };
    assert_eq!(result.outputs, again);
}
