use crate::{
    book::OrderBook,
    events::{ExecutionReport, RejectReason},
    types::{Order, PriceTicks, Qty, Side, Symbol},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchingEngine {
    book: OrderBook,
}

impl MatchingEngine {
    pub fn new(symbol: Symbol) -> Self {
        Self {
            book: OrderBook::new(symbol),
        }
    }

    pub fn submit_limit_order(&mut self, mut order: Order) -> Vec<ExecutionReport> {
        let order_id = order.order_id;
        let mut remaining = order.qty;
        let price = match self.book.validate_limit_order(&order) {
            Ok(price) => price,
            Err(_) => {
                return vec![ExecutionReport::Rejected {
                    order_id,
                    reason: RejectReason::InvalidOrder,
                }];
            }
        };

        let mut reports = vec![ExecutionReport::Accepted { order_id }];

        while remaining != Qty(0) {
            let Some(opposite_price) = self.best_opposite_price(order.side) else {
                break;
            };
            if !self.can_cross(order.side, price) {
                break;
            }

            let (resting_qty, resting_price) = {
                let resting_order = self
                    .book
                    .best_opposite_order(order.side)
                    .expect("crossing order must have opposing liquidity");
                (resting_order.qty, resting_order.price)
            };
            debug_assert_eq!(resting_price, opposite_price);

            let fill_qty = Qty(remaining.0.min(resting_qty.0));

            if fill_qty == resting_qty {
                let removed_order = self
                    .book
                    .remove_best_opposite(order.side)
                    .expect("best opposing order must be removable");
                debug_assert_eq!(removed_order.qty, fill_qty);
                debug_assert_eq!(removed_order.price, resting_price);
            } else {
                let updated_order = self
                    .book
                    .reduce_best_opposite_qty(order.side, fill_qty)
                    .expect("larger resting order must be reducible");
                debug_assert_eq!(updated_order.price, resting_price);
                debug_assert_eq!(updated_order.qty.0, resting_qty.0 - fill_qty.0);
            }

            remaining = Qty(remaining.0 - fill_qty.0);

            if remaining == Qty(0) {
                reports.push(ExecutionReport::Filled {
                    order_id,
                    qty: fill_qty,
                    price: resting_price,
                });
            } else {
                reports.push(ExecutionReport::PartiallyFilled {
                    order_id,
                    qty: fill_qty,
                    remaining,
                    price: resting_price,
                });
            }
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
        }

        debug_assert!(self.book.check_matching_invariants().is_ok());

        reports
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
}
