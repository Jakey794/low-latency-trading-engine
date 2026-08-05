//! Paper / demo market-data adapter (no live trading).
//!
//! Converts external-style JSON market-data messages into runtime events.
//! Includes an in-memory mock duplex for deterministic tests. Optional CLI
//! WebSocket demos bind only to localhost and never submit real orders.

mod convert;
mod mock;

pub use convert::{paper_message_to_runtime_event, PaperConvertError};
pub use mock::{DrainError, MockPaperSession, MockPaperSink, MockPaperSource};

use serde::{Deserialize, Serialize};

use crate::types::{OrderId, OrderType, PriceTicks, Qty, Side, Symbol, TimestampNanos};

/// Trait separating ingress adapters from engine logic.
pub trait MarketDataAdapter {
    type Error;

    /// Pull the next external message, if any.
    fn next_message(&mut self) -> Result<Option<PaperMdMessage>, Self::Error>;
}

/// External-style paper market-data / order messages (JSON-serializable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PaperMdMessage {
    /// Top-of-book style quote used to seed/rest liquidity via limit orders.
    Quote {
        seq: u64,
        ts_ns: TimestampNanos,
        symbol: String,
        bid_px: i64,
        bid_qty: u64,
        ask_px: i64,
        ask_qty: u64,
    },
    /// Aggressing paper order (limit or market).
    PaperOrder {
        seq: u64,
        ts_ns: TimestampNanos,
        order_id: OrderId,
        symbol: String,
        side: Side,
        #[serde(default = "default_limit")]
        order_type: OrderType,
        price: Option<i64>,
        qty: u64,
    },
    /// Cancel a previously submitted paper order.
    PaperCancel {
        seq: u64,
        ts_ns: TimestampNanos,
        order_id: OrderId,
        symbol: String,
    },
    /// Logical timer tick (not wall clock).
    Timer {
        seq: u64,
        ts_ns: TimestampNanos,
        #[serde(default)]
        symbol: Option<String>,
    },
    /// Adapter-level heartbeat / keepalive (ignored by engine conversion).
    Heartbeat { seq: u64, ts_ns: TimestampNanos },
    /// Explicit disconnect signal for demos/tests.
    Disconnect { reason: String },
}

fn default_limit() -> OrderType {
    OrderType::Limit
}

/// Paper execution / status reports emitted by the adapter bridge (demo only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PaperReport {
    Accepted {
        order_id: OrderId,
        symbol: String,
    },
    Rejected {
        order_id: Option<OrderId>,
        reason: String,
    },
    Trade {
        symbol: String,
        price: PriceTicks,
        qty: Qty,
        taker_side: Side,
    },
    Cancelled {
        order_id: OrderId,
    },
    AdapterError {
        message: String,
    },
    Reconnecting {
        attempt: u32,
    },
    Disconnected {
        reason: String,
    },
}

/// Adapter connection state for reconnect demos (deterministic counters only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdapterState {
    #[default]
    Disconnected,
    Connected,
    Reconnecting,
}

/// Lightweight reconnect controller with bounded attempts (no wall-clock sleep).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconnectPolicy {
    pub max_attempts: u32,
    attempts: u32,
    state: AdapterState,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            attempts: 0,
            state: AdapterState::Disconnected,
        }
    }
}

impl ReconnectPolicy {
    pub fn new(max_attempts: u32) -> Self {
        Self {
            max_attempts,
            attempts: 0,
            state: AdapterState::Disconnected,
        }
    }

    pub fn state(&self) -> AdapterState {
        self.state
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub fn on_connected(&mut self) {
        self.state = AdapterState::Connected;
        self.attempts = 0;
    }

    /// Record a disconnect and decide whether to reconnect.
    ///
    /// Returns `Some(attempt)` when another reconnect should be attempted.
    pub fn on_disconnect(&mut self) -> Option<u32> {
        if self.attempts >= self.max_attempts {
            self.state = AdapterState::Disconnected;
            return None;
        }
        self.attempts = self.attempts.saturating_add(1);
        self.state = AdapterState::Reconnecting;
        Some(self.attempts)
    }

    pub fn mark_failed(&mut self) {
        self.state = AdapterState::Disconnected;
    }
}

/// Helper to build a symbol from paper message text.
pub fn paper_symbol(s: impl Into<String>) -> Symbol {
    Symbol(s.into())
}
