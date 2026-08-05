//! Integration tests for runtime + strategy orchestration.

use engine::{
    risk::RiskLimits,
    runtime::{Runtime, RuntimeConfig, RuntimeEvent, RuntimeOutputKind},
    strategy::{Strategy, StrategyCommand, StrategyContext, StrategyEvent},
    types::{Order, OrderType, PriceTicks, Qty, Side, StrategyId, Symbol},
};

fn sym() -> Symbol {
    Symbol("AAPL".into())
}

struct OnceBuy {
    id: StrategyId,
    done: bool,
}

impl Strategy for OnceBuy {
    fn id(&self) -> StrategyId {
        self.id
    }
    fn name(&self) -> &str {
        "once_buy"
    }
    fn on_event(&mut self, event: &StrategyEvent, ctx: &StrategyContext) -> Vec<StrategyCommand> {
        if self.done || ctx.risk_killed {
            return Vec::new();
        }
        if let StrategyEvent::BookUpdate { symbol, book } = event {
            if book.asks.is_empty() {
                return Vec::new();
            }
            self.done = true;
            return vec![StrategyCommand::PlaceOrder {
                symbol: symbol.clone(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: Some(PriceTicks(100)),
                qty: Qty(2),
            }];
        }
        Vec::new()
    }
}

#[test]
fn end_to_end_strategy_risk_fill_deterministic() {
    fn run() -> (String, i64, i128) {
        let mut rt = Runtime::new(vec![sym()], RuntimeConfig::default());
        rt.add_strategy(Box::new(OnceBuy {
            id: 42,
            done: false,
        }));
        let result = rt
            .process_events(vec![RuntimeEvent::NewOrder {
                seq: 1,
                ts_ns: 100,
                order: Order {
                    order_id: 1,
                    symbol: sym(),
                    side: Side::Sell,
                    order_type: OrderType::Limit,
                    price: Some(PriceTicks(100)),
                    qty: Qty(10),
                    timestamp_ns: 10,
                    strategy_id: None,
                },
            }])
            .unwrap();

        let json = result
            .outputs
            .iter()
            .map(|o| serde_json::to_string(o).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        (
            json,
            rt.portfolio().position_qty(&sym()),
            rt.portfolio().cash(),
        )
    }

    let (a, pos_a, cash_a) = run();
    let (b, pos_b, cash_b) = run();
    assert_eq!(a, b);
    assert_eq!(pos_a, pos_b);
    assert_eq!(cash_a, cash_b);
    assert_eq!(pos_a, 2);
    assert_eq!(cash_a, 1_000_000 - 200);
    assert!(a.contains("trade"));
    assert!(a.contains("1000000")); // deterministic strategy order id
}

#[test]
fn risk_rejection_notifies_strategy_without_book_mutation() {
    struct CaptureReject;

    impl Strategy for CaptureReject {
        fn id(&self) -> StrategyId {
            1
        }
        fn name(&self) -> &str {
            "capture"
        }
        fn on_event(
            &mut self,
            event: &StrategyEvent,
            _ctx: &StrategyContext,
        ) -> Vec<StrategyCommand> {
            if matches!(event, StrategyEvent::Timer { .. }) {
                return vec![StrategyCommand::PlaceOrder {
                    symbol: sym(),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    price: Some(PriceTicks(100)),
                    qty: Qty(100),
                }];
            }
            Vec::new()
        }
    }

    let cfg = RuntimeConfig {
        risk_limits: RiskLimits {
            max_order_qty: Some(10),
            ..RiskLimits::default()
        },
        ..RuntimeConfig::default()
    };

    let mut rt = Runtime::new(vec![sym()], cfg);
    rt.add_strategy(Box::new(CaptureReject));

    let result = rt
        .process_events(vec![RuntimeEvent::Timer {
            seq: 1,
            ts_ns: 1,
            symbol: Some(sym()),
        }])
        .unwrap();

    assert!(result
        .outputs
        .iter()
        .any(|o| matches!(o.kind, RuntimeOutputKind::RiskRejected { .. })));
    assert!(rt
        .engine(&sym())
        .unwrap()
        .book()
        .get_order(1_000_000)
        .is_none());
}
