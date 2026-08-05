//! Multi-symbol runtime isolation and interleaved replay tests.

use engine::{
    replay::{parse_jsonl, ReplayEventKind},
    risk::RiskLimits,
    runtime::{Runtime, RuntimeConfig, RuntimeEvent, RuntimeOutputKind},
    types::{Order, OrderType, PriceTicks, Qty, Side, Symbol},
};
use std::{fs::File, io::BufReader, path::PathBuf};

fn repo_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn load_events(path: &str) -> Vec<RuntimeEvent> {
    let file = File::open(repo_path(path)).unwrap();
    parse_jsonl(BufReader::new(file))
        .unwrap()
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
fn interleaved_multi_symbol_is_deterministic_and_isolated() {
    fn run() -> (String, i64, i64) {
        let mut rt = Runtime::new(
            vec![Symbol("AAPL".into()), Symbol("MSFT".into())],
            RuntimeConfig::default(),
        );
        let result = rt
            .process_events(load_events("data/scenarios/multi_symbol_interleaved.jsonl"))
            .unwrap();
        let json = result
            .outputs
            .iter()
            .map(|o| serde_json::to_string(o).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        (
            json,
            rt.portfolio().position_qty(&Symbol("AAPL".into())),
            rt.portfolio().position_qty(&Symbol("MSFT".into())),
        )
    }

    let (a, aapl_pos, msft_pos) = run();
    let (b, aapl2, msft2) = run();
    assert_eq!(a, b);
    assert_eq!(aapl_pos, aapl2);
    assert_eq!(msft_pos, msft2);

    // Owned buys: AAPL +4, MSFT +3 then +7 = +10
    assert_eq!(aapl_pos, 4);
    assert_eq!(msft_pos, 10);

    // Books sorted in snapshot
    let mut rt = Runtime::new(
        vec![Symbol("MSFT".into()), Symbol("AAPL".into())],
        RuntimeConfig::default(),
    );
    let result = rt
        .process_events(load_events("data/scenarios/multi_symbol_interleaved.jsonl"))
        .unwrap();
    let symbols: Vec<_> = result.books.iter().map(|b| b.symbol.0.as_str()).collect();
    assert_eq!(symbols, vec!["AAPL", "MSFT"]);
}

#[test]
fn no_cross_symbol_cancel_or_fill_leakage() {
    let mut rt = Runtime::new(
        vec![Symbol("AAPL".into()), Symbol("MSFT".into())],
        RuntimeConfig::default(),
    );
    rt.process_events(vec![
        RuntimeEvent::NewOrder {
            seq: 1,
            ts_ns: 1,
            order: Order {
                order_id: 1,
                symbol: Symbol("AAPL".into()),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: Some(PriceTicks(100)),
                qty: Qty(5),
                timestamp_ns: 1,
                strategy_id: None,
            },
        },
        RuntimeEvent::Cancel {
            seq: 2,
            ts_ns: 2,
            order_id: 1,
            symbol: Symbol("MSFT".into()), // wrong symbol
        },
    ])
    .unwrap();

    // Cancel against wrong symbol engine should not remove AAPL order.
    assert!(rt
        .engine(&Symbol("AAPL".into()))
        .unwrap()
        .book()
        .get_order(1)
        .is_some());
}

#[test]
fn per_symbol_and_global_risk_apply() {
    let cfg = RuntimeConfig {
        risk_limits: RiskLimits {
            max_abs_position: Some(5),
            per_symbol_max_order_qty: [("AAPL".into(), 2)].into_iter().collect(),
            ..RiskLimits::default()
        },
        ..RuntimeConfig::default()
    };
    let mut rt = Runtime::new(vec![Symbol("AAPL".into()), Symbol("MSFT".into())], cfg);

    let result = rt
        .process_events(vec![RuntimeEvent::NewOrder {
            seq: 1,
            ts_ns: 1,
            order: Order {
                order_id: 10,
                symbol: Symbol("AAPL".into()),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: Some(PriceTicks(100)),
                qty: Qty(3),
                timestamp_ns: 1,
                strategy_id: Some(1),
            },
        }])
        .unwrap();

    assert!(result
        .outputs
        .iter()
        .any(|o| matches!(o.kind, RuntimeOutputKind::RiskRejected { .. })));
    assert_eq!(rt.portfolio().position_qty(&Symbol("AAPL".into())), 0);
    assert_eq!(rt.portfolio().position_qty(&Symbol("MSFT".into())), 0);
}
