use engine::{
    events::ExecutionReport,
    matching::MatchingEngine,
    types::{Order, OrderId, OrderType, PriceTicks, Qty, Side, Symbol},
};

fn symbol() -> Symbol {
    Symbol("AAPL".to_owned())
}

fn limit_order(order_id: OrderId, side: Side, price: i64, qty: u64) -> Order {
    Order {
        order_id,
        symbol: symbol(),
        side,
        order_type: OrderType::Limit,
        price: Some(PriceTicks(price)),
        qty: Qty(qty),
        timestamp_ns: order_id,
        strategy_id: None,
    }
}

#[test]
fn submit_non_crossing_buy_rests() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(10, Side::Sell, 101, 40));

    let reports = engine.submit_limit_order(limit_order(1, Side::Buy, 100, 25));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 1 },
            ExecutionReport::Rested {
                order_id: 1,
                remaining: Qty(25),
            },
        ]
    );
    assert_eq!(engine.book().best_bid(), Some(PriceTicks(100)));
    assert_eq!(engine.book().best_ask(), Some(PriceTicks(101)));
    assert_eq!(
        engine.book().get_order(1).map(|order| order.qty),
        Some(Qty(25))
    );
}

#[test]
fn submit_non_crossing_sell_rests() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(10, Side::Buy, 100, 40));

    let reports = engine.submit_limit_order(limit_order(2, Side::Sell, 101, 30));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 2 },
            ExecutionReport::Rested {
                order_id: 2,
                remaining: Qty(30),
            },
        ]
    );
    assert_eq!(engine.book().best_bid(), Some(PriceTicks(100)));
    assert_eq!(engine.book().best_ask(), Some(PriceTicks(101)));
    assert_eq!(
        engine.book().get_order(2).map(|order| order.qty),
        Some(Qty(30))
    );
}
