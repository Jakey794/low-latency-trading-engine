use engine::{
    book::assert_book_invariants,
    events::{CancelOrderEvent, InputEvent, NewOrderEvent},
    matching::MatchingEngine,
    types::{Order, OrderId, OrderType, PriceTicks, Qty, Side, Symbol},
};

fn symbol() -> Symbol {
    Symbol("AAPL".to_owned())
}

fn order(
    order_id: OrderId,
    side: Side,
    order_type: OrderType,
    price: Option<i64>,
    qty: u64,
) -> Order {
    Order {
        order_id,
        symbol: symbol(),
        side,
        order_type,
        price: price.map(PriceTicks),
        qty: Qty(qty),
        timestamp_ns: order_id,
        strategy_id: None,
    }
}

fn limit_order(order_id: OrderId, side: Side, price: i64, qty: u64) -> Order {
    order(order_id, side, OrderType::Limit, Some(price), qty)
}

fn submit_market(engine: &mut MatchingEngine, order_id: OrderId, side: Side, qty: u64) {
    engine.process_event(InputEvent::NewOrder(NewOrderEvent {
        seq: order_id,
        order: order(order_id, side, OrderType::Market, None, qty),
    }));
}

fn cancel(engine: &mut MatchingEngine, order_id: OrderId) {
    engine.process_event(InputEvent::Cancel(CancelOrderEvent {
        seq: order_id,
        order_id,
        symbol: symbol(),
        timestamp_ns: order_id,
    }));
}

#[test]
fn book_never_crossed_after_limit_matching() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 3));
    engine.submit_limit_order(limit_order(2, Side::Sell, 106, 4));
    engine.submit_limit_order(limit_order(3, Side::Buy, 105, 5));

    assert_eq!(assert_book_invariants(engine.book()), Ok(()));
    assert!(engine.book().best_bid().unwrap() < engine.book().best_ask().unwrap());
}

#[test]
fn book_never_crossed_after_market_orders() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 2));
    engine.submit_limit_order(limit_order(2, Side::Sell, 101, 3));
    submit_market(&mut engine, 3, Side::Buy, 4);

    assert_eq!(assert_book_invariants(engine.book()), Ok(()));
    assert_eq!(engine.book().best_bid(), None);
    assert_eq!(engine.book().best_ask(), Some(PriceTicks(101)));
}

#[test]
fn book_has_no_zero_qty_orders() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 10));
    engine.submit_limit_order(limit_order(2, Side::Buy, 100, 4));

    assert_eq!(assert_book_invariants(engine.book()), Ok(()));
    let snapshot = engine.book().snapshot(usize::MAX);
    for level in snapshot.bids.iter().chain(&snapshot.asks) {
        for order_id in &level.order_ids {
            assert!(engine.book().get_order(*order_id).unwrap().qty > Qty(0));
        }
    }
}

#[test]
fn empty_price_levels_removed() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 2));
    engine.submit_limit_order(limit_order(2, Side::Sell, 101, 3));
    engine.submit_limit_order(limit_order(3, Side::Buy, 100, 2));

    assert_eq!(assert_book_invariants(engine.book()), Ok(()));
    let snapshot = engine.book().snapshot(usize::MAX);
    assert_eq!(snapshot.asks.len(), 1);
    assert_eq!(snapshot.asks[0].price, PriceTicks(101));
}

#[test]
fn order_lookup_matches_book_after_fill() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Sell, 100, 2));
    engine.submit_limit_order(limit_order(2, Side::Sell, 101, 4));
    submit_market(&mut engine, 3, Side::Buy, 3);

    assert_eq!(assert_book_invariants(engine.book()), Ok(()));
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(
        engine.book().get_order(2).map(|order| order.qty),
        Some(Qty(3))
    );
}

#[test]
fn order_lookup_matches_book_after_cancel() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 100, 5));
    engine.submit_limit_order(limit_order(2, Side::Buy, 99, 7));
    cancel(&mut engine, 1);

    assert_eq!(assert_book_invariants(engine.book()), Ok(()));
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(
        engine.book().get_order(2).map(|order| order.qty),
        Some(Qty(7))
    );
}

#[test]
fn no_duplicate_active_order_ids() {
    let mut engine = MatchingEngine::new(symbol());
    engine.submit_limit_order(limit_order(1, Side::Buy, 100, 5));
    engine.submit_limit_order(limit_order(1, Side::Buy, 99, 7));

    assert_eq!(assert_book_invariants(engine.book()), Ok(()));
    let snapshot = engine.book().snapshot(usize::MAX);
    let active_ids: Vec<_> = snapshot
        .bids
        .iter()
        .chain(&snapshot.asks)
        .flat_map(|level| &level.order_ids)
        .copied()
        .collect();
    assert_eq!(active_ids, vec![1]);
}
