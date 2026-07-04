use engine::{
    book::assert_book_invariants,
    events::{CancelOrderEvent, ExecutionReport, InputEvent, NewOrderEvent, RejectReason},
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

fn submit_limit(
    engine: &mut MatchingEngine,
    order_id: OrderId,
    side: Side,
    price: i64,
    qty: u64,
) -> Vec<ExecutionReport> {
    engine.process_event(InputEvent::NewOrder(NewOrderEvent {
        seq: order_id,
        order: order(order_id, side, OrderType::Limit, Some(price), qty),
    }))
}

fn submit_market(
    engine: &mut MatchingEngine,
    order_id: OrderId,
    side: Side,
    qty: u64,
) -> Vec<ExecutionReport> {
    engine.process_event(InputEvent::NewOrder(NewOrderEvent {
        seq: order_id,
        order: order(order_id, side, OrderType::Market, None, qty),
    }))
}

fn cancel(engine: &mut MatchingEngine, order_id: OrderId) -> Vec<ExecutionReport> {
    engine.process_event(InputEvent::Cancel(CancelOrderEvent {
        seq: order_id + 100,
        order_id,
        symbol: symbol(),
        timestamp_ns: order_id + 100,
    }))
}

fn assert_valid(engine: &MatchingEngine) {
    assert_eq!(assert_book_invariants(engine.book()), Ok(()));
}

#[test]
fn simple_market_buy() {
    let mut engine = MatchingEngine::new(symbol());
    assert_eq!(
        submit_limit(&mut engine, 1, Side::Sell, 1000, 100),
        vec![
            ExecutionReport::Accepted { order_id: 1 },
            ExecutionReport::Rested {
                order_id: 1,
                remaining: Qty(100),
            },
        ]
    );
    assert_valid(&engine);

    let reports = submit_market(&mut engine, 2, Side::Buy, 40);

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 2 },
            ExecutionReport::Filled {
                order_id: 2,
                qty: Qty(40),
                price: PriceTicks(1000),
            },
        ]
    );
    assert_eq!(
        engine.book().get_order(1).map(|order| order.qty),
        Some(Qty(60))
    );
    let snapshot = engine.book().snapshot(usize::MAX);
    assert_eq!(snapshot.asks[0].total_qty, Qty(60));
    assert_valid(&engine);
}

#[test]
fn multi_level_market_sweep() {
    let mut engine = MatchingEngine::new(symbol());
    submit_limit(&mut engine, 1, Side::Sell, 1000, 50);
    submit_limit(&mut engine, 2, Side::Sell, 1001, 50);
    submit_limit(&mut engine, 3, Side::Sell, 1002, 50);
    assert_valid(&engine);

    let reports = submit_market(&mut engine, 4, Side::Buy, 120);

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 4 },
            ExecutionReport::PartiallyFilled {
                order_id: 4,
                qty: Qty(50),
                remaining: Qty(70),
                price: PriceTicks(1000),
            },
            ExecutionReport::PartiallyFilled {
                order_id: 4,
                qty: Qty(50),
                remaining: Qty(20),
                price: PriceTicks(1001),
            },
            ExecutionReport::Filled {
                order_id: 4,
                qty: Qty(20),
                price: PriceTicks(1002),
            },
        ]
    );
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(engine.book().get_order(2), None);
    assert_eq!(
        engine.book().get_order(3).map(|order| order.qty),
        Some(Qty(30))
    );
    let snapshot = engine.book().snapshot(usize::MAX);
    assert_eq!(snapshot.asks.len(), 1);
    assert_eq!(snapshot.asks[0].price, PriceTicks(1002));
    assert_eq!(snapshot.asks[0].total_qty, Qty(30));
    assert_valid(&engine);
}

#[test]
fn cancel_before_match() {
    let mut engine = MatchingEngine::new(symbol());
    submit_limit(&mut engine, 1, Side::Sell, 1000, 100);

    assert_eq!(
        cancel(&mut engine, 1),
        vec![ExecutionReport::Cancelled { order_id: 1 }]
    );
    assert_valid(&engine);

    assert_eq!(
        submit_market(&mut engine, 2, Side::Buy, 40),
        vec![ExecutionReport::Rejected {
            order_id: 2,
            reason: RejectReason::EmptyBook,
        }]
    );
    assert!(engine.book().snapshot(usize::MAX).asks.is_empty());
    assert_valid(&engine);
}

#[test]
fn partial_fill_then_cancel() {
    let mut engine = MatchingEngine::new(symbol());
    submit_limit(&mut engine, 1, Side::Sell, 1000, 100);

    assert_eq!(
        submit_market(&mut engine, 2, Side::Buy, 40),
        vec![
            ExecutionReport::Accepted { order_id: 2 },
            ExecutionReport::Filled {
                order_id: 2,
                qty: Qty(40),
                price: PriceTicks(1000),
            },
        ]
    );
    assert_eq!(engine.book().get_order(1).unwrap().qty, Qty(60));
    assert_valid(&engine);

    assert_eq!(
        cancel(&mut engine, 1),
        vec![ExecutionReport::Cancelled { order_id: 1 }]
    );
    assert_eq!(engine.book().get_order(1), None);
    assert!(engine.book().snapshot(usize::MAX).asks.is_empty());
    assert_valid(&engine);
}

#[test]
fn same_price_fifo() {
    let mut engine = MatchingEngine::new(symbol());
    submit_limit(&mut engine, 1, Side::Sell, 1000, 50);
    submit_limit(&mut engine, 2, Side::Sell, 1000, 50);
    assert_eq!(engine.book().snapshot(1).asks[0].order_ids, vec![1, 2]);
    assert_valid(&engine);

    let reports = submit_market(&mut engine, 3, Side::Buy, 75);

    assert_eq!(
        reports,
        vec![
            ExecutionReport::Accepted { order_id: 3 },
            ExecutionReport::PartiallyFilled {
                order_id: 3,
                qty: Qty(50),
                remaining: Qty(25),
                price: PriceTicks(1000),
            },
            ExecutionReport::Filled {
                order_id: 3,
                qty: Qty(25),
                price: PriceTicks(1000),
            },
        ]
    );
    assert_eq!(engine.book().get_order(1), None);
    assert_eq!(
        engine.book().get_order(2).map(|order| order.qty),
        Some(Qty(25))
    );
    let snapshot = engine.book().snapshot(1);
    assert_eq!(snapshot.asks[0].order_ids, vec![2]);
    assert_eq!(snapshot.asks[0].total_qty, Qty(25));
    assert_valid(&engine);
}

#[test]
fn mixed_side_limit_cancel_and_market_flow() {
    let mut engine = MatchingEngine::new(symbol());
    assert_eq!(
        submit_limit(&mut engine, 1, Side::Buy, 1000, 50),
        vec![
            ExecutionReport::Accepted { order_id: 1 },
            ExecutionReport::Rested {
                order_id: 1,
                remaining: Qty(50),
            },
        ]
    );
    assert_eq!(
        submit_limit(&mut engine, 2, Side::Sell, 1010, 40),
        vec![
            ExecutionReport::Accepted { order_id: 2 },
            ExecutionReport::Rested {
                order_id: 2,
                remaining: Qty(40),
            },
        ]
    );
    assert_valid(&engine);

    assert_eq!(
        submit_limit(&mut engine, 3, Side::Sell, 990, 20),
        vec![
            ExecutionReport::Accepted { order_id: 3 },
            ExecutionReport::Filled {
                order_id: 3,
                qty: Qty(20),
                price: PriceTicks(1000),
            },
        ]
    );
    assert_eq!(engine.book().get_order(1).unwrap().qty, Qty(30));
    assert_valid(&engine);

    assert_eq!(
        cancel(&mut engine, 2),
        vec![ExecutionReport::Cancelled { order_id: 2 }]
    );
    assert_valid(&engine);

    assert_eq!(
        submit_market(&mut engine, 4, Side::Sell, 25),
        vec![
            ExecutionReport::Accepted { order_id: 4 },
            ExecutionReport::Filled {
                order_id: 4,
                qty: Qty(25),
                price: PriceTicks(1000),
            },
        ]
    );
    assert_eq!(engine.book().get_order(2), None);
    assert_eq!(
        engine.book().get_order(1).map(|order| order.qty),
        Some(Qty(5))
    );
    let snapshot = engine.book().snapshot(usize::MAX);
    assert_eq!(snapshot.bids.len(), 1);
    assert_eq!(snapshot.bids[0].price, PriceTicks(1000));
    assert_eq!(snapshot.bids[0].total_qty, Qty(5));
    assert!(snapshot.asks.is_empty());
    assert_valid(&engine);
}
