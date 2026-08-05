//! Momentum demonstration strategy with a rolling trade-price window.
//!
//! Not a profitability claim.

use std::collections::{BTreeMap, VecDeque};

use crate::{
    strategy::{Strategy, StrategyCommand, StrategyContext, StrategyEvent},
    types::{OrderType, Qty, Side, StrategyId, Symbol},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MomentumConfig {
    pub window_size: usize,
    pub threshold_ticks: i64,
    /// Cooldown measured in external sequence numbers.
    pub cooldown_seqs: u64,
    pub order_qty: u64,
    pub max_abs_position: i64,
}

impl Default for MomentumConfig {
    fn default() -> Self {
        Self {
            window_size: 5,
            threshold_ticks: 10,
            cooldown_seqs: 2,
            order_qty: 1,
            max_abs_position: 5,
        }
    }
}

#[derive(Debug)]
pub struct MomentumStrategy {
    id: StrategyId,
    config: MomentumConfig,
    windows: BTreeMap<String, VecDeque<i64>>,
    last_trade_seq: BTreeMap<String, u64>,
}

impl MomentumStrategy {
    pub fn new(id: StrategyId, config: MomentumConfig) -> Self {
        Self {
            id,
            config,
            windows: BTreeMap::new(),
            last_trade_seq: BTreeMap::new(),
        }
    }

    fn position(ctx: &StrategyContext, symbol: &Symbol) -> i64 {
        ctx.portfolio
            .positions
            .iter()
            .find(|p| p.symbol == *symbol)
            .map(|p| p.qty)
            .unwrap_or(0)
    }

    fn signal(&self, symbol: &str) -> Option<i64> {
        let w = self.windows.get(symbol)?;
        if w.len() < self.config.window_size {
            return None;
        }
        let first = *w.front()?;
        let last = *w.back()?;
        Some(last - first)
    }
}

impl Strategy for MomentumStrategy {
    fn id(&self) -> StrategyId {
        self.id
    }

    fn name(&self) -> &str {
        "momentum"
    }

    fn on_event(&mut self, event: &StrategyEvent, ctx: &StrategyContext) -> Vec<StrategyCommand> {
        if ctx.risk_killed {
            return Vec::new();
        }

        let StrategyEvent::Trade { trade } = event else {
            return Vec::new();
        };

        let symbol = trade.symbol.clone();
        let window = self.windows.entry(symbol.0.clone()).or_default();
        window.push_back(trade.price.0);
        while window.len() > self.config.window_size {
            window.pop_front();
        }

        if let Some(last) = self.last_trade_seq.get(&symbol.0) {
            if ctx.seq.saturating_sub(*last) < self.config.cooldown_seqs {
                return Vec::new();
            }
        }

        let Some(delta) = self.signal(&symbol.0) else {
            return Vec::new();
        };

        let pos = Self::position(ctx, &symbol);
        let mut cmds = Vec::new();

        if delta >= self.config.threshold_ticks {
            if pos < 0 {
                cmds.push(StrategyCommand::PlaceOrder {
                    symbol: symbol.clone(),
                    side: Side::Buy,
                    order_type: OrderType::Market,
                    price: None,
                    qty: Qty(pos.unsigned_abs()),
                });
            }
            if pos < self.config.max_abs_position {
                let room = (self.config.max_abs_position - pos.max(0)) as u64;
                let qty = self.config.order_qty.min(room);
                if qty > 0 {
                    cmds.push(StrategyCommand::PlaceOrder {
                        symbol: symbol.clone(),
                        side: Side::Buy,
                        order_type: OrderType::Market,
                        price: None,
                        qty: Qty(qty),
                    });
                }
            }
            self.last_trade_seq.insert(symbol.0.clone(), ctx.seq);
        } else if delta <= -self.config.threshold_ticks {
            if pos > 0 {
                cmds.push(StrategyCommand::PlaceOrder {
                    symbol: symbol.clone(),
                    side: Side::Sell,
                    order_type: OrderType::Market,
                    price: None,
                    qty: Qty(pos as u64),
                });
            }
            if pos > -self.config.max_abs_position {
                let room = (self.config.max_abs_position + pos.min(0)) as u64;
                let qty = self.config.order_qty.min(room);
                if qty > 0 {
                    cmds.push(StrategyCommand::PlaceOrder {
                        symbol: symbol.clone(),
                        side: Side::Sell,
                        order_type: OrderType::Market,
                        price: None,
                        qty: Qty(qty),
                    });
                }
            }
            self.last_trade_seq.insert(symbol.0.clone(), ctx.seq);
        }

        cmds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{events::Trade, portfolio::PortfolioSnapshot, types::PriceTicks};

    fn ctx(seq: u64) -> StrategyContext {
        StrategyContext {
            strategy_id: 1,
            seq,
            ts_ns: seq,
            portfolio: PortfolioSnapshot {
                cash: 1_000_000,
                realized_pnl: 0,
                unrealized_pnl: 0,
                equity: 1_000_000,
                positions: vec![],
            },
            book: None,
            risk_killed: false,
        }
    }

    fn trade(px: i64) -> StrategyEvent {
        StrategyEvent::Trade {
            trade: Trade {
                symbol: Symbol("AAPL".into()),
                taker_order_id: 1,
                maker_order_id: 2,
                price: PriceTicks(px),
                qty: Qty(1),
                aggressor_side: Side::Buy,
                timestamp_ns: 1,
            },
        }
    }

    #[test]
    fn emits_buy_after_upward_move() {
        let mut s = MomentumStrategy::new(
            1,
            MomentumConfig {
                window_size: 3,
                threshold_ticks: 10,
                cooldown_seqs: 0,
                order_qty: 1,
                max_abs_position: 5,
            },
        );
        assert!(s.on_event(&trade(100), &ctx(1)).is_empty());
        assert!(s.on_event(&trade(105), &ctx(2)).is_empty());
        let cmds = s.on_event(&trade(120), &ctx(3));
        assert!(cmds.iter().any(|c| matches!(
            c,
            StrategyCommand::PlaceOrder {
                side: Side::Buy,
                order_type: OrderType::Market,
                ..
            }
        )));
    }

    #[test]
    fn cooldown_suppresses_repeat() {
        let mut s = MomentumStrategy::new(
            1,
            MomentumConfig {
                window_size: 2,
                threshold_ticks: 5,
                cooldown_seqs: 10,
                order_qty: 1,
                max_abs_position: 5,
            },
        );
        let _ = s.on_event(&trade(100), &ctx(1));
        let first = s.on_event(&trade(110), &ctx(2));
        assert!(!first.is_empty());
        let second = s.on_event(&trade(120), &ctx(3));
        assert!(second.is_empty());
    }
}
