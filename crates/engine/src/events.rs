use serde::{Deserialize, Serialize};

use crate::types::{Order, OrderId, PriceTicks, Qty, SequenceNumber, Side, Symbol, TimestampNanos};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewOrderEvent {
    pub seq: SequenceNumber,
    pub order: Order,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelOrderEvent {
    pub seq: SequenceNumber,
    pub order_id: OrderId,
    pub symbol: Symbol,
    pub timestamp_ns: TimestampNanos,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum InputEvent {
    NewOrder(NewOrderEvent),
    Cancel(CancelOrderEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trade {
    pub symbol: Symbol,
    pub taker_order_id: OrderId,
    pub maker_order_id: OrderId,
    pub price: PriceTicks,
    pub qty: Qty,
    pub aggressor_side: Side,
    pub timestamp_ns: TimestampNanos,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RejectReason {
    UnknownOrder,
    AlreadyFilled,
    AlreadyCancelled,
    EmptyBook,
    InvalidQuantity,
    InvalidPrice,
    MarketOrderWouldNotFill,
    InternalBookInvariantViolation,
    // Kept for compatibility with the Week 3 `submit_limit_order` API.
    InvalidOrder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ExecutionReport {
    Accepted {
        order_id: OrderId,
    },
    Filled {
        order_id: OrderId,
        qty: Qty,
        price: PriceTicks,
    },
    PartiallyFilled {
        order_id: OrderId,
        qty: Qty,
        remaining: Qty,
        price: PriceTicks,
    },
    Rested {
        order_id: OrderId,
        remaining: Qty,
    },
    Cancelled {
        order_id: OrderId,
    },
    Rejected {
        order_id: OrderId,
        reason: RejectReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OutputEvent {
    Accepted {
        seq: SequenceNumber,
        order_id: OrderId,
    },
    Cancelled {
        seq: SequenceNumber,
        order_id: OrderId,
    },
    Rejected {
        seq: SequenceNumber,
        order_id: Option<OrderId>,
        reason: RejectReason,
    },
    Trade {
        seq: SequenceNumber,
        trade: Trade,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OrderType, StrategyId};

    #[test]
    fn deserializes_new_order_event_from_json() {
        let json = r#"{"type":"NewOrder","seq":1,"order":{"order_id":1001,"symbol":"AAPL","side":"Buy","order_type":"Limit","price":10000,"qty":50,"timestamp_ns":1,"strategy_id":null}}"#;

        let event: InputEvent =
            serde_json::from_str(json).expect("new order event should deserialize");

        let expected = InputEvent::NewOrder(NewOrderEvent {
            seq: 1,
            order: Order {
                order_id: 1001,
                symbol: Symbol("AAPL".to_string()),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: Some(PriceTicks(10000)),
                qty: Qty(50),
                timestamp_ns: 1,
                strategy_id: None::<StrategyId>,
            },
        });

        assert_eq!(event, expected);
    }

    #[test]
    fn deserializes_cancel_event_from_json() {
        let json = r#"{"type":"Cancel","seq":2,"order_id":1001,"symbol":"AAPL","timestamp_ns":2}"#;

        let event: InputEvent =
            serde_json::from_str(json).expect("cancel event should deserialize");

        let expected = InputEvent::Cancel(CancelOrderEvent {
            seq: 2,
            order_id: 1001,
            symbol: Symbol("AAPL".to_string()),
            timestamp_ns: 2,
        });

        assert_eq!(event, expected);
    }

    #[test]
    fn serializes_output_event() {
        let event = OutputEvent::Accepted {
            seq: 1,
            order_id: 1001,
        };

        let json = serde_json::to_string(&event).expect("output event should serialize");

        assert_eq!(json, r#"{"type":"Accepted","seq":1,"order_id":1001}"#);
    }

    #[test]
    fn serializes_trade_output_event() {
        let event = OutputEvent::Trade {
            seq: 3,
            trade: Trade {
                symbol: Symbol("AAPL".to_string()),
                taker_order_id: 2001,
                maker_order_id: 1001,
                price: PriceTicks(10000),
                qty: Qty(25),
                aggressor_side: Side::Buy,
                timestamp_ns: 3,
            },
        };

        let json = serde_json::to_string(&event).expect("trade event should serialize");
        let decoded: OutputEvent =
            serde_json::from_str(&json).expect("trade event should deserialize");

        assert_eq!(decoded, event);
    }

    #[test]
    fn execution_report_serialization_round_trips() {
        let report = ExecutionReport::Rested {
            order_id: 1001,
            remaining: Qty(25),
        };

        let json = serde_json::to_string(&report).expect("execution report should serialize");
        let decoded: ExecutionReport =
            serde_json::from_str(&json).expect("execution report should deserialize");

        assert_eq!(decoded, report);
    }

    #[test]
    fn constructs_each_week_four_execution_outcome() {
        let reports = [
            ExecutionReport::Accepted { order_id: 1 },
            ExecutionReport::Filled {
                order_id: 1,
                qty: Qty(5),
                price: PriceTicks(100),
            },
            ExecutionReport::PartiallyFilled {
                order_id: 1,
                qty: Qty(3),
                remaining: Qty(2),
                price: PriceTicks(100),
            },
            ExecutionReport::Cancelled { order_id: 1 },
            ExecutionReport::Rejected {
                order_id: 1,
                reason: RejectReason::UnknownOrder,
            },
        ];

        for report in reports {
            let json = serde_json::to_string(&report).expect("report should serialize");
            let decoded: ExecutionReport =
                serde_json::from_str(&json).expect("report should deserialize");
            assert_eq!(decoded, report);
        }
    }

    #[test]
    fn reject_reasons_round_trip_through_json() {
        let reasons = [
            RejectReason::UnknownOrder,
            RejectReason::AlreadyFilled,
            RejectReason::AlreadyCancelled,
            RejectReason::EmptyBook,
            RejectReason::InvalidQuantity,
            RejectReason::InvalidPrice,
            RejectReason::MarketOrderWouldNotFill,
            RejectReason::InternalBookInvariantViolation,
        ];

        for reason in reasons {
            let json = serde_json::to_string(&reason).expect("reason should serialize");
            let decoded: RejectReason =
                serde_json::from_str(&json).expect("reason should deserialize");
            assert_eq!(decoded, reason);
        }
    }
}
