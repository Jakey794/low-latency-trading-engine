//! Deterministic runtime orchestration.
//!
//! Owns symbol routing, matching engines, risk, portfolio, strategies, and
//! ordered output collection. Strategy intents are bounded per external event
//! and always pass through risk before matching.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    book::BookSnapshot,
    events::{CancelOrderEvent, ExecutionReport, InputEvent, NewOrderEvent, RejectReason, Trade},
    matching::MatchingEngine,
    portfolio::{Money, Portfolio, PortfolioError, PortfolioSnapshot},
    risk::{RiskDecision, RiskLimits, RiskManager, RiskRejectReason},
    strategy::{Strategy, StrategyCommand, StrategyContext, StrategyEvent},
    types::{Order, OrderId, Qty, SequenceNumber, Side, StrategyId, Symbol, TimestampNanos},
};

/// Default cap on strategy-generated commands per external event (prevents runaway loops).
pub const DEFAULT_MAX_STRATEGY_COMMANDS_PER_EVENT: usize = 16;

/// Maximum nested strategy dispatch depth (external event = 1).
pub const MAX_STRATEGY_DISPATCH_DEPTH: u32 = 2;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    #[error("symbol mismatch: engine={engine:?} order={order:?}")]
    SymbolMismatch { engine: Symbol, order: Symbol },
    #[error("unknown symbol {0:?}")]
    UnknownSymbol(Symbol),
    #[error(transparent)]
    Portfolio(#[from] PortfolioError),
    #[error("strategy command budget exceeded ({limit})")]
    StrategyBudgetExceeded { limit: usize },
    #[error("duplicate order id {0}")]
    DuplicateOrderId(OrderId),
    #[error("duplicate runtime sequence {seq}")]
    DuplicateSequence { seq: SequenceNumber },
    #[error("out-of-order runtime sequence {seq} after {previous}")]
    OutOfOrderSequence {
        previous: SequenceNumber,
        seq: SequenceNumber,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeOutput {
    pub seq: SequenceNumber,
    pub ts_ns: TimestampNanos,
    #[serde(flatten)]
    pub kind: RuntimeOutputKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeOutputKind {
    Accepted {
        order_id: OrderId,
    },
    Rejected {
        order_id: Option<OrderId>,
        reason: RejectReason,
    },
    RiskRejected {
        order_id: Option<OrderId>,
        reason: RiskRejectReason,
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
    StrategyCommandsDropped {
        strategy_id: StrategyId,
        dropped: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub starting_cash: Money,
    pub risk_limits: RiskLimits,
    pub max_strategy_commands_per_event: usize,
    /// Starting value for runtime-assigned order IDs (strategies).
    pub next_order_id: OrderId,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            starting_cash: 1_000_000,
            risk_limits: RiskLimits::default(),
            max_strategy_commands_per_event: DEFAULT_MAX_STRATEGY_COMMANDS_PER_EVENT,
            next_order_id: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeResult {
    pub outputs: Vec<RuntimeOutput>,
    pub portfolio: PortfolioSnapshot,
    pub books: Vec<BookSnapshot>,
}

/// External events the runtime accepts (replay + explicit timer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    NewOrder {
        seq: SequenceNumber,
        ts_ns: TimestampNanos,
        order: Order,
    },
    Cancel {
        seq: SequenceNumber,
        ts_ns: TimestampNanos,
        order_id: OrderId,
        symbol: Symbol,
    },
    Timer {
        seq: SequenceNumber,
        ts_ns: TimestampNanos,
        symbol: Option<Symbol>,
    },
}

pub struct Runtime {
    engines: BTreeMap<String, MatchingEngine>,
    portfolio: Portfolio,
    risk: RiskManager,
    strategies: Vec<Box<dyn Strategy>>,
    owned_orders: HashSet<OrderId>,
    order_side: HashMap<OrderId, Side>,
    next_order_id: OrderId,
    max_strategy_commands_per_event: usize,
    last_seq: Option<SequenceNumber>,
    /// Nested strategy dispatch depth for the current external event.
    dispatch_depth: u32,
}

impl Runtime {
    pub fn new(symbols: Vec<Symbol>, config: RuntimeConfig) -> Self {
        let mut engines = BTreeMap::new();
        for symbol in symbols {
            engines.insert(symbol.0.clone(), MatchingEngine::new(symbol));
        }
        let starting = config.starting_cash;
        Self {
            engines,
            portfolio: Portfolio::new(starting),
            risk: RiskManager::new(config.risk_limits, starting),
            strategies: Vec::new(),
            owned_orders: HashSet::new(),
            order_side: HashMap::new(),
            next_order_id: config.next_order_id,
            max_strategy_commands_per_event: config.max_strategy_commands_per_event,
            last_seq: None,
            dispatch_depth: 0,
        }
    }

    pub fn ensure_symbol(&mut self, symbol: &Symbol) {
        self.engines
            .entry(symbol.0.clone())
            .or_insert_with(|| MatchingEngine::new(symbol.clone()));
    }

    pub fn add_strategy(&mut self, mut strategy: Box<dyn Strategy>) {
        let id = strategy.id();
        let ctx = self.strategy_context(id, 0, 0, None);
        let _ = strategy.on_event(&StrategyEvent::Activated, &ctx);
        self.strategies.push(strategy);
    }

    pub fn portfolio(&self) -> &Portfolio {
        &self.portfolio
    }

    pub fn risk(&self) -> &RiskManager {
        &self.risk
    }

    pub fn risk_mut(&mut self) -> &mut RiskManager {
        &mut self.risk
    }

    pub fn engine(&self, symbol: &Symbol) -> Option<&MatchingEngine> {
        self.engines.get(&symbol.0)
    }

    pub fn last_seq(&self) -> Option<SequenceNumber> {
        self.last_seq
    }

    pub fn process_events(
        &mut self,
        events: Vec<RuntimeEvent>,
    ) -> Result<RuntimeResult, RuntimeError> {
        self.validate_sequences(&events)?;
        let mut outputs = Vec::new();
        for event in events {
            let seq = event_seq(&event);
            self.process_one(event, &mut outputs)?;
            self.last_seq = Some(seq);
        }
        self.snapshot_result(outputs)
    }

    pub fn process_one(
        &mut self,
        event: RuntimeEvent,
        outputs: &mut Vec<RuntimeOutput>,
    ) -> Result<(), RuntimeError> {
        match event {
            RuntimeEvent::NewOrder { seq, ts_ns, order } => {
                self.handle_external_order(seq, ts_ns, order, outputs)?;
            }
            RuntimeEvent::Cancel {
                seq,
                ts_ns,
                order_id,
                symbol,
            } => {
                self.execute_cancel(seq, ts_ns, order_id, symbol, outputs, true)?;
            }
            RuntimeEvent::Timer { seq, ts_ns, symbol } => {
                self.dispatch_strategies(
                    seq,
                    ts_ns,
                    &StrategyEvent::Timer { seq, ts_ns },
                    symbol.as_ref(),
                    outputs,
                )?;
            }
        }
        Ok(())
    }

    fn handle_external_order(
        &mut self,
        seq: SequenceNumber,
        ts_ns: TimestampNanos,
        order: Order,
        outputs: &mut Vec<RuntimeOutput>,
    ) -> Result<(), RuntimeError> {
        let symbol = order.symbol.clone();
        self.ensure_symbol(&symbol);
        // External replay liquidity is not portfolio-owned unless strategy_id is set.
        let owned = order.strategy_id.is_some();
        self.submit_order(seq, ts_ns, order, owned, outputs)?;
        let book = self
            .engines
            .get(&symbol.0)
            .map(|e| e.book().snapshot(usize::MAX));
        if let Some(book) = book {
            self.dispatch_strategies(
                seq,
                ts_ns,
                &StrategyEvent::BookUpdate {
                    symbol: symbol.clone(),
                    book,
                },
                Some(&symbol),
                outputs,
            )?;
        }
        Ok(())
    }

    fn execute_cancel(
        &mut self,
        seq: SequenceNumber,
        ts_ns: TimestampNanos,
        order_id: OrderId,
        symbol: Symbol,
        outputs: &mut Vec<RuntimeOutput>,
        notify_strategies: bool,
    ) -> Result<(), RuntimeError> {
        debug_assert!(self.risk.allow_cancel());
        self.ensure_symbol(&symbol);
        let engine = self
            .engines
            .get_mut(&symbol.0)
            .ok_or_else(|| RuntimeError::UnknownSymbol(symbol.clone()))?;
        let (reports, _trades) = engine.process_event_with_trades(
            InputEvent::Cancel(CancelOrderEvent {
                seq,
                order_id,
                symbol: symbol.clone(),
                timestamp_ns: ts_ns,
            }),
            ts_ns,
        );
        self.emit_reports(seq, ts_ns, reports, Vec::new(), outputs);
        self.owned_orders.remove(&order_id);
        self.order_side.remove(&order_id);

        if notify_strategies {
            let book = self
                .engines
                .get(&symbol.0)
                .map(|e| e.book().snapshot(usize::MAX));
            if let Some(book) = book {
                self.dispatch_strategies(
                    seq,
                    ts_ns,
                    &StrategyEvent::BookUpdate {
                        symbol: symbol.clone(),
                        book,
                    },
                    Some(&symbol),
                    outputs,
                )?;
            }
        }
        Ok(())
    }

    fn submit_order(
        &mut self,
        seq: SequenceNumber,
        ts_ns: TimestampNanos,
        order: Order,
        mark_owned: bool,
        outputs: &mut Vec<RuntimeOutput>,
    ) -> Result<(), RuntimeError> {
        let symbol = order.symbol.clone();
        let order_id = order.order_id;
        let side = order.side;
        let order_price = order.price;
        let order_qty = order.qty;
        self.ensure_symbol(&symbol);

        match self.risk.check_new_order(&order, &self.portfolio) {
            RiskDecision::Reject { reason } => {
                outputs.push(RuntimeOutput {
                    seq,
                    ts_ns,
                    kind: RuntimeOutputKind::RiskRejected {
                        order_id: Some(order_id),
                        reason: reason.clone(),
                    },
                });
                self.dispatch_strategies(
                    seq,
                    ts_ns,
                    &StrategyEvent::RiskRejection {
                        order_id: Some(order_id),
                        reason,
                    },
                    Some(&symbol),
                    outputs,
                )?;
                return Ok(());
            }
            RiskDecision::Allow => {}
        }

        if mark_owned {
            if !self.owned_orders.insert(order_id) {
                return Err(RuntimeError::DuplicateOrderId(order_id));
            }
            self.order_side.insert(order_id, side);
        }

        let engine = self
            .engines
            .get_mut(&symbol.0)
            .ok_or_else(|| RuntimeError::UnknownSymbol(symbol.clone()))?;

        let (reports, trades) = engine
            .process_event_with_trades(InputEvent::NewOrder(NewOrderEvent { seq, order }), ts_ns);

        self.apply_owned_fills(&trades)?;
        let _ = self.risk.check_post_trade_loss(&self.portfolio);

        let fill_events: Vec<StrategyEvent> = trades
            .iter()
            .filter_map(|trade| self.fills_for_owned(trade))
            .flatten()
            .collect();

        let accepted = reports.iter().any(|r| {
            matches!(
                r,
                ExecutionReport::Accepted {
                    order_id: id
                } if *id == order_id
            )
        });

        self.emit_reports(seq, ts_ns, reports, trades.clone(), outputs);

        if mark_owned && accepted {
            self.dispatch_strategies(
                seq,
                ts_ns,
                &StrategyEvent::OrderAccepted {
                    order_id,
                    symbol: symbol.clone(),
                    side,
                    price: order_price,
                    qty: order_qty,
                },
                Some(&symbol),
                outputs,
            )?;
        }

        for trade in &trades {
            self.dispatch_strategies(
                seq,
                ts_ns,
                &StrategyEvent::Trade {
                    trade: trade.clone(),
                },
                Some(&symbol),
                outputs,
            )?;
        }
        for fill in fill_events {
            self.dispatch_strategies(seq, ts_ns, &fill, Some(&symbol), outputs)?;
        }

        Ok(())
    }

    fn fills_for_owned(&self, trade: &Trade) -> Option<Vec<StrategyEvent>> {
        let mut events = Vec::new();
        if self.owned_orders.contains(&trade.taker_order_id) {
            events.push(StrategyEvent::Fill {
                order_id: trade.taker_order_id,
                symbol: trade.symbol.clone(),
                side: trade.aggressor_side,
                price: trade.price,
                qty: trade.qty,
            });
        }
        if self.owned_orders.contains(&trade.maker_order_id) {
            let maker_side = match trade.aggressor_side {
                Side::Buy => Side::Sell,
                Side::Sell => Side::Buy,
            };
            events.push(StrategyEvent::Fill {
                order_id: trade.maker_order_id,
                symbol: trade.symbol.clone(),
                side: maker_side,
                price: trade.price,
                qty: trade.qty,
            });
        }
        if events.is_empty() {
            None
        } else {
            Some(events)
        }
    }

    fn apply_owned_fills(&mut self, trades: &[Trade]) -> Result<(), RuntimeError> {
        for trade in trades {
            if self.owned_orders.contains(&trade.taker_order_id) {
                self.portfolio.apply_fill(
                    &trade.symbol,
                    trade.aggressor_side,
                    trade.price,
                    trade.qty,
                )?;
            }
            if self.owned_orders.contains(&trade.maker_order_id) {
                let maker_side = match trade.aggressor_side {
                    Side::Buy => Side::Sell,
                    Side::Sell => Side::Buy,
                };
                self.portfolio
                    .apply_fill(&trade.symbol, maker_side, trade.price, trade.qty)?;
            }
            let _ = self.portfolio.set_mark(&trade.symbol, trade.price);
        }
        Ok(())
    }

    fn dispatch_strategies(
        &mut self,
        seq: SequenceNumber,
        ts_ns: TimestampNanos,
        event: &StrategyEvent,
        symbol: Option<&Symbol>,
        outputs: &mut Vec<RuntimeOutput>,
    ) -> Result<(), RuntimeError> {
        if self.dispatch_depth >= MAX_STRATEGY_DISPATCH_DEPTH {
            return Ok(());
        }
        self.dispatch_depth += 1;

        let mut pending: Vec<(StrategyId, Vec<StrategyCommand>)> = Vec::new();
        let strategy_count = self.strategies.len();
        for idx in 0..strategy_count {
            let id = self.strategies[idx].id();
            let ctx = self.strategy_context(id, seq, ts_ns, symbol);
            let mut commands = self.strategies[idx].on_event(event, &ctx);
            let limit = self.max_strategy_commands_per_event;
            if commands.len() > limit {
                let dropped = (commands.len() - limit) as u64;
                commands.truncate(limit);
                outputs.push(RuntimeOutput {
                    seq,
                    ts_ns,
                    kind: RuntimeOutputKind::StrategyCommandsDropped {
                        strategy_id: id,
                        dropped,
                    },
                });
            }
            if !commands.is_empty() {
                pending.push((id, commands));
            }
        }

        let result = (|| {
            for (strategy_id, commands) in pending {
                for command in commands {
                    self.execute_strategy_command(seq, ts_ns, strategy_id, command, outputs)?;
                }
            }
            Ok(())
        })();

        self.dispatch_depth -= 1;
        result
    }

    fn execute_strategy_command(
        &mut self,
        seq: SequenceNumber,
        ts_ns: TimestampNanos,
        strategy_id: StrategyId,
        command: StrategyCommand,
        outputs: &mut Vec<RuntimeOutput>,
    ) -> Result<(), RuntimeError> {
        match command {
            StrategyCommand::PlaceOrder {
                symbol,
                side,
                order_type,
                price,
                qty,
            } => {
                let order_id = self.alloc_order_id();
                let order = Order {
                    order_id,
                    symbol,
                    side,
                    order_type,
                    price,
                    qty,
                    timestamp_ns: ts_ns,
                    strategy_id: Some(strategy_id),
                };
                self.submit_order(seq, ts_ns, order, true, outputs)?;
            }
            StrategyCommand::Cancel { order_id, symbol } => {
                // Do not re-enter strategy book-update dispatch from cancels
                // generated inside an existing strategy dispatch.
                self.execute_cancel(seq, ts_ns, order_id, symbol, outputs, false)?;
            }
        }
        Ok(())
    }

    fn alloc_order_id(&mut self) -> OrderId {
        let id = self.next_order_id;
        self.next_order_id = self.next_order_id.saturating_add(1);
        id
    }

    fn strategy_context(
        &self,
        strategy_id: StrategyId,
        seq: u64,
        ts_ns: TimestampNanos,
        symbol: Option<&Symbol>,
    ) -> StrategyContext {
        let book = match symbol {
            Some(sym) => self
                .engines
                .get(&sym.0)
                .map(|e| e.book().snapshot(usize::MAX)),
            None => None,
        };
        let portfolio = self.portfolio.snapshot().unwrap_or(PortfolioSnapshot {
            cash: self.portfolio.cash(),
            realized_pnl: self.portfolio.realized_pnl(),
            unrealized_pnl: 0,
            equity: self.portfolio.cash(),
            positions: Vec::new(),
        });
        StrategyContext {
            strategy_id,
            seq,
            ts_ns,
            portfolio,
            book,
            risk_killed: self.risk.is_killed(),
        }
    }

    fn emit_reports(
        &self,
        seq: SequenceNumber,
        ts_ns: TimestampNanos,
        reports: Vec<ExecutionReport>,
        trades: Vec<Trade>,
        outputs: &mut Vec<RuntimeOutput>,
    ) {
        let mut trades = trades.into_iter();
        for report in reports {
            let kind = match report {
                ExecutionReport::Accepted { order_id } => {
                    Some(RuntimeOutputKind::Accepted { order_id })
                }
                ExecutionReport::Filled { .. } | ExecutionReport::PartiallyFilled { .. } => {
                    Some(RuntimeOutputKind::Trade {
                        trade: trades.next().expect("each fill report must have one trade"),
                    })
                }
                ExecutionReport::Rested { .. } => None,
                ExecutionReport::Cancelled { order_id } => {
                    Some(RuntimeOutputKind::Cancelled { order_id })
                }
                ExecutionReport::Expired {
                    order_id,
                    remaining,
                } => Some(RuntimeOutputKind::Expired {
                    order_id,
                    remaining,
                }),
                ExecutionReport::Rejected { order_id, reason } => {
                    Some(RuntimeOutputKind::Rejected {
                        order_id: Some(order_id),
                        reason,
                    })
                }
            };
            if let Some(kind) = kind {
                outputs.push(RuntimeOutput { seq, ts_ns, kind });
            }
        }
    }

    fn validate_sequences(&self, events: &[RuntimeEvent]) -> Result<(), RuntimeError> {
        let mut previous = self.last_seq;
        for event in events {
            let seq = event_seq(event);
            if let Some(prev) = previous {
                if seq == prev {
                    return Err(RuntimeError::DuplicateSequence { seq });
                }
                if seq < prev {
                    return Err(RuntimeError::OutOfOrderSequence {
                        previous: prev,
                        seq,
                    });
                }
            }
            previous = Some(seq);
        }
        Ok(())
    }

    fn snapshot_result(&self, outputs: Vec<RuntimeOutput>) -> Result<RuntimeResult, RuntimeError> {
        let mut books: Vec<BookSnapshot> = self
            .engines
            .values()
            .map(|e| e.book().snapshot(usize::MAX))
            .collect();
        books.sort_by(|a, b| a.symbol.0.cmp(&b.symbol.0));
        Ok(RuntimeResult {
            outputs,
            portfolio: self.portfolio.snapshot()?,
            books,
        })
    }
}

fn event_seq(event: &RuntimeEvent) -> SequenceNumber {
    match event {
        RuntimeEvent::NewOrder { seq, .. }
        | RuntimeEvent::Cancel { seq, .. }
        | RuntimeEvent::Timer { seq, .. } => *seq,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OrderType, PriceTicks};

    struct EchoBuyStrategy {
        id: StrategyId,
        fired: bool,
    }

    impl Strategy for EchoBuyStrategy {
        fn id(&self) -> StrategyId {
            self.id
        }

        fn name(&self) -> &str {
            "echo_buy"
        }

        fn on_event(
            &mut self,
            event: &StrategyEvent,
            ctx: &StrategyContext,
        ) -> Vec<StrategyCommand> {
            if ctx.risk_killed {
                return Vec::new();
            }
            if self.fired {
                return Vec::new();
            }
            if let StrategyEvent::BookUpdate { symbol, .. } = event {
                self.fired = true;
                return vec![StrategyCommand::PlaceOrder {
                    symbol: symbol.clone(),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    price: Some(PriceTicks(100)),
                    qty: Qty(1),
                }];
            }
            Vec::new()
        }
    }

    fn sym() -> Symbol {
        Symbol("AAPL".into())
    }

    fn ext_sell(seq: u64, id: u64, px: i64, qty: u64) -> RuntimeEvent {
        RuntimeEvent::NewOrder {
            seq,
            ts_ns: seq * 100,
            order: Order {
                order_id: id,
                symbol: sym(),
                side: Side::Sell,
                order_type: OrderType::Limit,
                price: Some(PriceTicks(px)),
                qty: Qty(qty),
                timestamp_ns: seq * 10,
                strategy_id: None,
            },
        }
    }

    #[test]
    fn strategy_receives_events_and_orders_pass_risk() {
        let mut rt = Runtime::new(vec![sym()], RuntimeConfig::default());
        rt.add_strategy(Box::new(EchoBuyStrategy {
            id: 7,
            fired: false,
        }));

        let result = rt.process_events(vec![ext_sell(1, 1, 100, 5)]).unwrap();

        assert!(result.outputs.iter().any(|o| matches!(
            o.kind,
            RuntimeOutputKind::Accepted {
                order_id: 1_000_000
            }
        )));
        // Strategy buy at 100 crosses the resting sell → immediate fill.
        assert_eq!(rt.portfolio().position_qty(&sym()), 1);
        assert!(result
            .outputs
            .iter()
            .any(|o| matches!(o.kind, RuntimeOutputKind::Trade { .. })));
    }

    #[test]
    fn deterministic_byte_identical_outputs() {
        fn run() -> String {
            let mut rt = Runtime::new(vec![sym()], RuntimeConfig::default());
            rt.add_strategy(Box::new(EchoBuyStrategy {
                id: 7,
                fired: false,
            }));
            let result = rt
                .process_events(vec![
                    ext_sell(1, 1, 100, 5),
                    RuntimeEvent::NewOrder {
                        seq: 2,
                        ts_ns: 200,
                        order: Order {
                            order_id: 2,
                            symbol: sym(),
                            side: Side::Buy,
                            order_type: OrderType::Limit,
                            price: Some(PriceTicks(100)),
                            qty: Qty(1),
                            timestamp_ns: 20,
                            strategy_id: None,
                        },
                    },
                ])
                .unwrap();
            result
                .outputs
                .iter()
                .map(|o| serde_json::to_string(o).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
        }

        assert_eq!(run(), run());
    }

    #[test]
    fn strategy_fill_updates_portfolio() {
        let mut rt = Runtime::new(vec![sym()], RuntimeConfig::default());
        rt.add_strategy(Box::new(EchoBuyStrategy {
            id: 7,
            fired: false,
        }));

        // Resting ask at 100 qty 5; strategy places buy at 100 qty 1 → fills.
        // Actually strategy places buy which rests if we put ask first...
        // Order: external sell rests, strategy buy at 100 crosses → fill.
        let result = rt.process_events(vec![ext_sell(1, 1, 100, 5)]).unwrap();
        assert!(result
            .outputs
            .iter()
            .any(|o| { matches!(o.kind, RuntimeOutputKind::Trade { .. }) }));
        assert_eq!(rt.portfolio().position_qty(&sym()), 1);
    }

    #[test]
    fn command_budget_drops_excess() {
        struct SpamStrategy;
        impl Strategy for SpamStrategy {
            fn id(&self) -> StrategyId {
                1
            }
            fn name(&self) -> &str {
                "spam"
            }
            fn on_event(
                &mut self,
                event: &StrategyEvent,
                _ctx: &StrategyContext,
            ) -> Vec<StrategyCommand> {
                if matches!(event, StrategyEvent::Timer { .. }) {
                    (0..20)
                        .map(|_| StrategyCommand::PlaceOrder {
                            symbol: sym(),
                            side: Side::Buy,
                            order_type: OrderType::Limit,
                            price: Some(PriceTicks(50)),
                            qty: Qty(1),
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            }
        }

        let cfg = RuntimeConfig {
            max_strategy_commands_per_event: 3,
            ..RuntimeConfig::default()
        };
        let mut rt = Runtime::new(vec![sym()], cfg);
        rt.add_strategy(Box::new(SpamStrategy));
        let result = rt
            .process_events(vec![RuntimeEvent::Timer {
                seq: 1,
                ts_ns: 1,
                symbol: Some(sym()),
            }])
            .unwrap();
        assert!(result.outputs.iter().any(|o| matches!(
            o.kind,
            RuntimeOutputKind::StrategyCommandsDropped { dropped: 17, .. }
        )));
        let accepted = result
            .outputs
            .iter()
            .filter(|o| matches!(o.kind, RuntimeOutputKind::Accepted { .. }))
            .count();
        assert_eq!(accepted, 3);
    }
}
