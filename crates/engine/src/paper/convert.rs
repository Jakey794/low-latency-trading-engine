//! Convert paper market-data messages into runtime events.

use thiserror::Error;

use crate::{
    runtime::RuntimeEvent,
    types::{Order, OrderType, PriceTicks, Qty, Side, Symbol},
};

use super::PaperMdMessage;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PaperConvertError {
    #[error("invalid quote: crossed or non-positive sizes")]
    InvalidQuote,
    #[error("invalid paper order quantity")]
    InvalidQuantity,
    #[error("limit order requires a non-negative price")]
    InvalidPrice,
    #[error("heartbeat and disconnect messages are not runtime events")]
    NotAnEvent,
}

/// Convert a paper MD message into zero or more runtime events.
///
/// Quotes expand into two resting limit orders (bid then ask) using
/// deterministic synthetic order IDs derived from `seq`.
pub fn paper_message_to_runtime_event(
    msg: &PaperMdMessage,
) -> Result<Vec<RuntimeEvent>, PaperConvertError> {
    match msg {
        PaperMdMessage::Heartbeat { .. } | PaperMdMessage::Disconnect { .. } => {
            Err(PaperConvertError::NotAnEvent)
        }
        PaperMdMessage::Timer { seq, ts_ns, symbol } => Ok(vec![RuntimeEvent::Timer {
            seq: *seq,
            ts_ns: *ts_ns,
            symbol: symbol.as_ref().map(|s| Symbol(s.clone())),
        }]),
        PaperMdMessage::PaperCancel {
            seq,
            ts_ns,
            order_id,
            symbol,
        } => Ok(vec![RuntimeEvent::Cancel {
            seq: *seq,
            ts_ns: *ts_ns,
            order_id: *order_id,
            symbol: Symbol(symbol.clone()),
        }]),
        PaperMdMessage::PaperOrder {
            seq,
            ts_ns,
            order_id,
            symbol,
            side,
            order_type,
            price,
            qty,
        } => {
            if *qty == 0 {
                return Err(PaperConvertError::InvalidQuantity);
            }
            let price_ticks = match (order_type, price) {
                (OrderType::Limit, Some(p)) if *p >= 0 => Some(PriceTicks(*p)),
                (OrderType::Limit, _) => return Err(PaperConvertError::InvalidPrice),
                (OrderType::Market, Some(p)) if *p >= 0 => Some(PriceTicks(*p)),
                (OrderType::Market, Some(_)) => return Err(PaperConvertError::InvalidPrice),
                (OrderType::Market, None) => None,
            };
            Ok(vec![RuntimeEvent::NewOrder {
                seq: *seq,
                ts_ns: *ts_ns,
                order: Order {
                    order_id: *order_id,
                    symbol: Symbol(symbol.clone()),
                    side: *side,
                    order_type: *order_type,
                    price: price_ticks,
                    qty: Qty(*qty),
                    timestamp_ns: *ts_ns,
                    strategy_id: None,
                },
            }])
        }
        PaperMdMessage::Quote {
            seq,
            ts_ns,
            symbol,
            bid_px,
            bid_qty,
            ask_px,
            ask_qty,
        } => {
            if *bid_qty == 0 || *ask_qty == 0 || *bid_px < 0 || *ask_px < 0 || bid_px >= ask_px {
                return Err(PaperConvertError::InvalidQuote);
            }
            // Deterministic synthetic IDs: bid = seq*2, ask = seq*2+1
            let bid_id = seq.saturating_mul(2);
            let ask_id = bid_id.saturating_add(1);
            Ok(vec![
                RuntimeEvent::NewOrder {
                    seq: *seq,
                    ts_ns: *ts_ns,
                    order: Order {
                        order_id: bid_id,
                        symbol: Symbol(symbol.clone()),
                        side: Side::Buy,
                        order_type: OrderType::Limit,
                        price: Some(PriceTicks(*bid_px)),
                        qty: Qty(*bid_qty),
                        timestamp_ns: *ts_ns,
                        strategy_id: None,
                    },
                },
                RuntimeEvent::NewOrder {
                    seq: seq.saturating_add(1),
                    ts_ns: *ts_ns,
                    order: Order {
                        order_id: ask_id,
                        symbol: Symbol(symbol.clone()),
                        side: Side::Sell,
                        order_type: OrderType::Limit,
                        price: Some(PriceTicks(*ask_px)),
                        qty: Qty(*ask_qty),
                        timestamp_ns: *ts_ns,
                        strategy_id: None,
                    },
                },
            ])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Side;

    #[test]
    fn quote_expands_to_bid_ask() {
        let msg = PaperMdMessage::Quote {
            seq: 10,
            ts_ns: 100,
            symbol: "AAPL".into(),
            bid_px: 100,
            bid_qty: 5,
            ask_px: 101,
            ask_qty: 5,
        };
        let events = paper_message_to_runtime_event(&msg).unwrap();
        assert_eq!(events.len(), 2);
        match &events[0] {
            RuntimeEvent::NewOrder { order, .. } => {
                assert_eq!(order.side, Side::Buy);
                assert_eq!(order.price, Some(PriceTicks(100)));
            }
            _ => panic!("expected bid"),
        }
        match &events[1] {
            RuntimeEvent::NewOrder { order, .. } => {
                assert_eq!(order.side, Side::Sell);
                assert_eq!(order.price, Some(PriceTicks(101)));
            }
            _ => panic!("expected ask"),
        }
    }

    #[test]
    fn crossed_quote_rejected() {
        let msg = PaperMdMessage::Quote {
            seq: 1,
            ts_ns: 1,
            symbol: "AAPL".into(),
            bid_px: 105,
            bid_qty: 1,
            ask_px: 100,
            ask_qty: 1,
        };
        assert_eq!(
            paper_message_to_runtime_event(&msg),
            Err(PaperConvertError::InvalidQuote)
        );
    }
}
