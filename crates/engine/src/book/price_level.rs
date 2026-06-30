use std::collections::VecDeque;

use crate::types::{OrderId, PriceTicks, Qty};

use super::order_book::RestingOrder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PriceLevel {
    price: PriceTicks,
    orders: VecDeque<RestingOrder>,
}

impl PriceLevel {
    pub(crate) fn new(price: PriceTicks) -> Self {
        Self {
            price,
            orders: VecDeque::new(),
        }
    }

    pub(crate) fn push_back(&mut self, order: RestingOrder) {
        assert_eq!(
            order.price, self.price,
            "resting order price must match its price level"
        );
        self.orders.push_back(order);
    }

    pub(crate) fn front(&self) -> Option<&RestingOrder> {
        self.orders.front()
    }

    pub(crate) fn pop_front(&mut self) -> Option<RestingOrder> {
        self.orders.pop_front()
    }

    pub(crate) fn len(&self) -> usize {
        self.orders.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    pub(crate) fn total_qty(&self) -> Qty {
        self.orders.iter().fold(Qty(0), |total, order| {
            Qty(total
                .0
                .checked_add(order.qty.0)
                .expect("price-level quantity overflow"))
        })
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &RestingOrder> {
        self.orders.iter()
    }

    pub(crate) fn get_order(&self, order_id: OrderId) -> Option<&RestingOrder> {
        let front = self.front()?;
        if front.order_id == order_id {
            return Some(front);
        }

        self.orders
            .iter()
            .skip(1)
            .find(|order| order.order_id == order_id)
    }
}

#[cfg(test)]
mod tests {
    use crate::types::{PriceTicks, Qty, Side};

    use super::*;

    const PRICE: PriceTicks = PriceTicks(100);

    fn resting_order(order_id: OrderId) -> RestingOrder {
        RestingOrder {
            order_id,
            side: Side::Buy,
            price: PRICE,
            qty: Qty(10),
            timestamp_ns: order_id,
            strategy_id: None,
        }
    }

    #[test]
    fn same_price_orders_preserve_fifo() {
        let mut level = PriceLevel::new(PRICE);

        level.push_back(resting_order(1));
        level.push_back(resting_order(2));

        let order_ids: Vec<_> = level.orders.iter().map(|order| order.order_id).collect();
        assert_eq!(order_ids, vec![1, 2]);
    }

    #[test]
    fn front_order_is_earliest_order() {
        let mut level = PriceLevel::new(PRICE);

        level.push_back(resting_order(10));
        level.push_back(resting_order(11));

        assert_eq!(level.front().map(|order| order.order_id), Some(10));
    }

    #[test]
    fn pop_front_removes_earliest_order() {
        let mut level = PriceLevel::new(PRICE);
        level.push_back(resting_order(10));
        level.push_back(resting_order(11));

        let removed = level.pop_front();

        assert_eq!(removed.map(|order| order.order_id), Some(10));
        assert_eq!(level.front().map(|order| order.order_id), Some(11));
        assert_eq!(level.len(), 1);
    }

    #[test]
    fn price_level_len_updates() {
        let mut level = PriceLevel::new(PRICE);
        assert_eq!(level.len(), 0);

        level.push_back(resting_order(1));
        assert_eq!(level.len(), 1);

        level.push_back(resting_order(2));
        assert_eq!(level.len(), 2);
    }

    #[test]
    fn price_level_empty_after_no_orders() {
        let level = PriceLevel::new(PRICE);

        assert!(level.is_empty());
        assert_eq!(level.front(), None);
    }

    #[test]
    fn multiple_orders_same_price_keep_insertion_order() {
        let mut level = PriceLevel::new(PRICE);

        for order_id in 1..=5 {
            level.push_back(resting_order(order_id));
        }

        let order_ids: Vec<_> = level.orders.iter().map(|order| order.order_id).collect();
        assert_eq!(order_ids, vec![1, 2, 3, 4, 5]);
    }
}
