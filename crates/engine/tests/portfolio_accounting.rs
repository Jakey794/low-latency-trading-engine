//! Integration tests for portfolio accounting against matching-engine fills.

use engine::{
    events::{ExecutionReport, InputEvent, NewOrderEvent},
    matching::MatchingEngine,
    portfolio::Portfolio,
    types::{Order, OrderType, PriceTicks, Qty, Side, Symbol},
};

fn symbol() -> Symbol {
    Symbol("AAPL".to_owned())
}

fn limit_order(id: u64, side: Side, price: i64, qty: u64) -> Order {
    Order {
        order_id: id,
        symbol: symbol(),
        side,
        order_type: OrderType::Limit,
        price: Some(PriceTicks(price)),
        qty: Qty(qty),
        timestamp_ns: id,
        strategy_id: None,
    }
}

fn apply_taker_fills(
    portfolio: &mut Portfolio,
    symbol: &Symbol,
    side: Side,
    reports: &[ExecutionReport],
) {
    for report in reports {
        match report {
            ExecutionReport::Filled { qty, price, .. }
            | ExecutionReport::PartiallyFilled { qty, price, .. } => {
                portfolio
                    .apply_fill(symbol, side, *price, *qty)
                    .expect("fill should apply");
            }
            _ => {}
        }
    }
}

#[test]
fn portfolio_tracks_taker_fills_from_matching_engine() {
    let mut engine = MatchingEngine::new(symbol());
    let mut portfolio = Portfolio::new(1_000_000);

    // Resting ask (external liquidity); portfolio will be the aggressive buyer.
    engine.process_event(InputEvent::NewOrder(NewOrderEvent {
        seq: 1,
        order: limit_order(1, Side::Sell, 100, 10),
    }));

    let reports = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
        seq: 2,
        order: limit_order(2, Side::Buy, 100, 10),
    }));

    assert!(reports.iter().any(|r| matches!(
        r,
        ExecutionReport::Filled { .. } | ExecutionReport::PartiallyFilled { .. }
    )));

    apply_taker_fills(&mut portfolio, &symbol(), Side::Buy, &reports);

    assert_eq!(portfolio.position_qty(&symbol()), 10);
    assert_eq!(portfolio.realized_pnl(), 0);
    assert_eq!(portfolio.cash(), 1_000_000 - 1_000);

    portfolio.set_mark(&symbol(), PriceTicks(105)).unwrap();
    assert_eq!(portfolio.unrealized_pnl().unwrap(), 50);
    assert_eq!(portfolio.equity().unwrap(), 1_000_000 - 1_000 + 1_050);
}

#[test]
fn multi_symbol_portfolios_do_not_leak() {
    let aapl = Symbol("AAPL".to_owned());
    let msft = Symbol("MSFT".to_owned());
    let mut portfolio = Portfolio::new(5_000_000);

    portfolio
        .apply_fill(&aapl, Side::Buy, PriceTicks(100), Qty(10))
        .unwrap();
    portfolio
        .apply_fill(&msft, Side::Sell, PriceTicks(200), Qty(3))
        .unwrap();

    let snap = portfolio.snapshot().unwrap();
    assert_eq!(snap.positions.len(), 2);
    assert_eq!(snap.positions[0].symbol, aapl);
    assert_eq!(snap.positions[1].symbol, msft);
    assert_eq!(portfolio.position_qty(&aapl), 10);
    assert_eq!(portfolio.position_qty(&msft), -3);
}

#[test]
fn closing_through_engine_realizes_pnl() {
    let mut engine = MatchingEngine::new(symbol());
    let mut portfolio = Portfolio::new(1_000_000);

    // Build long via buy against resting sell.
    engine.process_event(InputEvent::NewOrder(NewOrderEvent {
        seq: 1,
        order: limit_order(1, Side::Sell, 100, 10),
    }));
    let buy_reports = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
        seq: 2,
        order: limit_order(2, Side::Buy, 100, 10),
    }));
    apply_taker_fills(&mut portfolio, &symbol(), Side::Buy, &buy_reports);

    // Close long via sell against resting buy.
    engine.process_event(InputEvent::NewOrder(NewOrderEvent {
        seq: 3,
        order: limit_order(3, Side::Buy, 120, 10),
    }));
    let sell_reports = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
        seq: 4,
        order: limit_order(4, Side::Sell, 120, 10),
    }));
    apply_taker_fills(&mut portfolio, &symbol(), Side::Sell, &sell_reports);

    assert_eq!(portfolio.position_qty(&symbol()), 0);
    assert_eq!(portfolio.realized_pnl(), 10 * (120 - 100));
    assert_eq!(portfolio.cash(), 1_000_000 + 200);
}
