use serde::{Deserialize, Serialize};

pub type OrderId = u64;
pub type StrategyId = u64;
pub type SequenceNumber = u64;
pub type TimestampNanos = u64;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PriceTicks(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Qty(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub order_id: OrderId,
    pub symbol: Symbol,
    pub side: Side,
    pub order_type: OrderType,
    pub price: Option<PriceTicks>,
    pub qty: Qty,
    pub timestamp_ns: TimestampNanos,
    pub strategy_id: Option<StrategyId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_ticks_ordering_works() {
        assert!(PriceTicks(101) > PriceTicks(100));
        assert!(PriceTicks(99) < PriceTicks(100));
        assert_eq!(PriceTicks(100), PriceTicks(100));
    }

    #[test]
    fn qty_equality_works() {
        assert_eq!(Qty(10), Qty(10));
        assert_ne!(Qty(10), Qty(11));
    }

    #[test]
    fn order_serialization_round_trips() {
        let order = Order {
            order_id: 1001,
            symbol: Symbol("AAPL".to_string()),
            side: Side::Buy,
            order_type: OrderType::Limit,
            price: Some(PriceTicks(10000)),
            qty: Qty(50),
            timestamp_ns: 1,
            strategy_id: None,
        };

        let json = serde_json::to_string(&order).expect("order should serialize");
        let decoded: Order = serde_json::from_str(&json).expect("order should deserialize");

        assert_eq!(decoded, order);
    }

    #[test]
    fn market_order_has_no_price() {
        let order = Order {
            order_id: 1,
            symbol: Symbol("AAPL".to_string()),
            side: Side::Buy,
            order_type: OrderType::Market,
            price: None,
            qty: Qty(10),
            timestamp_ns: 1,
            strategy_id: None,
        };

        assert_eq!(order.order_type, OrderType::Market);
        assert_eq!(order.price, None);
    }

    #[test]
    fn limit_order_has_price() {
        let order = Order {
            order_id: 1,
            symbol: Symbol("AAPL".to_string()),
            side: Side::Buy,
            order_type: OrderType::Limit,
            price: Some(PriceTicks(10000)),
            qty: Qty(10),
            timestamp_ns: 1,
            strategy_id: None,
        };

        assert_eq!(order.order_type, OrderType::Limit);
        assert_eq!(order.price, Some(PriceTicks(10000)));
    }
}
