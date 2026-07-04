use engine::{
    events::{CancelOrderEvent, ExecutionReport, InputEvent, NewOrderEvent, RejectReason},
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

fn cancel(engine: &mut MatchingEngine, order_id: OrderId) -> Vec<ExecutionReport> {
    engine.process_event(InputEvent::Cancel(CancelOrderEvent {
        seq: 1,
        order_id,
        symbol: symbol(),
        timestamp_ns: 1,
    }))
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

fn submit_market(engine: &mut MatchingEngine, order_id: OrderId, side: Side, qty: u64) {
    engine.process_event(InputEvent::NewOrder(NewOrderEvent {
        seq: 1,
        order: market_order(order_id, side, qty),
    }));
}

#[test]
fn cancel_existing_bid() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 100, 5));

    let reports = cancel(&mut engine, 1);

    assert_eq!(reports, vec![ExecutionReport::Cancelled { order_id: 1 }]);
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(engine.book().best_bid(), None);
}

#[test]
fn cancel_existing_ask() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 101, 5));

    let reports = cancel(&mut engine, 1);

    assert_eq!(reports, vec![ExecutionReport::Cancelled { order_id: 1 }]);
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(engine.book().best_ask(), None);
}

#[test]
fn cancel_unknown_order_rejected() {
    let mut engine = MatchingEngine::new(symbol());

    let reports = cancel(&mut engine, 99);

    assert_eq!(
        reports,
        vec![ExecutionReport::Rejected {
            order_id: 99,
            reason: RejectReason::UnknownOrder,
        }]
    );
}

#[test]
fn cancel_removes_order_from_snapshot() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 100, 5));
    engine.submit_limit_order(limit_order(2, Side::Buy, 100, 7));

    cancel(&mut engine, 1);

    let snapshot = engine.book().snapshot(usize::MAX);
    assert_eq!(snapshot.bids[0].order_ids, vec![2]);
    assert_eq!(snapshot.bids[0].order_count, 1);
    assert_eq!(snapshot.bids[0].total_qty, Qty(7));
}

#[test]
fn cancel_removes_empty_price_level() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 100, 5));
    engine.submit_limit_order(limit_order(2, Side::Buy, 99, 7));

    cancel(&mut engine, 1);

    let snapshot = engine.book().snapshot(usize::MAX);
    assert_eq!(snapshot.bids.len(), 1);
    assert_eq!(snapshot.bids[0].price, PriceTicks(99));
    assert_eq!(engine.book().best_bid(), Some(PriceTicks(99)));
}

#[test]
fn cancel_preserves_fifo_for_remaining_orders() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 100, 2));
    engine.submit_limit_order(limit_order(2, Side::Buy, 100, 3));
    engine.submit_limit_order(limit_order(3, Side::Buy, 100, 4));

    cancel(&mut engine, 2);

    assert_eq!(engine.book().snapshot(1).bids[0].order_ids, vec![1, 3]);
    engine.submit_limit_order(limit_order(10, Side::Sell, 100, 2));
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(
        engine.book().get_order(3).map(|order| order.qty),
        Some(Qty(4))
    );
}

#[test]
fn fully_filled_order_is_not_cancellable() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 5));
    engine.submit_limit_order(limit_order(2, Side::Buy, 100, 5));

    let reports = cancel(&mut engine, 1);

    assert_eq!(
        reports,
        vec![ExecutionReport::Rejected {
            order_id: 1,
            reason: RejectReason::AlreadyFilled,
        }]
    );
    assert_eq!(engine.book().get_order(1), None);
}

#[test]
fn cancel_after_full_fill_rejected() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 5));
    engine.submit_limit_order(limit_order(2, Side::Buy, 100, 5));

    assert_eq!(
        cancel(&mut engine, 1),
        vec![ExecutionReport::Rejected {
            order_id: 1,
            reason: RejectReason::AlreadyFilled,
        }]
    );
}

#[test]
fn cancel_after_cancel_rejected() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 100, 5));
    assert_eq!(
        cancel(&mut engine, 1),
        vec![ExecutionReport::Cancelled { order_id: 1 }]
    );

    assert_eq!(
        cancel(&mut engine, 1),
        vec![ExecutionReport::Rejected {
            order_id: 1,
            reason: RejectReason::AlreadyCancelled,
        }]
    );
}

#[test]
fn cancel_partially_filled_resting_order_succeeds() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 10));
    engine.submit_limit_order(limit_order(2, Side::Buy, 100, 4));

    assert_eq!(
        engine.book().get_order(1).map(|order| order.qty),
        Some(Qty(6))
    );
    assert_eq!(
        cancel(&mut engine, 1),
        vec![ExecutionReport::Cancelled { order_id: 1 }]
    );
}

#[test]
fn cancel_partially_filled_order_removes_remaining_qty() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 10));
    engine.submit_limit_order(limit_order(2, Side::Buy, 100, 4));

    cancel(&mut engine, 1);

    assert_eq!(engine.book().get_order(1), None);
    assert!(engine.book().snapshot(usize::MAX).asks.is_empty());
    assert_eq!(engine.book().best_ask(), None);
}

#[test]
fn cancel_aggressive_market_order_rejected() {
    let mut fully_filled_engine = MatchingEngine::new(symbol());
    fully_filled_engine.submit_limit_order(limit_order(1, Side::Sell, 100, 5));
    submit_market(&mut fully_filled_engine, 2, Side::Buy, 5);
    assert_eq!(
        cancel(&mut fully_filled_engine, 2),
        vec![ExecutionReport::Rejected {
            order_id: 2,
            reason: RejectReason::AlreadyFilled,
        }]
    );

    let mut partially_filled_engine = MatchingEngine::new(symbol());
    partially_filled_engine.submit_limit_order(limit_order(10, Side::Sell, 100, 3));
    submit_market(&mut partially_filled_engine, 11, Side::Buy, 5);
    assert_eq!(
        cancel(&mut partially_filled_engine, 11),
        vec![ExecutionReport::Rejected {
            order_id: 11,
            reason: RejectReason::AlreadyExpired,
        }]
    );
}

#[test]
fn cancel_aggressive_limit_order_that_never_rested_rejected() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 5));
    engine.submit_limit_order(limit_order(2, Side::Buy, 101, 5));

    assert_eq!(
        cancel(&mut engine, 2),
        vec![ExecutionReport::Rejected {
            order_id: 2,
            reason: RejectReason::AlreadyFilled,
        }]
    );
}

#[test]
fn cancel_aggressive_limit_remainder_that_rested_succeeds() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 3));
    engine.submit_limit_order(limit_order(2, Side::Buy, 101, 5));

    assert_eq!(
        engine.book().get_order(2).map(|order| order.qty),
        Some(Qty(2))
    );
    assert_eq!(
        cancel(&mut engine, 2),
        vec![ExecutionReport::Cancelled { order_id: 2 }]
    );
    assert_eq!(engine.book().get_order(2), None);
    assert_eq!(engine.book().best_bid(), None);
}
