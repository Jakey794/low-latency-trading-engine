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

#[test]
fn buy_limit_fully_fills_against_best_ask() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 10));

    let reports = engine.submit_limit_order(limit_order(2, Side::Buy, 100, 10));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 2 },
            ExecutionReport::Filled {
                order_id: 2,
                qty: Qty(10),
                price: PriceTicks(100),
            },
        ]
    );
    assert_eq!(engine.book().best_ask(), None);
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(engine.book().get_order(2), None);
}

#[test]
fn sell_limit_fully_fills_against_best_bid() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 100, 10));

    let reports = engine.submit_limit_order(limit_order(2, Side::Sell, 100, 10));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 2 },
            ExecutionReport::Filled {
                order_id: 2,
                qty: Qty(10),
                price: PriceTicks(100),
            },
        ]
    );
    assert_eq!(engine.book().best_bid(), None);
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(engine.book().get_order(2), None);
}

#[test]
fn trade_price_is_resting_order_price() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 10));

    let reports = engine.submit_limit_order(limit_order(2, Side::Buy, 105, 10));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 2 },
            ExecutionReport::Filled {
                order_id: 2,
                qty: Qty(10),
                price: PriceTicks(100),
            },
        ]
    );
}

#[test]
fn fully_filled_resting_order_removed_from_book() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 10));
    engine.submit_limit_order(limit_order(3, Side::Sell, 102, 5));

    engine.submit_limit_order(limit_order(2, Side::Buy, 100, 10));

    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(engine.book().best_ask(), Some(PriceTicks(102)));
    assert_eq!(
        engine.book().get_order(3).map(|order| order.qty),
        Some(Qty(5))
    );
}

#[test]
fn fully_filled_incoming_order_does_not_rest() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 10));

    let reports = engine.submit_limit_order(limit_order(2, Side::Buy, 100, 10));

    assert_eq!(engine.book().get_order(2), None);
    assert_eq!(engine.book().best_bid(), None);
    assert!(engine.book().snapshot(usize::MAX).bids.is_empty());
    assert!(reports
        .iter()
        .all(|report| !matches!(report, ExecutionReport::Rested { .. })));
}

#[test]
fn incoming_buy_partially_fills_resting_ask() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 10));

    let reports = engine.submit_limit_order(limit_order(2, Side::Buy, 100, 4));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 2 },
            ExecutionReport::Filled {
                order_id: 2,
                qty: Qty(4),
                price: PriceTicks(100),
            },
        ]
    );
    assert_eq!(engine.book().best_ask(), Some(PriceTicks(100)));
    assert_eq!(
        engine.book().get_order(1).map(|order| order.qty),
        Some(Qty(6))
    );
    assert_eq!(engine.book().get_order(2), None);
}

#[test]
fn incoming_sell_partially_fills_resting_bid() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 100, 10));

    let reports = engine.submit_limit_order(limit_order(2, Side::Sell, 100, 4));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 2 },
            ExecutionReport::Filled {
                order_id: 2,
                qty: Qty(4),
                price: PriceTicks(100),
            },
        ]
    );
    assert_eq!(engine.book().best_bid(), Some(PriceTicks(100)));
    assert_eq!(
        engine.book().get_order(1).map(|order| order.qty),
        Some(Qty(6))
    );
    assert_eq!(engine.book().get_order(2), None);
}

#[test]
fn partially_filled_resting_order_stays_at_front() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 10));
    engine.submit_limit_order(limit_order(3, Side::Sell, 100, 20));

    engine.submit_limit_order(limit_order(2, Side::Buy, 100, 4));

    let best_ask = &engine.book().snapshot(1).asks[0];
    assert_eq!(best_ask.order_ids, vec![1, 3]);
    assert_eq!(
        engine.book().get_order(1).map(|order| order.qty),
        Some(Qty(6))
    );
}

#[test]
fn resting_order_remaining_qty_is_correct() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 10));

    engine.submit_limit_order(limit_order(2, Side::Buy, 105, 4));

    let best_ask = &engine.book().snapshot(1).asks[0];
    assert_eq!(best_ask.total_qty, Qty(6));
    assert_eq!(best_ask.order_ids, vec![1]);
    assert_eq!(
        engine.book().get_order(1).map(|order| order.qty),
        Some(Qty(6))
    );
}

#[test]
fn incoming_order_fully_filled_after_partial_against_larger_resting_order() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 10));

    let reports = engine.submit_limit_order(limit_order(2, Side::Buy, 100, 4));

    assert_eq!(engine.book().get_order(2), None);
    assert_eq!(engine.book().best_bid(), None);
    assert!(engine.book().snapshot(usize::MAX).bids.is_empty());
    assert!(reports
        .iter()
        .all(|report| !matches!(report, ExecutionReport::Rested { .. })));
}
