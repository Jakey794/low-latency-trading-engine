use engine::book::{BookError, OrderBook};
use engine::types::{Order, OrderId, OrderType, PriceTicks, Qty, Side, Symbol};

fn limit_order(order_id: OrderId, side: Side, price: i64, qty: u64) -> Order {
    Order {
        order_id,
        symbol: Symbol("AAPL".to_owned()),
        side,
        order_type: OrderType::Limit,
        price: Some(PriceTicks(price)),
        qty: Qty(qty),
        timestamp_ns: order_id,
        strategy_id: None,
    }
}

#[test]
fn order_book_core_scenario() {
    let mut book = OrderBook::new(Symbol("AAPL".to_owned()));

    book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
        .unwrap();
    book.add_limit_order(limit_order(2, Side::Buy, 101, 20))
        .unwrap();
    book.add_limit_order(limit_order(3, Side::Buy, 101, 30))
        .unwrap();
    book.add_limit_order(limit_order(4, Side::Sell, 103, 40))
        .unwrap();
    book.add_limit_order(limit_order(5, Side::Sell, 102, 50))
        .unwrap();
    book.add_limit_order(limit_order(6, Side::Sell, 104, 60))
        .unwrap();

    assert_eq!(book.best_bid(), Some(PriceTicks(101)));
    assert_eq!(book.best_ask(), Some(PriceTicks(102)));
    assert_eq!(book.get_order(2).map(|order| order.order_id), Some(2));
    assert_eq!(book.get_order(3).map(|order| order.order_id), Some(3));

    let snapshot = book.snapshot(2);
    let bid_prices: Vec<_> = snapshot.bids.iter().map(|level| level.price).collect();
    let ask_prices: Vec<_> = snapshot.asks.iter().map(|level| level.price).collect();

    assert_eq!(bid_prices, vec![PriceTicks(101), PriceTicks(100)]);
    assert_eq!(ask_prices, vec![PriceTicks(102), PriceTicks(103)]);
    assert_eq!(snapshot.bids[0].order_ids, vec![2, 3]);
    assert_eq!(snapshot.bids[0].order_count, 2);
    assert_eq!(snapshot.bids[0].total_qty, Qty(50));

    let before_duplicate = book.snapshot(usize::MAX);
    let result = book.add_limit_order(limit_order(2, Side::Sell, 105, 70));

    assert_eq!(result, Err(BookError::DuplicateOrderId(2)));
    assert_eq!(book.snapshot(usize::MAX), before_duplicate);
}
