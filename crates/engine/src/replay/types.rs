use serde::{Deserialize, Serialize};

use crate::{
    events::{RejectReason, Trade},
    types::{Order, OrderId, Qty, SequenceNumber, Symbol, TimestampNanos},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayEvent {
    pub seq: SequenceNumber,
    pub ts_ns: TimestampNanos,
    #[serde(flatten)]
    pub kind: ReplayEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplayEventKind {
    NewOrder { order: Order },
    Cancel { order_id: OrderId, symbol: Symbol },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayOutputEvent {
    pub seq: SequenceNumber,
    pub ts_ns: TimestampNanos,
    #[serde(flatten)]
    pub kind: ReplayOutputKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplayOutputKind {
    Accepted {
        order_id: OrderId,
    },
    Rejected {
        order_id: Option<OrderId>,
        reason: RejectReason,
    },
    Trade {
        trade: Trade,
    },
    Cancelled {
        order_id: OrderId,
    },
    Expired {
        order_id: OrderId,
        remaining: Qty,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OrderType, PriceTicks, Side, StrategyId};

    #[test]
    fn deserializes_limit_order_json_object() {
        let json = r#"{"seq":1,"ts_ns":100,"kind":"new_order","order":{"order_id":1001,"symbol":"AAPL","side":"Buy","order_type":"Limit","price":10025,"qty":10,"timestamp_ns":90,"strategy_id":null}}"#;

        let event: ReplayEvent =
            serde_json::from_str(json).expect("limit replay event should deserialize");

        assert_eq!(
            event,
            ReplayEvent {
                seq: 1,
                ts_ns: 100,
                kind: ReplayEventKind::NewOrder {
                    order: Order {
                        order_id: 1001,
                        symbol: Symbol("AAPL".to_owned()),
                        side: Side::Buy,
                        order_type: OrderType::Limit,
                        price: Some(PriceTicks(10025)),
                        qty: Qty(10),
                        timestamp_ns: 90,
                        strategy_id: None::<StrategyId>,
                    },
                },
            }
        );
    }

    #[test]
    fn deserializes_market_order_json_object() {
        let json = r#"{"seq":2,"ts_ns":200,"kind":"new_order","order":{"order_id":1002,"symbol":"AAPL","side":"Sell","order_type":"Market","price":null,"qty":7,"timestamp_ns":190,"strategy_id":42}}"#;

        let event: ReplayEvent =
            serde_json::from_str(json).expect("market replay event should deserialize");

        assert_eq!(
            event,
            ReplayEvent {
                seq: 2,
                ts_ns: 200,
                kind: ReplayEventKind::NewOrder {
                    order: Order {
                        order_id: 1002,
                        symbol: Symbol("AAPL".to_owned()),
                        side: Side::Sell,
                        order_type: OrderType::Market,
                        price: None,
                        qty: Qty(7),
                        timestamp_ns: 190,
                        strategy_id: Some(42),
                    },
                },
            }
        );
    }

    #[test]
    fn deserializes_cancel_json_object() {
        let json = r#"{"seq":3,"ts_ns":300,"kind":"cancel","order_id":1001,"symbol":"AAPL"}"#;

        let event: ReplayEvent =
            serde_json::from_str(json).expect("cancel replay event should deserialize");

        assert_eq!(
            event,
            ReplayEvent {
                seq: 3,
                ts_ns: 300,
                kind: ReplayEventKind::Cancel {
                    order_id: 1001,
                    symbol: Symbol("AAPL".to_owned()),
                },
            }
        );
    }

    #[test]
    fn serializes_output_event_as_compact_json() {
        let event = ReplayOutputEvent {
            seq: 4,
            ts_ns: 400,
            kind: ReplayOutputKind::Rejected {
                order_id: Some(1003),
                reason: RejectReason::InvalidQuantity,
            },
        };

        let json = serde_json::to_string(&event).expect("output replay event should serialize");

        assert_eq!(
            json,
            r#"{"seq":4,"ts_ns":400,"kind":"rejected","order_id":1003,"reason":"InvalidQuantity"}"#
        );
    }

    #[test]
    fn malformed_json_returns_error() {
        let malformed = r#"{"seq":1,"ts_ns":100,"kind":"new_order","order":{"#;

        let result = serde_json::from_str::<ReplayEvent>(malformed);

        assert!(result.is_err());
    }
}
