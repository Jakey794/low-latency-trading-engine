use std::collections::HashSet;

use crate::{
    book::{BookError, OrderBook},
    events::{ExecutionReport, InputEvent, RejectReason},
    types::{Order, OrderId, OrderType, PriceTicks, Qty, Side, Symbol},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchingEngine {
    book: OrderBook,
    filled_order_ids: HashSet<OrderId>,
    cancelled_order_ids: HashSet<OrderId>,
}

impl MatchingEngine {
    pub fn new(symbol: Symbol) -> Self {
        Self {
            book: OrderBook::new(symbol),
            filled_order_ids: HashSet::new(),
            cancelled_order_ids: HashSet::new(),
        }
    }

    pub fn process_event(&mut self, event: InputEvent) -> Vec<ExecutionReport> {
        match event {
            InputEvent::NewOrder(event) => self.process_new_order(event.order),
            InputEvent::Cancel(event) => self.process_cancel(event.order_id, &event.symbol),
        }
    }

    pub fn submit_limit_order(&mut self, order: Order) -> Vec<ExecutionReport> {
        self.submit_limit_order_inner(order, true)
    }

    fn process_new_order(&mut self, order: Order) -> Vec<ExecutionReport> {
        if order.qty == Qty(0) {
            return Self::rejected(order.order_id, RejectReason::InvalidQuantity);
        }
        if order.order_type == OrderType::Market {
            return self.submit_market_order(order);
        }

        self.submit_limit_order_inner(order, false)
    }

    fn submit_market_order(&mut self, order: Order) -> Vec<ExecutionReport> {
        let order_id = order.order_id;

        if self.book.check_matching_invariants().is_err() {
            return Self::rejected(order_id, RejectReason::InternalBookInvariantViolation);
        }
        if self.filled_order_ids.contains(&order_id) {
            return Self::rejected(order_id, RejectReason::AlreadyFilled);
        }
        if self.cancelled_order_ids.contains(&order_id) {
            return Self::rejected(order_id, RejectReason::AlreadyCancelled);
        }
        if self.book.get_order(order_id).is_some() || order.symbol != *self.book.symbol() {
            return Self::rejected(order_id, RejectReason::InvalidOrder);
        }
        if order.price.is_some() {
            return Self::rejected(order_id, RejectReason::InvalidPrice);
        }
        if self.best_opposite_price(order.side).is_none() {
            return Self::rejected(order_id, RejectReason::EmptyBook);
        }

        let mut remaining = order.qty;
        let mut reports = vec![ExecutionReport::Accepted { order_id }];

        while remaining != Qty(0) && self.best_opposite_price(order.side).is_some() {
            let (fill_qty, resting_price) = self.execute_at_best(order.side, remaining);
            remaining = Qty(remaining.0 - fill_qty.0);
            Self::push_fill_report(&mut reports, order_id, fill_qty, remaining, resting_price);
        }

        if remaining == Qty(0) {
            self.filled_order_ids.insert(order_id);
        } else {
            reports.push(ExecutionReport::Rejected {
                order_id,
                reason: RejectReason::MarketOrderWouldNotFill,
            });
        }

        debug_assert!(self.book.check_matching_invariants().is_ok());
        reports
    }

    fn submit_limit_order_inner(
        &mut self,
        mut order: Order,
        preserve_legacy_market_rejection: bool,
    ) -> Vec<ExecutionReport> {
        let order_id = order.order_id;
        let mut remaining = order.qty;

        if self.book.check_matching_invariants().is_err() {
            return Self::rejected(order_id, RejectReason::InternalBookInvariantViolation);
        }
        if self.filled_order_ids.contains(&order_id) {
            return Self::rejected(order_id, RejectReason::AlreadyFilled);
        }
        if self.cancelled_order_ids.contains(&order_id) {
            return Self::rejected(order_id, RejectReason::AlreadyCancelled);
        }

        let price = match self.book.validate_limit_order(&order) {
            Ok(price) => price,
            Err(error) => {
                let reason = if preserve_legacy_market_rejection
                    && matches!(error, BookError::NotLimitOrder(_))
                {
                    RejectReason::InvalidOrder
                } else {
                    Self::reject_reason_for_book_error(error)
                };
                return Self::rejected(order_id, reason);
            }
        };

        let mut reports = vec![ExecutionReport::Accepted { order_id }];

        while remaining != Qty(0) {
            if self.best_opposite_price(order.side).is_none() {
                break;
            }
            if !self.can_cross(order.side, price) {
                break;
            }

            let (fill_qty, resting_price) = self.execute_at_best(order.side, remaining);
            remaining = Qty(remaining.0 - fill_qty.0);
            Self::push_fill_report(&mut reports, order_id, fill_qty, remaining, resting_price);
        }

        if remaining != Qty(0) {
            order.qty = remaining;
            self.book
                .add_limit_order(order)
                .expect("validated residual order must be able to rest");
            reports.push(ExecutionReport::Rested {
                order_id,
                remaining,
            });
        } else {
            self.filled_order_ids.insert(order_id);
        }

        debug_assert!(self.book.check_matching_invariants().is_ok());

        reports
    }

    fn execute_at_best(&mut self, incoming_side: Side, incoming_qty: Qty) -> (Qty, PriceTicks) {
        let (resting_qty, resting_price) = {
            let resting_order = self
                .book
                .best_opposite_order(incoming_side)
                .expect("execution requires opposing liquidity");
            (resting_order.qty, resting_order.price)
        };
        let fill_qty = Qty(incoming_qty.0.min(resting_qty.0));

        if fill_qty == resting_qty {
            let removed_order = self
                .book
                .remove_best_opposite(incoming_side)
                .expect("best opposing order must be removable");
            debug_assert_eq!(removed_order.qty, fill_qty);
            debug_assert_eq!(removed_order.price, resting_price);
            self.filled_order_ids.insert(removed_order.order_id);
        } else {
            let updated_order = self
                .book
                .reduce_best_opposite_qty(incoming_side, fill_qty)
                .expect("larger resting order must be reducible");
            debug_assert_eq!(updated_order.price, resting_price);
            debug_assert_eq!(updated_order.qty.0, resting_qty.0 - fill_qty.0);
        }

        (fill_qty, resting_price)
    }

    fn push_fill_report(
        reports: &mut Vec<ExecutionReport>,
        order_id: OrderId,
        fill_qty: Qty,
        remaining: Qty,
        price: PriceTicks,
    ) {
        if remaining == Qty(0) {
            reports.push(ExecutionReport::Filled {
                order_id,
                qty: fill_qty,
                price,
            });
        } else {
            reports.push(ExecutionReport::PartiallyFilled {
                order_id,
                qty: fill_qty,
                remaining,
                price,
            });
        }
    }

    fn process_cancel(&mut self, order_id: OrderId, symbol: &Symbol) -> Vec<ExecutionReport> {
        if self.filled_order_ids.contains(&order_id) {
            return Self::rejected(order_id, RejectReason::AlreadyFilled);
        }
        if self.cancelled_order_ids.contains(&order_id) {
            return Self::rejected(order_id, RejectReason::AlreadyCancelled);
        }
        if self.book.best_bid().is_none() && self.book.best_ask().is_none() {
            return Self::rejected(order_id, RejectReason::EmptyBook);
        }
        if self.book.symbol() != symbol || self.book.get_order(order_id).is_none() {
            return Self::rejected(order_id, RejectReason::UnknownOrder);
        }

        match self.book.cancel_order(order_id) {
            Ok(_) => {
                self.cancelled_order_ids.insert(order_id);
                vec![ExecutionReport::Cancelled { order_id }]
            }
            Err(_) => Self::rejected(order_id, RejectReason::InternalBookInvariantViolation),
        }
    }

    fn reject_reason_for_book_error(error: BookError) -> RejectReason {
        match error {
            BookError::InvalidQuantity(_) | BookError::QuantityOverflow(_) => {
                RejectReason::InvalidQuantity
            }
            BookError::MissingPrice(_) | BookError::InvalidPrice(_) => RejectReason::InvalidPrice,
            BookError::NotLimitOrder(_) => RejectReason::MarketOrderWouldNotFill,
            BookError::UnknownOrder(_) => RejectReason::UnknownOrder,
            BookError::DuplicateOrderId(_) | BookError::SymbolMismatch { .. } => {
                RejectReason::InvalidOrder
            }
        }
    }

    fn rejected(order_id: OrderId, reason: RejectReason) -> Vec<ExecutionReport> {
        vec![ExecutionReport::Rejected { order_id, reason }]
    }

    pub fn book(&self) -> &OrderBook {
        &self.book
    }

    fn best_opposite_price(&self, side: Side) -> Option<PriceTicks> {
        match side {
            Side::Buy => self.book.best_ask(),
            Side::Sell => self.book.best_bid(),
        }
    }

    fn can_cross(&self, side: Side, limit_price: PriceTicks) -> bool {
        self.best_opposite_price(side)
            .is_some_and(|opposite_price| Self::prices_cross(side, limit_price, opposite_price))
    }

    fn prices_cross(side: Side, limit_price: PriceTicks, opposite_price: PriceTicks) -> bool {
        match side {
            Side::Buy => limit_price >= opposite_price,
            Side::Sell => limit_price <= opposite_price,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::events::{CancelOrderEvent, NewOrderEvent};
    use crate::types::{OrderId, OrderType, Qty};

    use super::*;

    fn symbol() -> Symbol {
        Symbol("AAPL".to_owned())
    }

    fn order(order_id: OrderId, side: Side, price: Option<i64>, qty: u64) -> Order {
        Order {
            order_id,
            symbol: symbol(),
            side,
            order_type: OrderType::Limit,
            price: price.map(PriceTicks),
            qty: Qty(qty),
            timestamp_ns: order_id,
            strategy_id: None,
        }
    }

    fn engine_with_ask(price: i64) -> MatchingEngine {
        let mut engine = MatchingEngine::new(symbol());
        engine.submit_limit_order(order(1, Side::Sell, Some(price), 10));
        engine
    }

    fn engine_with_bid(price: i64) -> MatchingEngine {
        let mut engine = MatchingEngine::new(symbol());
        engine.submit_limit_order(order(1, Side::Buy, Some(price), 10));
        engine
    }

    #[test]
    fn buy_without_ask_does_not_cross() {
        let engine = MatchingEngine::new(symbol());

        assert!(!engine.can_cross(Side::Buy, PriceTicks(100)));
    }

    #[test]
    fn sell_without_bid_does_not_cross() {
        let engine = MatchingEngine::new(symbol());

        assert!(!engine.can_cross(Side::Sell, PriceTicks(100)));
    }

    #[test]
    fn buy_below_ask_does_not_cross() {
        let engine = engine_with_ask(100);

        assert!(!engine.can_cross(Side::Buy, PriceTicks(99)));
    }

    #[test]
    fn buy_at_ask_crosses() {
        let engine = engine_with_ask(100);

        assert!(engine.can_cross(Side::Buy, PriceTicks(100)));
    }

    #[test]
    fn buy_above_ask_crosses() {
        let engine = engine_with_ask(100);

        assert!(engine.can_cross(Side::Buy, PriceTicks(101)));
    }

    #[test]
    fn sell_above_bid_does_not_cross() {
        let engine = engine_with_bid(100);

        assert!(!engine.can_cross(Side::Sell, PriceTicks(101)));
    }

    #[test]
    fn sell_at_bid_crosses() {
        let engine = engine_with_bid(100);

        assert!(engine.can_cross(Side::Sell, PriceTicks(100)));
    }

    #[test]
    fn sell_below_bid_crosses() {
        let engine = engine_with_bid(100);

        assert!(engine.can_cross(Side::Sell, PriceTicks(99)));
    }

    #[test]
    fn buy_larger_than_available_book_rests_residual() {
        let mut engine = MatchingEngine::new(symbol());
        engine.submit_limit_order(order(1, Side::Sell, Some(100), 10));

        let reports = engine.submit_limit_order(order(2, Side::Buy, Some(100), 15));

        assert_eq!(
            reports,
            vec![
                ExecutionReport::Accepted { order_id: 2 },
                ExecutionReport::PartiallyFilled {
                    order_id: 2,
                    qty: Qty(10),
                    remaining: Qty(5),
                    price: PriceTicks(100),
                },
                ExecutionReport::Rested {
                    order_id: 2,
                    remaining: Qty(5),
                },
            ]
        );
        assert_eq!(engine.book().get_order(1), None);
        assert_eq!(
            engine.book().get_order(2).map(|order| order.qty),
            Some(Qty(5))
        );
    }

    #[test]
    fn sell_larger_than_available_book_rests_residual() {
        let mut engine = MatchingEngine::new(symbol());
        engine.submit_limit_order(order(1, Side::Buy, Some(100), 10));

        let reports = engine.submit_limit_order(order(2, Side::Sell, Some(100), 15));

        assert_eq!(
            reports,
            vec![
                ExecutionReport::Accepted { order_id: 2 },
                ExecutionReport::PartiallyFilled {
                    order_id: 2,
                    qty: Qty(10),
                    remaining: Qty(5),
                    price: PriceTicks(100),
                },
                ExecutionReport::Rested {
                    order_id: 2,
                    remaining: Qty(5),
                },
            ]
        );
        assert_eq!(engine.book().get_order(1), None);
        assert_eq!(
            engine.book().get_order(2).map(|order| order.qty),
            Some(Qty(5))
        );
    }

    #[test]
    fn market_order_is_rejected_without_mutating_book() {
        let mut engine = MatchingEngine::new(symbol());
        let mut market_order = order(1, Side::Buy, None, 10);
        market_order.order_type = OrderType::Market;

        let reports = engine.submit_limit_order(market_order);

        assert_eq!(
            reports,
            vec![ExecutionReport::Rejected {
                order_id: 1,
                reason: RejectReason::InvalidOrder,
            }]
        );
        assert_eq!(engine.book().snapshot(usize::MAX).bids, vec![]);
        assert_eq!(engine.book().snapshot(usize::MAX).asks, vec![]);
    }

    #[test]
    fn process_event_accepts_new_limit_order() {
        let mut engine = MatchingEngine::new(symbol());

        let reports = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
            seq: 1,
            order: order(10, Side::Buy, Some(100), 5),
        }));

        assert_eq!(
            reports,
            vec![
                ExecutionReport::Accepted { order_id: 10 },
                ExecutionReport::Rested {
                    order_id: 10,
                    remaining: Qty(5),
                },
            ]
        );
    }

    #[test]
    fn process_event_rejects_invalid_quantity_and_price() {
        let mut engine = MatchingEngine::new(symbol());

        let zero_qty = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
            seq: 1,
            order: order(10, Side::Buy, Some(100), 0),
        }));
        let missing_price = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
            seq: 2,
            order: order(11, Side::Buy, None, 5),
        }));
        let non_positive_price = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
            seq: 3,
            order: order(12, Side::Buy, Some(0), 5),
        }));

        assert_eq!(
            zero_qty,
            vec![ExecutionReport::Rejected {
                order_id: 10,
                reason: RejectReason::InvalidQuantity,
            }]
        );
        for (reports, order_id) in [(missing_price, 11), (non_positive_price, 12)] {
            assert_eq!(
                reports,
                vec![ExecutionReport::Rejected {
                    order_id,
                    reason: RejectReason::InvalidPrice,
                }]
            );
        }
        assert!(engine.book().snapshot(usize::MAX).bids.is_empty());
    }

    #[test]
    fn process_event_rejects_market_order_when_opposite_side_is_empty() {
        let mut engine = MatchingEngine::new(symbol());
        let mut market_order = order(10, Side::Buy, None, 5);
        market_order.order_type = OrderType::Market;

        let reports = engine.process_event(InputEvent::NewOrder(NewOrderEvent {
            seq: 1,
            order: market_order,
        }));

        assert_eq!(
            reports,
            vec![ExecutionReport::Rejected {
                order_id: 10,
                reason: RejectReason::EmptyBook,
            }]
        );
    }

    #[test]
    fn cancel_event_removes_resting_order_and_rejects_repeat() {
        let mut engine = MatchingEngine::new(symbol());
        engine.submit_limit_order(order(10, Side::Buy, Some(100), 5));
        let cancel = || {
            InputEvent::Cancel(CancelOrderEvent {
                seq: 2,
                order_id: 10,
                symbol: symbol(),
                timestamp_ns: 2,
            })
        };

        let cancelled = engine.process_event(cancel());
        let repeated = engine.process_event(cancel());

        assert_eq!(cancelled, vec![ExecutionReport::Cancelled { order_id: 10 }]);
        assert_eq!(
            repeated,
            vec![ExecutionReport::Rejected {
                order_id: 10,
                reason: RejectReason::AlreadyCancelled,
            }]
        );
        assert_eq!(engine.book().get_order(10), None);
    }

    #[test]
    fn cancel_rejects_empty_book_unknown_order_and_filled_order() {
        let cancel = |order_id| {
            InputEvent::Cancel(CancelOrderEvent {
                seq: 3,
                order_id,
                symbol: symbol(),
                timestamp_ns: 3,
            })
        };
        let mut empty_engine = MatchingEngine::new(symbol());
        assert_eq!(
            empty_engine.process_event(cancel(99)),
            vec![ExecutionReport::Rejected {
                order_id: 99,
                reason: RejectReason::EmptyBook,
            }]
        );

        let mut engine = MatchingEngine::new(symbol());
        engine.submit_limit_order(order(1, Side::Sell, Some(100), 5));
        assert_eq!(
            engine.process_event(cancel(99)),
            vec![ExecutionReport::Rejected {
                order_id: 99,
                reason: RejectReason::UnknownOrder,
            }]
        );
        engine.submit_limit_order(order(2, Side::Buy, Some(100), 5));
        assert_eq!(
            engine.process_event(cancel(2)),
            vec![ExecutionReport::Rejected {
                order_id: 2,
                reason: RejectReason::AlreadyFilled,
            }]
        );
    }
}
