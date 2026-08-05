//! Property-based tests for matching and portfolio invariants.

#![allow(clippy::explicit_counter_loop)]

use engine::{
    events::{CancelOrderEvent, ExecutionReport, InputEvent, NewOrderEvent},
    matching::MatchingEngine,
    portfolio::Portfolio,
    risk::{RiskDecision, RiskLimits, RiskManager},
    types::{Order, OrderType, PriceTicks, Qty, Side, Symbol},
};
use proptest::prelude::*;

fn symbol() -> Symbol {
    Symbol("AAPL".into())
}

#[derive(Debug, Clone)]
enum GenOp {
    Limit { side: Side, px: i64, qty: u64 },
    CancelPrev,
}

fn arb_ops() -> impl Strategy<Value = Vec<GenOp>> {
    let limit = (
        prop_oneof![Just(Side::Buy), Just(Side::Sell)],
        50i64..150,
        1u64..5,
    )
        .prop_map(|(side, px, qty)| GenOp::Limit { side, px, qty });
    prop::collection::vec(prop_oneof![3 => limit, 1 => Just(GenOp::CancelPrev)], 0..40)
}

fn apply_ops(ops: &[GenOp]) -> MatchingEngine {
    let mut engine = MatchingEngine::new(symbol());
    let mut seq = 1u64;
    let mut last_id = None;
    for op in ops {
        let event = match op {
            GenOp::Limit { side, px, qty } => {
                let id = seq;
                last_id = Some(id);
                InputEvent::NewOrder(NewOrderEvent {
                    seq,
                    order: Order {
                        order_id: id,
                        symbol: symbol(),
                        side: *side,
                        order_type: OrderType::Limit,
                        price: Some(PriceTicks(*px)),
                        qty: Qty(*qty),
                        timestamp_ns: seq,
                        strategy_id: None,
                    },
                })
            }
            GenOp::CancelPrev => InputEvent::Cancel(CancelOrderEvent {
                seq,
                order_id: last_id.unwrap_or(1),
                symbol: symbol(),
                timestamp_ns: seq,
            }),
        };
        let _ = engine.process_event(event);
        seq += 1;
    }
    engine
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn book_never_crossed_and_positive_qty(ops in arb_ops()) {
        let engine = apply_ops(&ops);
        let book = engine.book();
        let snap = book.snapshot(usize::MAX);
        if let (Some(bid), Some(ask)) = (snap.bids.first(), snap.asks.first()) {
            prop_assert!(bid.price.0 < ask.price.0, "crossed book {:?} {:?}", bid.price, ask.price);
        }
        for level in snap.bids.iter().chain(snap.asks.iter()) {
            prop_assert!(level.total_qty.0 > 0);
            prop_assert!(level.order_count > 0);
            for &oid in &level.order_ids {
                let resting = book.get_order(oid).expect("index consistent");
                prop_assert!(resting.qty.0 > 0);
            }
        }
    }

    #[test]
    fn filled_qty_never_exceeds_submitted(ops in arb_ops()) {
        let mut engine = MatchingEngine::new(symbol());
        let mut submitted: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
        let mut filled: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
        let mut seq = 1u64;
        let mut last_id = None;
        for op in ops {
            match op {
                GenOp::Limit { side, px, qty } => {
                    let id = seq;
                    last_id = Some(id);
                    submitted.insert(id, qty);
                    let reports = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
                        seq,
                        order: Order {
                            order_id: id,
                            symbol: symbol(),
                            side,
                            order_type: OrderType::Limit,
                            price: Some(PriceTicks(px)),
                            qty: Qty(qty),
                            timestamp_ns: seq,
                            strategy_id: None,
                        },
                    }));
                    for r in reports {
                        if let ExecutionReport::Filled { order_id, qty, .. }
                        | ExecutionReport::PartiallyFilled { order_id, qty, .. } = r
                        {
                            *filled.entry(order_id).or_default() += qty.0;
                        }
                    }
                }
                GenOp::CancelPrev => {
                    let _ = engine.process_event(InputEvent::Cancel(CancelOrderEvent {
                        seq,
                        order_id: last_id.unwrap_or(1),
                        symbol: symbol(),
                        timestamp_ns: seq,
                    }));
                }
            }
            seq += 1;
        }
        for (id, f) in filled {
            if let Some(s) = submitted.get(&id) {
                prop_assert!(f <= *s, "order {id} filled {f} > submitted {s}");
            }
        }
    }

    #[test]
    fn risk_rejection_is_atomic(qty in 1u64..20) {
        let limits = RiskLimits {
            max_order_qty: Some(5),
            ..RiskLimits::default()
        };
        let risk = RiskManager::new(limits, 1_000_000);
        let portfolio = Portfolio::new(1_000_000);
        let mut engine = MatchingEngine::new(symbol());
        let order = Order {
            order_id: 1,
            symbol: symbol(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            price: Some(PriceTicks(100)),
            qty: Qty(qty),
            timestamp_ns: 1,
            strategy_id: None,
        };
        match risk.check_new_order(&order, &portfolio) {
            RiskDecision::Allow => {
                let _ = engine.process_event(InputEvent::NewOrder(NewOrderEvent { seq: 1, order }));
            }
            RiskDecision::Reject { .. } => {
                prop_assert!(engine.book().get_order(1).is_none());
                prop_assert_eq!(portfolio.cash(), 1_000_000);
                prop_assert_eq!(portfolio.position_qty(&symbol()), 0);
            }
        }
    }

    #[test]
    fn fifo_at_equal_price(_seed in 0u64..20) {
        let mut engine = MatchingEngine::new(symbol());
        engine.process_event(InputEvent::NewOrder(NewOrderEvent {
            seq: 1,
            order: Order {
                order_id: 1,
                symbol: symbol(),
                side: Side::Sell,
                order_type: OrderType::Limit,
                price: Some(PriceTicks(100)),
                qty: Qty(1),
                timestamp_ns: 1,
                strategy_id: None,
            },
        }));
        engine.process_event(InputEvent::NewOrder(NewOrderEvent {
            seq: 2,
            order: Order {
                order_id: 2,
                symbol: symbol(),
                side: Side::Sell,
                order_type: OrderType::Limit,
                price: Some(PriceTicks(100)),
                qty: Qty(1),
                timestamp_ns: 2,
                strategy_id: None,
            },
        }));
        let _reports = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
            seq: 3,
            order: Order {
                order_id: 3,
                symbol: symbol(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: Some(PriceTicks(100)),
                qty: Qty(1),
                timestamp_ns: 3,
                strategy_id: None,
            },
        }));
        prop_assert!(engine.book().get_order(1).is_none());
        prop_assert!(engine.book().get_order(2).is_some());
    }
}
