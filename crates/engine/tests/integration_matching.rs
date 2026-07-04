use engine::{
    events::{ExecutionReport, InputEvent, NewOrderEvent, RejectReason},
    matching::MatchingEngine,
    types::{Order, OrderId, OrderType, PriceTicks, Qty, Side, Symbol},
};

fn symbol() -> Symbol {
    Symbol("AAPL".to_owned())
}

fn market_order(order_id: OrderId, side: Side, qty: u64) -> Order {
    Order {
        order_id,
        symbol: symbol(),
        side,
        order_type: OrderType::Market,
        price: None,
        qty: Qty(qty),
        timestamp_ns: order_id,
        strategy_id: None,
    }
}

fn submit_market_order(engine: &mut MatchingEngine, order: Order) -> Vec<ExecutionReport> {
    engine.process_event(InputEvent::NewOrder(NewOrderEvent { seq: 1, order }))
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

fn assert_public_book_invariants(engine: &MatchingEngine) {
    let snapshot = engine.book().snapshot(usize::MAX);

    for level in snapshot.bids.iter().chain(&snapshot.asks) {
        assert!(level.total_qty > Qty(0));
        assert!(level.order_count > 0);
        assert_eq!(level.order_count, level.order_ids.len());
        for order_id in &level.order_ids {
            let order = engine
                .book()
                .get_order(*order_id)
                .expect("snapshot order must be present in the order index");
            assert!(order.qty > Qty(0));
        }
    }

    if let (Some(best_bid), Some(best_ask)) = (engine.book().best_bid(), engine.book().best_ask()) {
        assert!(best_bid < best_ask);
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

#[test]
fn buy_consumes_multiple_ask_orders_same_price() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 3));
    engine.submit_limit_order(limit_order(2, Side::Sell, 100, 5));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 100, 4));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(3),
                remaining: Qty(1),
                price: PriceTicks(100),
            },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(1),
                price: PriceTicks(100),
            },
        ]
    );
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(
        engine.book().get_order(2).map(|order| order.qty),
        Some(Qty(4))
    );
    assert_eq!(engine.book().snapshot(1).asks[0].order_ids, vec![2]);
}

#[test]
fn sell_consumes_multiple_bid_orders_same_price() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 100, 3));
    engine.submit_limit_order(limit_order(2, Side::Buy, 100, 5));

    let reports = engine.submit_limit_order(limit_order(10, Side::Sell, 100, 4));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(3),
                remaining: Qty(1),
                price: PriceTicks(100),
            },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(1),
                price: PriceTicks(100),
            },
        ]
    );
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(
        engine.book().get_order(2).map(|order| order.qty),
        Some(Qty(4))
    );
    assert_eq!(engine.book().snapshot(1).bids[0].order_ids, vec![2]);
}

#[test]
fn buy_consumes_multiple_price_levels() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 2));
    engine.submit_limit_order(limit_order(2, Side::Sell, 101, 3));
    engine.submit_limit_order(limit_order(3, Side::Sell, 102, 5));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 101, 5));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(2),
                remaining: Qty(3),
                price: PriceTicks(100),
            },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(3),
                price: PriceTicks(101),
            },
        ]
    );
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(engine.book().get_order(2), None);
    assert_eq!(engine.book().best_ask(), Some(PriceTicks(102)));
}

#[test]
fn sell_consumes_multiple_price_levels() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 101, 2));
    engine.submit_limit_order(limit_order(2, Side::Buy, 100, 3));
    engine.submit_limit_order(limit_order(3, Side::Buy, 99, 4));
    engine.submit_limit_order(limit_order(4, Side::Buy, 98, 5));

    let reports = engine.submit_limit_order(limit_order(10, Side::Sell, 99, 9));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(2),
                remaining: Qty(7),
                price: PriceTicks(101),
            },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(3),
                remaining: Qty(4),
                price: PriceTicks(100),
            },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(4),
                price: PriceTicks(99),
            },
        ]
    );
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(engine.book().get_order(2), None);
    assert_eq!(engine.book().get_order(3), None);
    assert_eq!(engine.book().best_bid(), Some(PriceTicks(98)));
}

#[test]
fn incoming_stops_when_limit_price_no_longer_crosses() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 3));
    engine.submit_limit_order(limit_order(2, Side::Sell, 101, 4));
    engine.submit_limit_order(limit_order(3, Side::Sell, 102, 5));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 101, 10));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(3),
                remaining: Qty(7),
                price: PriceTicks(100),
            },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(4),
                remaining: Qty(3),
                price: PriceTicks(101),
            },
            ExecutionReport::Rested {
                order_id: 10,
                remaining: Qty(3),
            },
        ]
    );
    assert_eq!(engine.book().best_bid(), Some(PriceTicks(101)));
    assert_eq!(engine.book().best_ask(), Some(PriceTicks(102)));
    assert_eq!(
        engine.book().get_order(10).map(|order| order.qty),
        Some(Qty(3))
    );
    assert_eq!(
        engine.book().get_order(3).map(|order| order.qty),
        Some(Qty(5))
    );
}

#[test]
fn empty_price_levels_are_removed() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 2));
    engine.submit_limit_order(limit_order(2, Side::Sell, 101, 3));

    engine.submit_limit_order(limit_order(10, Side::Buy, 101, 5));

    assert!(engine.book().snapshot(usize::MAX).asks.is_empty());
    assert_eq!(engine.book().best_ask(), None);
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(engine.book().get_order(2), None);
}

#[test]
fn fill_reports_are_in_matching_order() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 2));
    engine.submit_limit_order(limit_order(2, Side::Sell, 100, 3));
    engine.submit_limit_order(limit_order(3, Side::Sell, 101, 4));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 101, 9));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(2),
                remaining: Qty(7),
                price: PriceTicks(100),
            },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(3),
                remaining: Qty(4),
                price: PriceTicks(100),
            },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(4),
                price: PriceTicks(101),
            },
        ]
    );
}

#[test]
fn buy_residual_rests_after_consuming_all_asks() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 5));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 100, 10));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(5),
                remaining: Qty(5),
                price: PriceTicks(100),
            },
            ExecutionReport::Rested {
                order_id: 10,
                remaining: Qty(5),
            },
        ]
    );
    assert_eq!(engine.book().best_ask(), None);
    assert_eq!(engine.book().best_bid(), Some(PriceTicks(100)));
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(
        engine.book().get_order(10).map(|order| order.qty),
        Some(Qty(5))
    );
}

#[test]
fn sell_residual_rests_after_consuming_all_bids() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 100, 5));

    let reports = engine.submit_limit_order(limit_order(10, Side::Sell, 100, 10));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(5),
                remaining: Qty(5),
                price: PriceTicks(100),
            },
            ExecutionReport::Rested {
                order_id: 10,
                remaining: Qty(5),
            },
        ]
    );
    assert_eq!(engine.book().best_bid(), None);
    assert_eq!(engine.book().best_ask(), Some(PriceTicks(100)));
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(
        engine.book().get_order(10).map(|order| order.qty),
        Some(Qty(5))
    );
}

#[test]
fn fully_filled_order_does_not_rest() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 5));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 100, 5));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(5),
                price: PriceTicks(100),
            },
        ]
    );
    assert_eq!(engine.book().get_order(10), None);
    assert_eq!(engine.book().best_bid(), None);
    assert!(reports
        .iter()
        .all(|report| !matches!(report, ExecutionReport::Rested { .. })));
}

#[test]
fn residual_order_has_correct_remaining_qty() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 99, 2));
    engine.submit_limit_order(limit_order(2, Side::Sell, 100, 3));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 100, 8));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(2),
                remaining: Qty(6),
                price: PriceTicks(99),
            },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(3),
                remaining: Qty(3),
                price: PriceTicks(100),
            },
            ExecutionReport::Rested {
                order_id: 10,
                remaining: Qty(3),
            },
        ]
    );
    let best_bid = &engine.book().snapshot(1).bids[0];
    assert_eq!(best_bid.price, PriceTicks(100));
    assert_eq!(best_bid.total_qty, Qty(3));
    assert_eq!(best_bid.order_ids, vec![10]);
    assert_eq!(
        engine.book().get_order(10).map(|order| order.qty),
        Some(Qty(3))
    );
}

#[test]
fn residual_order_has_new_time_priority_at_its_price() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 5));
    engine.submit_limit_order(limit_order(10, Side::Buy, 100, 10));

    let later_reports = engine.submit_limit_order(limit_order(11, Side::Buy, 100, 2));

    assert_eq!(
        later_reports,
        vec![
            ExecutionReport::Accepted { order_id: 11 },
            ExecutionReport::Rested {
                order_id: 11,
                remaining: Qty(2),
            },
        ]
    );
    let best_bid = &engine.book().snapshot(1).bids[0];
    assert_eq!(best_bid.order_ids, vec![10, 11]);
    assert_eq!(best_bid.total_qty, Qty(7));
}

#[test]
fn book_not_crossed_after_residual_rest() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 99, 5));
    engine.submit_limit_order(limit_order(2, Side::Sell, 101, 5));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 100, 10));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(5),
                remaining: Qty(5),
                price: PriceTicks(99),
            },
            ExecutionReport::Rested {
                order_id: 10,
                remaining: Qty(5),
            },
        ]
    );
    let best_bid = engine.book().best_bid().unwrap();
    let best_ask = engine.book().best_ask().unwrap();
    assert_eq!(best_bid, PriceTicks(100));
    assert_eq!(best_ask, PriceTicks(101));
    assert!(best_bid < best_ask);
}

#[test]
fn scenario_simple_cross() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 5));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 100, 5));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(5),
                price: PriceTicks(100),
            },
        ]
    );
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(engine.book().get_order(10), None);
    assert_public_book_invariants(&engine);
}

#[test]
fn scenario_partial_fill() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 10));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 100, 4));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(4),
                price: PriceTicks(100),
            },
        ]
    );
    assert_eq!(
        engine.book().get_order(1).map(|order| order.qty),
        Some(Qty(6))
    );
    assert_eq!(engine.book().get_order(10), None);
    assert_public_book_invariants(&engine);
}

#[test]
fn scenario_multi_level_fill() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 2));
    engine.submit_limit_order(limit_order(2, Side::Sell, 101, 3));
    engine.submit_limit_order(limit_order(3, Side::Sell, 102, 4));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 101, 5));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(2),
                remaining: Qty(3),
                price: PriceTicks(100),
            },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(3),
                price: PriceTicks(101),
            },
        ]
    );
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(engine.book().get_order(2), None);
    assert_eq!(engine.book().best_ask(), Some(PriceTicks(102)));
    assert_public_book_invariants(&engine);
}

#[test]
fn scenario_residual_rests() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 3));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 100, 5));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(3),
                remaining: Qty(2),
                price: PriceTicks(100),
            },
            ExecutionReport::Rested {
                order_id: 10,
                remaining: Qty(2),
            },
        ]
    );
    assert_eq!(
        engine.book().get_order(10).map(|order| order.qty),
        Some(Qty(2))
    );
    assert_eq!(engine.book().best_bid(), Some(PriceTicks(100)));
    assert_public_book_invariants(&engine);
}

#[test]
fn scenario_fifo_priority() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 2));
    engine.submit_limit_order(limit_order(2, Side::Sell, 100, 3));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 100, 3));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(2),
                remaining: Qty(1),
                price: PriceTicks(100),
            },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(1),
                price: PriceTicks(100),
            },
        ]
    );
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(
        engine.book().get_order(2).map(|order| order.qty),
        Some(Qty(2))
    );
    assert_eq!(engine.book().snapshot(1).asks[0].order_ids, vec![2]);
    assert_public_book_invariants(&engine);
}

#[test]
fn scenario_trade_price_is_resting_price() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 2));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 105, 2));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(2),
                price: PriceTicks(100),
            },
        ]
    );
    assert_public_book_invariants(&engine);
}

#[test]
fn scenario_book_cleanup_after_fills() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 2));
    engine.submit_limit_order(limit_order(2, Side::Sell, 101, 3));
    engine.submit_limit_order(limit_order(3, Side::Sell, 102, 4));

    let reports = engine.submit_limit_order(limit_order(10, Side::Buy, 101, 5));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(2),
                remaining: Qty(3),
                price: PriceTicks(100),
            },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(3),
                price: PriceTicks(101),
            },
        ]
    );
    let snapshot = engine.book().snapshot(usize::MAX);
    assert_eq!(snapshot.asks.len(), 1);
    assert_eq!(snapshot.asks[0].price, PriceTicks(102));
    assert_eq!(snapshot.asks[0].order_ids, vec![3]);
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(engine.book().get_order(2), None);
    assert_eq!(
        engine.book().get_order(3).map(|order| order.qty),
        Some(Qty(4))
    );
    assert_public_book_invariants(&engine);
}

#[test]
fn market_buy_consumes_best_ask() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 10));

    let reports = submit_market_order(&mut engine, market_order(10, Side::Buy, 4));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(4),
                price: PriceTicks(100),
            },
        ]
    );
    assert_eq!(
        engine.book().get_order(1).map(|order| order.qty),
        Some(Qty(6))
    );
    assert_eq!(engine.book().get_order(10), None);
    assert_public_book_invariants(&engine);
}

#[test]
fn market_sell_consumes_best_bid() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 100, 10));

    let reports = submit_market_order(&mut engine, market_order(10, Side::Sell, 4));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(4),
                price: PriceTicks(100),
            },
        ]
    );
    assert_eq!(
        engine.book().get_order(1).map(|order| order.qty),
        Some(Qty(6))
    );
    assert_eq!(engine.book().get_order(10), None);
    assert_public_book_invariants(&engine);
}

#[test]
fn market_order_sweeps_multiple_levels() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 2));
    engine.submit_limit_order(limit_order(2, Side::Sell, 101, 3));
    engine.submit_limit_order(limit_order(3, Side::Sell, 102, 4));

    let reports = submit_market_order(&mut engine, market_order(10, Side::Buy, 7));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(2),
                remaining: Qty(5),
                price: PriceTicks(100),
            },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(3),
                remaining: Qty(2),
                price: PriceTicks(101),
            },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(2),
                price: PriceTicks(102),
            },
        ]
    );
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(engine.book().get_order(2), None);
    assert_eq!(
        engine.book().get_order(3).map(|order| order.qty),
        Some(Qty(2))
    );
    assert_public_book_invariants(&engine);
}

#[test]
fn market_order_partial_fill_expires_remainder() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 3));

    let reports = submit_market_order(&mut engine, market_order(10, Side::Buy, 5));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::PartiallyFilled {
                order_id: 10,
                qty: Qty(3),
                remaining: Qty(2),
                price: PriceTicks(100),
            },
            ExecutionReport::Expired {
                order_id: 10,
                remaining: Qty(2),
            },
        ]
    );
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(engine.book().get_order(10), None);
    assert_public_book_invariants(&engine);
}

#[test]
fn market_order_empty_book_rejected() {
    let mut engine = MatchingEngine::new(symbol());

    let reports = submit_market_order(&mut engine, market_order(10, Side::Buy, 5));

    assert_eq!(
        reports,
        vec![ExecutionReport::Rejected {
            order_id: 10,
            reason: RejectReason::EmptyBook,
        }]
    );
}

#[test]
fn market_order_never_rests() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 100, 2));

    let reports = submit_market_order(&mut engine, market_order(10, Side::Sell, 5));

    assert!(reports
        .iter()
        .all(|report| !matches!(report, ExecutionReport::Rested { .. })));
    assert_eq!(engine.book().get_order(10), None);
    assert_eq!(engine.book().best_ask(), None);
    assert_public_book_invariants(&engine);
}

#[test]
fn trade_price_uses_resting_order_price() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 107, 2));

    let reports = submit_market_order(&mut engine, market_order(10, Side::Buy, 2));

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 10 },
            ExecutionReport::Filled {
                order_id: 10,
                qty: Qty(2),
                price: PriceTicks(107),
            },
        ]
    );
    assert_public_book_invariants(&engine);
}

#[test]
fn expired_market_order_id_cannot_be_reused() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 3));

    let first_reports = submit_market_order(&mut engine, market_order(10, Side::Buy, 5));
    engine.submit_limit_order(limit_order(2, Side::Sell, 100, 5));
    let repeated_market = submit_market_order(&mut engine, market_order(10, Side::Buy, 5));
    let repeated_limit = engine.submit_limit_order(limit_order(10, Side::Buy, 100, 5));

    assert_eq!(
        first_reports.last(),
        Some(&ExecutionReport::Expired {
            order_id: 10,
            remaining: Qty(2),
        })
    );
    for reports in [repeated_market, repeated_limit] {
        assert_eq!(
            reports,
            vec![ExecutionReport::Rejected {
                order_id: 10,
                reason: RejectReason::AlreadyExpired,
            }]
        );
    }
    assert_eq!(
        engine.book().get_order(2).map(|order| order.qty),
        Some(Qty(5))
    );
    assert_public_book_invariants(&engine);
}

#[test]
fn market_order_with_only_same_side_liquidity_is_not_empty_book() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 100, 5));

    let reports = submit_market_order(&mut engine, market_order(10, Side::Buy, 5));

    assert_eq!(
        reports,
        vec![ExecutionReport::Rejected {
            order_id: 10,
            reason: RejectReason::MarketOrderWouldNotFill,
        }]
    );
    assert_eq!(
        engine.book().get_order(1).map(|order| order.qty),
        Some(Qty(5))
    );
    assert_public_book_invariants(&engine);
}
