use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::types::{
    Order, OrderId, OrderType, PriceTicks, Qty, Side, StrategyId, Symbol, TimestampNanos,
};

use super::price_level::PriceLevel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestingOrder {
    pub order_id: OrderId,
    pub side: Side,
    pub price: PriceTicks,
    pub qty: Qty,
    pub timestamp_ns: TimestampNanos,
    pub strategy_id: Option<StrategyId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookSnapshot {
    pub symbol: Symbol,
    pub bids: Vec<LevelSnapshot>,
    pub asks: Vec<LevelSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LevelSnapshot {
    pub price: PriceTicks,
    pub total_qty: Qty,
    pub order_count: usize,
    pub order_ids: Vec<OrderId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OrderLocation {
    side: Side,
    price: PriceTicks,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BookError {
    #[error("duplicate order ID: {0}")]
    DuplicateOrderId(OrderId),
    #[error("invalid order quantity: {0:?}")]
    InvalidQuantity(Qty),
    #[error("aggregate quantity would overflow at price {0:?}")]
    QuantityOverflow(PriceTicks),
    #[error("order symbol {actual:?} does not match book symbol {expected:?}")]
    SymbolMismatch { expected: Symbol, actual: Symbol },
    #[error("order {0} is not a limit order")]
    NotLimitOrder(OrderId),
    #[error("limit order {0} is missing a price")]
    MissingPrice(OrderId),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum BookInvariantError {
    #[error("empty {side:?} price level at {price:?}")]
    EmptyPriceLevel { side: Side, price: PriceTicks },
    #[error("order {order_id} is stored on {expected:?} but has side {actual:?}")]
    WrongOrderSide {
        order_id: OrderId,
        expected: Side,
        actual: Side,
    },
    #[error("order {order_id} is stored at {level_price:?} but has price {order_price:?}")]
    WrongOrderPrice {
        order_id: OrderId,
        level_price: PriceTicks,
        order_price: PriceTicks,
    },
    #[error("resting order {0} has zero quantity")]
    ZeroQuantity(OrderId),
    #[error("aggregate quantity overflows for {side:?} price level {price:?}")]
    QuantityOverflow { side: Side, price: PriceTicks },
    #[error("resting order {0} has no order-index entry")]
    MissingIndexEntry(OrderId),
    #[error(
        "order {order_id} index location ({indexed_side:?}, {indexed_price:?}) does not match its resting location ({resting_side:?}, {resting_price:?})"
    )]
    IndexLocationMismatch {
        order_id: OrderId,
        indexed_side: Side,
        indexed_price: PriceTicks,
        resting_side: Side,
        resting_price: PriceTicks,
    },
    #[error("order-index entry {order_id} points to missing {side:?} order at {price:?}")]
    IndexPointsToMissingOrder {
        order_id: OrderId,
        side: Side,
        price: PriceTicks,
    },
    #[error("order-index count {indexed} does not match resting-order count {resting}")]
    OrderCountMismatch { indexed: usize, resting: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderBook {
    symbol: Symbol,
    bids: BTreeMap<PriceTicks, PriceLevel>,
    asks: BTreeMap<PriceTicks, PriceLevel>,
    order_index: HashMap<OrderId, OrderLocation>,
}

impl OrderBook {
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            order_index: HashMap::new(),
        }
    }

    pub fn add_limit_order(&mut self, order: Order) -> Result<(), BookError> {
        let price = self.validate_limit_order(&order)?;
        let order_id = order.order_id;
        let side = order.side;
        let levels = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };

        let resting_order = RestingOrder {
            order_id,
            side,
            price,
            qty: order.qty,
            timestamp_ns: order.timestamp_ns,
            strategy_id: order.strategy_id,
        };

        levels
            .entry(price)
            .or_insert_with(|| PriceLevel::new(price))
            .push_back(resting_order);

        self.order_index
            .insert(order_id, OrderLocation { side, price });

        debug_assert!(self.check_invariants().is_ok());

        Ok(())
    }

    pub(crate) fn validate_limit_order(&self, order: &Order) -> Result<PriceTicks, BookError> {
        if self.order_index.contains_key(&order.order_id) {
            return Err(BookError::DuplicateOrderId(order.order_id));
        }

        if order.qty == Qty(0) {
            return Err(BookError::InvalidQuantity(order.qty));
        }

        if order.symbol != self.symbol {
            return Err(BookError::SymbolMismatch {
                expected: self.symbol.clone(),
                actual: order.symbol.clone(),
            });
        }

        if order.order_type != OrderType::Limit {
            return Err(BookError::NotLimitOrder(order.order_id));
        }

        let price = order.price.ok_or(BookError::MissingPrice(order.order_id))?;
        let levels = match order.side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        };
        if levels
            .get(&price)
            .is_some_and(|level| level.total_qty().0.checked_add(order.qty.0).is_none())
        {
            return Err(BookError::QuantityOverflow(price));
        }

        Ok(price)
    }

    pub fn best_bid(&self) -> Option<PriceTicks> {
        self.bids.keys().next_back().copied()
    }

    pub fn best_ask(&self) -> Option<PriceTicks> {
        self.asks.keys().next().copied()
    }

    pub fn get_order(&self, order_id: OrderId) -> Option<&RestingOrder> {
        let location = self.order_index.get(&order_id)?;
        let levels = match location.side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        };

        levels.get(&location.price)?.get_order(order_id)
    }

    pub fn snapshot(&self, depth: usize) -> BookSnapshot {
        let bids = self
            .bids
            .iter()
            .rev()
            .take(depth)
            .map(|(price, level)| LevelSnapshot {
                price: *price,
                total_qty: level.total_qty(),
                order_count: level.len(),
                order_ids: level.iter().map(|order| order.order_id).collect(),
            })
            .collect();
        let asks = self
            .asks
            .iter()
            .take(depth)
            .map(|(price, level)| LevelSnapshot {
                price: *price,
                total_qty: level.total_qty(),
                order_count: level.len(),
                order_ids: level.iter().map(|order| order.order_id).collect(),
            })
            .collect();

        BookSnapshot {
            symbol: self.symbol.clone(),
            bids,
            asks,
        }
    }

    pub(crate) fn check_invariants(&self) -> Result<(), BookInvariantError> {
        let mut resting_count = 0;

        for (expected_side, levels) in [(Side::Buy, &self.bids), (Side::Sell, &self.asks)] {
            for (level_price, level) in levels {
                if level.is_empty() {
                    return Err(BookInvariantError::EmptyPriceLevel {
                        side: expected_side,
                        price: *level_price,
                    });
                }

                let mut level_qty = 0_u64;
                for order in level.iter() {
                    resting_count += 1;

                    if order.side != expected_side {
                        return Err(BookInvariantError::WrongOrderSide {
                            order_id: order.order_id,
                            expected: expected_side,
                            actual: order.side,
                        });
                    }
                    if order.price != *level_price {
                        return Err(BookInvariantError::WrongOrderPrice {
                            order_id: order.order_id,
                            level_price: *level_price,
                            order_price: order.price,
                        });
                    }
                    if order.qty == Qty(0) {
                        return Err(BookInvariantError::ZeroQuantity(order.order_id));
                    }
                    level_qty = level_qty.checked_add(order.qty.0).ok_or(
                        BookInvariantError::QuantityOverflow {
                            side: expected_side,
                            price: *level_price,
                        },
                    )?;

                    let location = self
                        .order_index
                        .get(&order.order_id)
                        .ok_or(BookInvariantError::MissingIndexEntry(order.order_id))?;
                    if location.side != expected_side || location.price != *level_price {
                        return Err(BookInvariantError::IndexLocationMismatch {
                            order_id: order.order_id,
                            indexed_side: location.side,
                            indexed_price: location.price,
                            resting_side: expected_side,
                            resting_price: *level_price,
                        });
                    }
                }
            }
        }

        for (order_id, location) in &self.order_index {
            let levels = match location.side {
                Side::Buy => &self.bids,
                Side::Sell => &self.asks,
            };
            let order_exists = levels
                .get(&location.price)
                .and_then(|level| level.get_order(*order_id))
                .is_some();
            if !order_exists {
                return Err(BookInvariantError::IndexPointsToMissingOrder {
                    order_id: *order_id,
                    side: location.side,
                    price: location.price,
                });
            }
        }

        if self.order_index.len() != resting_count {
            return Err(BookInvariantError::OrderCountMismatch {
                indexed: self.order_index.len(),
                resting: resting_count,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn resting_order(order_id: OrderId, side: Side, price: i64, qty: u64) -> RestingOrder {
        RestingOrder {
            order_id,
            side,
            price: PriceTicks(price),
            qty: Qty(qty),
            timestamp_ns: order_id,
            strategy_id: None,
        }
    }

    #[test]
    fn new_book_is_empty() {
        let book = OrderBook::new(symbol());

        assert!(book.bids.is_empty());
        assert!(book.asks.is_empty());
        assert!(book.order_index.is_empty());
    }

    #[test]
    fn empty_book_has_no_best_bid_or_ask() {
        let book = OrderBook::new(symbol());

        assert_eq!(book.best_bid(), None);
        assert_eq!(book.best_ask(), None);
    }

    #[test]
    fn add_one_bid_updates_best_bid() {
        let mut book = OrderBook::new(symbol());

        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();

        assert_eq!(book.best_bid(), Some(PriceTicks(100)));
    }

    #[test]
    fn add_one_ask_updates_best_ask() {
        let mut book = OrderBook::new(symbol());

        book.add_limit_order(limit_order(1, Side::Sell, 101, 10))
            .unwrap();

        assert_eq!(book.best_ask(), Some(PriceTicks(101)));
    }

    #[test]
    fn best_bid_is_highest_bid() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        book.add_limit_order(limit_order(2, Side::Buy, 102, 10))
            .unwrap();
        book.add_limit_order(limit_order(3, Side::Buy, 101, 10))
            .unwrap();

        assert_eq!(book.best_bid(), Some(PriceTicks(102)));
    }

    #[test]
    fn best_ask_is_lowest_ask() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Sell, 102, 10))
            .unwrap();
        book.add_limit_order(limit_order(2, Side::Sell, 100, 10))
            .unwrap();
        book.add_limit_order(limit_order(3, Side::Sell, 101, 10))
            .unwrap();

        assert_eq!(book.best_ask(), Some(PriceTicks(100)));
    }

    #[test]
    fn multiple_bids_update_best_bid() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 101, 10))
            .unwrap();
        assert_eq!(book.best_bid(), Some(PriceTicks(101)));

        book.add_limit_order(limit_order(2, Side::Buy, 103, 10))
            .unwrap();
        assert_eq!(book.best_bid(), Some(PriceTicks(103)));
    }

    #[test]
    fn multiple_asks_update_best_ask() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Sell, 103, 10))
            .unwrap();
        assert_eq!(book.best_ask(), Some(PriceTicks(103)));

        book.add_limit_order(limit_order(2, Side::Sell, 101, 10))
            .unwrap();
        assert_eq!(book.best_ask(), Some(PriceTicks(101)));
    }

    #[test]
    fn bid_and_ask_books_are_independent() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        book.add_limit_order(limit_order(2, Side::Sell, 105, 10))
            .unwrap();
        book.add_limit_order(limit_order(3, Side::Buy, 102, 10))
            .unwrap();
        book.add_limit_order(limit_order(4, Side::Sell, 103, 10))
            .unwrap();

        assert_eq!(book.best_bid(), Some(PriceTicks(102)));
        assert_eq!(book.best_ask(), Some(PriceTicks(103)));
    }

    #[test]
    fn order_index_tracks_added_bid() {
        let mut book = OrderBook::new(symbol());

        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();

        assert_eq!(
            book.order_index.get(&1),
            Some(&OrderLocation {
                side: Side::Buy,
                price: PriceTicks(100),
            })
        );
    }

    #[test]
    fn order_index_tracks_added_ask() {
        let mut book = OrderBook::new(symbol());

        book.add_limit_order(limit_order(2, Side::Sell, 105, 20))
            .unwrap();

        assert_eq!(
            book.order_index.get(&2),
            Some(&OrderLocation {
                side: Side::Sell,
                price: PriceTicks(105),
            })
        );
    }

    #[test]
    fn duplicate_order_id_is_rejected() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();

        let result = book.add_limit_order(limit_order(1, Side::Sell, 101, 20));

        assert_eq!(result, Err(BookError::DuplicateOrderId(1)));
    }

    #[test]
    fn rejected_duplicate_does_not_mutate_book() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        let before = book.clone();

        let result = book.add_limit_order(limit_order(1, Side::Sell, 101, 20));

        assert_eq!(result, Err(BookError::DuplicateOrderId(1)));
        assert_eq!(book, before);
    }

    #[test]
    fn rejected_invalid_quantity_does_not_mutate_book() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        let before = book.clone();

        let result = book.add_limit_order(limit_order(2, Side::Sell, 101, 0));

        assert_eq!(result, Err(BookError::InvalidQuantity(Qty(0))));
        assert_eq!(book, before);
    }

    #[test]
    fn rejected_quantity_overflow_does_not_mutate_book() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, u64::MAX))
            .unwrap();
        let before = book.clone();

        let result = book.add_limit_order(limit_order(2, Side::Buy, 100, 1));

        assert_eq!(result, Err(BookError::QuantityOverflow(PriceTicks(100))));
        assert_eq!(book, before);
        assert_eq!(book.snapshot(1).bids[0].total_qty, Qty(u64::MAX));
    }

    #[test]
    fn get_unknown_order_returns_none() {
        let book = OrderBook::new(symbol());

        assert_eq!(book.get_order(999), None);
    }

    #[test]
    fn get_order_returns_added_bid() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(7, Side::Buy, 100, 25))
            .unwrap();

        assert_eq!(
            book.get_order(7),
            Some(&resting_order(7, Side::Buy, 100, 25))
        );
    }

    #[test]
    fn get_order_returns_added_ask() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(8, Side::Sell, 105, 30))
            .unwrap();

        assert_eq!(
            book.get_order(8),
            Some(&resting_order(8, Side::Sell, 105, 30))
        );
    }

    #[test]
    fn snapshot_empty_book() {
        let book = OrderBook::new(symbol());

        assert_eq!(
            book.snapshot(5),
            BookSnapshot {
                symbol: symbol(),
                bids: vec![],
                asks: vec![],
            }
        );
    }

    #[test]
    fn snapshot_zero_depth() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        book.add_limit_order(limit_order(2, Side::Sell, 101, 20))
            .unwrap();

        let snapshot = book.snapshot(0);

        assert!(snapshot.bids.is_empty());
        assert!(snapshot.asks.is_empty());
    }

    #[test]
    fn snapshot_bid_depth_sorted_descending() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        book.add_limit_order(limit_order(2, Side::Buy, 103, 10))
            .unwrap();
        book.add_limit_order(limit_order(3, Side::Buy, 101, 10))
            .unwrap();

        let prices: Vec<_> = book
            .snapshot(2)
            .bids
            .into_iter()
            .map(|level| level.price)
            .collect();

        assert_eq!(prices, vec![PriceTicks(103), PriceTicks(101)]);
    }

    #[test]
    fn snapshot_ask_depth_sorted_ascending() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Sell, 105, 10))
            .unwrap();
        book.add_limit_order(limit_order(2, Side::Sell, 103, 10))
            .unwrap();
        book.add_limit_order(limit_order(3, Side::Sell, 104, 10))
            .unwrap();

        let prices: Vec<_> = book
            .snapshot(2)
            .asks
            .into_iter()
            .map(|level| level.price)
            .collect();

        assert_eq!(prices, vec![PriceTicks(103), PriceTicks(104)]);
    }

    #[test]
    fn snapshot_aggregates_quantity_at_level() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        book.add_limit_order(limit_order(2, Side::Buy, 100, 25))
            .unwrap();

        assert_eq!(
            book.snapshot(1).bids,
            vec![LevelSnapshot {
                price: PriceTicks(100),
                total_qty: Qty(35),
                order_count: 2,
                order_ids: vec![1, 2],
            }]
        );
    }

    #[test]
    fn snapshot_respects_depth_limit() {
        let mut book = OrderBook::new(symbol());
        for (order_id, price) in [(1, 100), (2, 101), (3, 102)] {
            book.add_limit_order(limit_order(order_id, Side::Buy, price, 10))
                .unwrap();
        }
        for (order_id, price) in [(4, 103), (5, 104), (6, 105)] {
            book.add_limit_order(limit_order(order_id, Side::Sell, price, 10))
                .unwrap();
        }

        let snapshot = book.snapshot(2);

        assert_eq!(snapshot.bids.len(), 2);
        assert_eq!(snapshot.asks.len(), 2);
        assert_eq!(snapshot.bids[0].price, PriceTicks(102));
        assert_eq!(snapshot.asks[0].price, PriceTicks(103));
    }

    #[test]
    fn snapshot_includes_symbol() {
        let book = OrderBook::new(symbol());

        assert_eq!(book.snapshot(1).symbol, symbol());
    }

    #[test]
    fn snapshot_is_deterministic() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        book.add_limit_order(limit_order(2, Side::Sell, 101, 20))
            .unwrap();

        let first = serde_json::to_string(&book.snapshot(10)).unwrap();
        let second = serde_json::to_string(&book.snapshot(10)).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn invariants_hold_after_multiple_adds() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        book.add_limit_order(limit_order(2, Side::Buy, 100, 20))
            .unwrap();
        book.add_limit_order(limit_order(3, Side::Buy, 99, 30))
            .unwrap();
        book.add_limit_order(limit_order(4, Side::Sell, 101, 40))
            .unwrap();
        book.add_limit_order(limit_order(5, Side::Sell, 102, 50))
            .unwrap();

        assert_eq!(book.check_invariants(), Ok(()));
    }

    #[test]
    fn invariants_hold_after_duplicate_rejection() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();

        assert_eq!(
            book.add_limit_order(limit_order(1, Side::Sell, 101, 20)),
            Err(BookError::DuplicateOrderId(1))
        );
        assert_eq!(book.check_invariants(), Ok(()));
    }

    #[test]
    fn invariants_hold_after_invalid_quantity_rejection() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();

        assert_eq!(
            book.add_limit_order(limit_order(2, Side::Sell, 101, 0)),
            Err(BookError::InvalidQuantity(Qty(0)))
        );
        assert_eq!(book.check_invariants(), Ok(()));
    }

    #[test]
    fn no_empty_price_levels_after_adds() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        book.add_limit_order(limit_order(2, Side::Sell, 101, 20))
            .unwrap();

        assert!(book.bids.values().all(|level| !level.is_empty()));
        assert!(book.asks.values().all(|level| !level.is_empty()));
        assert_eq!(book.check_invariants(), Ok(()));
    }

    #[test]
    fn invariant_count_matches_total_resting_orders() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        book.add_limit_order(limit_order(2, Side::Buy, 100, 20))
            .unwrap();
        book.add_limit_order(limit_order(3, Side::Sell, 101, 30))
            .unwrap();

        let resting_count: usize = book
            .bids
            .values()
            .chain(book.asks.values())
            .map(PriceLevel::len)
            .sum();

        assert_eq!(book.order_index.len(), resting_count);
        assert_eq!(book.check_invariants(), Ok(()));
    }

    #[test]
    fn invariant_checker_detects_empty_price_level() {
        let mut book = OrderBook::new(symbol());
        book.bids
            .insert(PriceTicks(100), PriceLevel::new(PriceTicks(100)));

        assert_eq!(
            book.check_invariants(),
            Err(BookInvariantError::EmptyPriceLevel {
                side: Side::Buy,
                price: PriceTicks(100),
            })
        );
    }

    #[test]
    fn invariant_checker_detects_missing_index_entry() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        book.order_index.remove(&1);

        assert_eq!(
            book.check_invariants(),
            Err(BookInvariantError::MissingIndexEntry(1))
        );
    }

    #[test]
    fn invariant_checker_detects_index_pointing_to_missing_order() {
        let mut book = OrderBook::new(symbol());
        book.order_index.insert(
            1,
            OrderLocation {
                side: Side::Sell,
                price: PriceTicks(101),
            },
        );

        assert_eq!(
            book.check_invariants(),
            Err(BookInvariantError::IndexPointsToMissingOrder {
                order_id: 1,
                side: Side::Sell,
                price: PriceTicks(101),
            })
        );
    }

    #[test]
    fn invariant_checker_detects_wrong_order_side() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        book.bids
            .get_mut(&PriceTicks(100))
            .unwrap()
            .push_back(resting_order(2, Side::Sell, 100, 10));

        assert_eq!(
            book.check_invariants(),
            Err(BookInvariantError::WrongOrderSide {
                order_id: 2,
                expected: Side::Buy,
                actual: Side::Sell,
            })
        );
    }

    #[test]
    fn invariant_checker_detects_wrong_order_price() {
        let mut book = OrderBook::new(symbol());
        let mut level = PriceLevel::new(PriceTicks(99));
        level.push_back(resting_order(1, Side::Buy, 99, 10));
        book.bids.insert(PriceTicks(100), level);

        assert_eq!(
            book.check_invariants(),
            Err(BookInvariantError::WrongOrderPrice {
                order_id: 1,
                level_price: PriceTicks(100),
                order_price: PriceTicks(99),
            })
        );
    }

    #[test]
    fn invariant_checker_detects_zero_quantity() {
        let mut book = OrderBook::new(symbol());
        let mut level = PriceLevel::new(PriceTicks(100));
        level.push_back(resting_order(1, Side::Buy, 100, 0));
        book.bids.insert(PriceTicks(100), level);

        assert_eq!(
            book.check_invariants(),
            Err(BookInvariantError::ZeroQuantity(1))
        );
    }

    #[test]
    fn invariant_checker_detects_index_location_mismatch() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        book.order_index.insert(
            1,
            OrderLocation {
                side: Side::Sell,
                price: PriceTicks(101),
            },
        );

        assert_eq!(
            book.check_invariants(),
            Err(BookInvariantError::IndexLocationMismatch {
                order_id: 1,
                indexed_side: Side::Sell,
                indexed_price: PriceTicks(101),
                resting_side: Side::Buy,
                resting_price: PriceTicks(100),
            })
        );
    }

    #[test]
    fn invariant_checker_detects_order_count_mismatch() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, 10))
            .unwrap();
        book.bids
            .get_mut(&PriceTicks(100))
            .unwrap()
            .push_back(resting_order(1, Side::Buy, 100, 10));

        assert_eq!(
            book.check_invariants(),
            Err(BookInvariantError::OrderCountMismatch {
                indexed: 1,
                resting: 2,
            })
        );
    }

    #[test]
    fn invariant_checker_detects_quantity_overflow() {
        let mut book = OrderBook::new(symbol());
        book.add_limit_order(limit_order(1, Side::Buy, 100, u64::MAX))
            .unwrap();
        book.bids
            .get_mut(&PriceTicks(100))
            .unwrap()
            .push_back(resting_order(2, Side::Buy, 100, 1));

        assert_eq!(
            book.check_invariants(),
            Err(BookInvariantError::QuantityOverflow {
                side: Side::Buy,
                price: PriceTicks(100),
            })
        );
    }
}
