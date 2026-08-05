//! Integration tests for risk checks wrapping the matching engine.

use engine::{
    events::{ExecutionReport, InputEvent, NewOrderEvent},
    matching::MatchingEngine,
    portfolio::Portfolio,
    risk::{RiskDecision, RiskLimits, RiskManager, RiskRejectReason},
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

/// Submit only if risk allows; otherwise leave book and portfolio untouched.
fn submit_with_risk(
    risk: &RiskManager,
    engine: &mut MatchingEngine,
    portfolio: &Portfolio,
    order: Order,
) -> Result<Vec<ExecutionReport>, RiskRejectReason> {
    match risk.check_new_order(&order, portfolio) {
        RiskDecision::Allow => Ok(engine.process_event(InputEvent::NewOrder(NewOrderEvent {
            seq: order.order_id,
            order,
        }))),
        RiskDecision::Reject { reason } => Err(reason),
    }
}

#[test]
fn rejected_order_does_not_mutate_book_or_portfolio() {
    let limits = RiskLimits {
        max_order_qty: Some(5),
        ..RiskLimits::default()
    };
    let risk = RiskManager::new(limits, 1_000_000);
    let mut engine = MatchingEngine::new(symbol());
    let portfolio = Portfolio::new(1_000_000);

    let err = submit_with_risk(
        &risk,
        &mut engine,
        &portfolio,
        limit_order(1, Side::Buy, 100, 6),
    )
    .unwrap_err();
    assert!(matches!(err, RiskRejectReason::MaxOrderQty { .. }));
    assert!(engine.book().get_order(1).is_none());
    assert_eq!(portfolio.position_qty(&symbol()), 0);
    assert_eq!(portfolio.cash(), 1_000_000);
}

#[test]
fn cancel_allowed_after_kill_switch() {
    let mut risk = RiskManager::new(RiskLimits::default(), 1_000_000);
    let mut engine = MatchingEngine::new(symbol());
    let portfolio = Portfolio::new(1_000_000);

    submit_with_risk(
        &risk,
        &mut engine,
        &portfolio,
        limit_order(1, Side::Buy, 100, 5),
    )
    .unwrap();
    assert!(engine.book().get_order(1).is_some());

    risk.activate_kill_switch("test");
    assert!(risk.allow_cancel());

    let reports = engine.process_event(InputEvent::Cancel(engine::events::CancelOrderEvent {
        seq: 2,
        order_id: 1,
        symbol: symbol(),
        timestamp_ns: 2,
    }));
    assert!(matches!(
        reports.as_slice(),
        [ExecutionReport::Cancelled { order_id: 1 }]
    ));
    assert!(engine.book().get_order(1).is_none());
}

#[test]
fn post_trade_loss_trips_kill_and_blocks_new_orders() {
    let limits = RiskLimits {
        max_total_loss: Some(50),
        ..RiskLimits::default()
    };
    let mut risk = RiskManager::new(limits, 10_000);
    let mut engine = MatchingEngine::new(symbol());
    let mut portfolio = Portfolio::new(10_000);

    // Resting sell; buy fills at 100 for qty 10 → cash 9000, mark later at 90 → loss 100
    engine.process_event(InputEvent::NewOrder(NewOrderEvent {
        seq: 1,
        order: limit_order(1, Side::Sell, 100, 10),
    }));
    let reports = submit_with_risk(
        &risk,
        &mut engine,
        &portfolio,
        limit_order(2, Side::Buy, 100, 10),
    )
    .unwrap();

    for report in &reports {
        if let ExecutionReport::Filled { qty, price, .. }
        | ExecutionReport::PartiallyFilled { qty, price, .. } = report
        {
            portfolio
                .apply_fill(&symbol(), Side::Buy, *price, *qty)
                .unwrap();
        }
    }
    portfolio.set_mark(&symbol(), PriceTicks(90)).unwrap();
    assert!(risk.check_post_trade_loss(&portfolio).unwrap());
    assert!(risk.is_killed());

    let err = submit_with_risk(
        &risk,
        &mut engine,
        &portfolio,
        limit_order(3, Side::Buy, 90, 1),
    )
    .unwrap_err();
    assert_eq!(err, RiskRejectReason::KillSwitchActive);
    assert!(engine.book().get_order(3).is_none());
}

#[test]
fn multi_symbol_aggregate_position_limit() {
    let limits = RiskLimits {
        max_abs_position: Some(10),
        ..RiskLimits::default()
    };
    let risk = RiskManager::new(limits, 1_000_000);
    let mut portfolio = Portfolio::new(1_000_000);
    portfolio
        .apply_fill(&Symbol("MSFT".into()), Side::Buy, PriceTicks(100), Qty(10))
        .unwrap();

    // Global max abs position is per projected symbol position, not gross across symbols.
    // MSFT is at 10; AAPL order for 10 is still allowed on AAPL.
    let aapl = Order {
        order_id: 1,
        symbol: Symbol("AAPL".into()),
        side: Side::Buy,
        order_type: OrderType::Limit,
        price: Some(PriceTicks(100)),
        qty: Qty(10),
        timestamp_ns: 1,
        strategy_id: None,
    };
    assert!(risk.check_new_order(&aapl, &portfolio).is_allow());

    let over = Order {
        order_id: 2,
        symbol: Symbol("MSFT".into()),
        side: Side::Buy,
        order_type: OrderType::Limit,
        price: Some(PriceTicks(100)),
        qty: Qty(1),
        timestamp_ns: 2,
        strategy_id: None,
    };
    assert!(matches!(
        risk.check_new_order(&over, &portfolio),
        RiskDecision::Reject {
            reason: RiskRejectReason::MaxAbsPosition { .. }
        }
    ));
}

#[test]
fn one_unit_over_notional_rejected() {
    let limits = RiskLimits {
        max_gross_notional: Some(999),
        ..RiskLimits::default()
    };
    let risk = RiskManager::new(limits, 1_000_000);
    let portfolio = Portfolio::new(1_000_000);
    assert!(matches!(
        risk.check_new_order(&limit_order(1, Side::Buy, 100, 10), &portfolio),
        RiskDecision::Reject {
            reason: RiskRejectReason::MaxGrossNotional {
                notional: 1000,
                limit: 999
            }
        }
    ));
}
