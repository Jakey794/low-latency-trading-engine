//! Strategy plugin interface.
//!
//! Strategies observe read-only market/portfolio context and return intents.
//! They must never mutate the book, risk state, or portfolio directly.

use crate::{
    book::BookSnapshot,
    events::Trade,
    portfolio::PortfolioSnapshot,
    risk::RiskRejectReason,
    types::{OrderId, OrderType, PriceTicks, Qty, Side, StrategyId, Symbol, TimestampNanos},
};

/// Read-only context provided to strategies on each callback.
#[derive(Debug, Clone)]
pub struct StrategyContext {
    pub strategy_id: StrategyId,
    pub seq: u64,
    pub ts_ns: TimestampNanos,
    pub portfolio: PortfolioSnapshot,
    pub book: Option<BookSnapshot>,
    pub risk_killed: bool,
}

/// Events delivered to strategies in deterministic order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrategyEvent {
    Activated,
    Deactivated,
    BookUpdate {
        symbol: Symbol,
        book: BookSnapshot,
    },
    Trade {
        trade: Trade,
    },
    Fill {
        order_id: OrderId,
        symbol: Symbol,
        side: Side,
        price: PriceTicks,
        qty: Qty,
    },
    /// Explicit timer/tick from replay input (not wall-clock).
    Timer {
        seq: u64,
        ts_ns: TimestampNanos,
    },
    RiskRejection {
        order_id: Option<OrderId>,
        reason: RiskRejectReason,
    },
}

/// Commands a strategy may emit. The runtime assigns deterministic order IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrategyCommand {
    PlaceOrder {
        symbol: Symbol,
        side: Side,
        order_type: OrderType,
        price: Option<PriceTicks>,
        qty: Qty,
    },
    Cancel {
        order_id: OrderId,
        symbol: Symbol,
    },
}

/// Strategy plugin trait. Implementations must be deterministic.
pub trait Strategy: Send {
    fn id(&self) -> StrategyId;
    fn name(&self) -> &str;

    fn on_event(&mut self, event: &StrategyEvent, ctx: &StrategyContext) -> Vec<StrategyCommand>;
}

/// No-op strategy used in tests.
#[derive(Debug, Default)]
pub struct NullStrategy {
    pub id: StrategyId,
}

impl Strategy for NullStrategy {
    fn id(&self) -> StrategyId {
        self.id
    }

    fn name(&self) -> &str {
        "null"
    }

    fn on_event(&mut self, _event: &StrategyEvent, _ctx: &StrategyContext) -> Vec<StrategyCommand> {
        Vec::new()
    }
}
