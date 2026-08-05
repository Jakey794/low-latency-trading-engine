//! Inventory-aware market-making demonstration strategy.
//!
//! Places symmetric limit quotes around mid with optional inventory skew.
//! Not a profitability claim.

use std::collections::BTreeMap;

use crate::{
    book::BookSnapshot,
    strategy::{Strategy, StrategyCommand, StrategyContext, StrategyEvent},
    types::{OrderId, OrderType, PriceTicks, Qty, Side, StrategyId, Symbol},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketMakingConfig {
    pub half_spread_ticks: i64,
    pub quote_size: u64,
    pub max_active_orders: usize,
    /// Inventory skew in ticks per unit of position (shifts quotes).
    pub inventory_skew_ticks_per_unit: i64,
    pub max_inventory: i64,
}

impl Default for MarketMakingConfig {
    fn default() -> Self {
        Self {
            half_spread_ticks: 5,
            quote_size: 1,
            max_active_orders: 2,
            inventory_skew_ticks_per_unit: 1,
            max_inventory: 20,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct QuotePair {
    bid_id: Option<OrderId>,
    ask_id: Option<OrderId>,
    bid_px: Option<PriceTicks>,
    ask_px: Option<PriceTicks>,
}

#[derive(Debug)]
pub struct MarketMakingStrategy {
    id: StrategyId,
    config: MarketMakingConfig,
    quotes: BTreeMap<String, QuotePair>,
}

impl MarketMakingStrategy {
    pub fn new(id: StrategyId, config: MarketMakingConfig) -> Self {
        Self {
            id,
            config,
            quotes: BTreeMap::new(),
        }
    }

    fn mid(book: &BookSnapshot) -> Option<i64> {
        match (book.bids.first(), book.asks.first()) {
            (Some(b), Some(a)) => Some((b.price.0 + a.price.0) / 2),
            (Some(b), None) => Some(b.price.0),
            (None, Some(a)) => Some(a.price.0),
            (None, None) => None,
        }
    }

    fn desired(&self, mid: i64, inventory: i64) -> (PriceTicks, PriceTicks) {
        let skew = inventory.saturating_mul(self.config.inventory_skew_ticks_per_unit);
        let bid = (mid - self.config.half_spread_ticks - skew).max(1);
        let ask = (mid + self.config.half_spread_ticks - skew).max(bid + 1);
        (PriceTicks(bid), PriceTicks(ask))
    }

    fn inventory(ctx: &StrategyContext, symbol: &Symbol) -> i64 {
        ctx.portfolio
            .positions
            .iter()
            .find(|p| p.symbol == *symbol)
            .map(|p| p.qty)
            .unwrap_or(0)
    }

    fn cancel_pair(&mut self, symbol: &Symbol) -> Vec<StrategyCommand> {
        let mut cmds = Vec::new();
        if let Some(q) = self.quotes.remove(&symbol.0) {
            if let Some(order_id) = q.bid_id {
                cmds.push(StrategyCommand::Cancel {
                    order_id,
                    symbol: symbol.clone(),
                });
            }
            if let Some(order_id) = q.ask_id {
                cmds.push(StrategyCommand::Cancel {
                    order_id,
                    symbol: symbol.clone(),
                });
            }
        }
        cmds
    }

    fn refresh(
        &mut self,
        symbol: &Symbol,
        book: &BookSnapshot,
        ctx: &StrategyContext,
    ) -> Vec<StrategyCommand> {
        if ctx.risk_killed {
            return self.cancel_pair(symbol);
        }
        let Some(mid) = Self::mid(book) else {
            return Vec::new();
        };
        let inv = Self::inventory(ctx, symbol);
        if (inv.unsigned_abs() as i64) >= self.config.max_inventory {
            return self.cancel_pair(symbol);
        }

        let (want_bid, want_ask) = self.desired(mid, inv);
        let mut cmds = Vec::new();
        let q = self.quotes.entry(symbol.0.clone()).or_default();

        if q.bid_px != Some(want_bid) {
            if let Some(order_id) = q.bid_id.take() {
                cmds.push(StrategyCommand::Cancel {
                    order_id,
                    symbol: symbol.clone(),
                });
            }
            q.bid_px = None;
        }
        if q.ask_px != Some(want_ask) {
            if let Some(order_id) = q.ask_id.take() {
                cmds.push(StrategyCommand::Cancel {
                    order_id,
                    symbol: symbol.clone(),
                });
            }
            q.ask_px = None;
        }

        let mut active = usize::from(q.bid_id.is_some()) + usize::from(q.ask_id.is_some());

        if q.bid_id.is_none() && q.bid_px.is_none() && active < self.config.max_active_orders {
            cmds.push(StrategyCommand::PlaceOrder {
                symbol: symbol.clone(),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: Some(want_bid),
                qty: Qty(self.config.quote_size),
            });
            q.bid_px = Some(want_bid);
            active += 1;
        }
        if q.ask_id.is_none() && q.ask_px.is_none() && active < self.config.max_active_orders {
            cmds.push(StrategyCommand::PlaceOrder {
                symbol: symbol.clone(),
                side: Side::Sell,
                order_type: OrderType::Limit,
                price: Some(want_ask),
                qty: Qty(self.config.quote_size),
            });
            q.ask_px = Some(want_ask);
        }

        cmds
    }
}

impl Strategy for MarketMakingStrategy {
    fn id(&self) -> StrategyId {
        self.id
    }

    fn name(&self) -> &str {
        "market_making"
    }

    fn on_event(&mut self, event: &StrategyEvent, ctx: &StrategyContext) -> Vec<StrategyCommand> {
        match event {
            StrategyEvent::BookUpdate { symbol, book } => self.refresh(symbol, book, ctx),
            StrategyEvent::Timer { .. } => {
                if let Some(book) = &ctx.book {
                    let symbol = book.symbol.clone();
                    self.refresh(&symbol, book, ctx)
                } else {
                    Vec::new()
                }
            }
            StrategyEvent::OrderAccepted {
                order_id,
                symbol,
                side,
                price,
                ..
            } => {
                let q = self.quotes.entry(symbol.0.clone()).or_default();
                match side {
                    Side::Buy => {
                        q.bid_id = Some(*order_id);
                        if let Some(p) = price {
                            q.bid_px = Some(*p);
                        }
                    }
                    Side::Sell => {
                        q.ask_id = Some(*order_id);
                        if let Some(p) = price {
                            q.ask_px = Some(*p);
                        }
                    }
                }
                Vec::new()
            }
            StrategyEvent::Fill {
                order_id, symbol, ..
            } => {
                if let Some(q) = self.quotes.get_mut(&symbol.0) {
                    if q.bid_id == Some(*order_id) {
                        q.bid_id = None;
                        q.bid_px = None;
                    }
                    if q.ask_id == Some(*order_id) {
                        q.ask_id = None;
                        q.ask_px = None;
                    }
                }
                Vec::new()
            }
            StrategyEvent::RiskRejection { .. } | StrategyEvent::Deactivated => {
                let symbols: Vec<_> = self.quotes.keys().cloned().collect();
                let mut cmds = Vec::new();
                for s in symbols {
                    cmds.extend(self.cancel_pair(&Symbol(s)));
                }
                cmds
            }
            StrategyEvent::Activated | StrategyEvent::Trade { .. } => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::PortfolioSnapshot;

    fn ctx(killed: bool) -> StrategyContext {
        StrategyContext {
            strategy_id: 1,
            seq: 1,
            ts_ns: 1,
            portfolio: PortfolioSnapshot {
                cash: 1_000_000,
                realized_pnl: 0,
                unrealized_pnl: 0,
                equity: 1_000_000,
                positions: vec![],
            },
            book: None,
            risk_killed: killed,
        }
    }

    #[test]
    fn quotes_around_mid_with_spread() {
        let mut s = MarketMakingStrategy::new(1, MarketMakingConfig::default());
        let book = BookSnapshot {
            symbol: Symbol("AAPL".into()),
            bids: vec![crate::book::PriceLevelSnapshot {
                price: PriceTicks(100),
                total_qty: Qty(5),
                order_count: 1,
                order_ids: vec![1],
            }],
            asks: vec![crate::book::PriceLevelSnapshot {
                price: PriceTicks(110),
                total_qty: Qty(5),
                order_count: 1,
                order_ids: vec![2],
            }],
        };
        let cmds = s.on_event(
            &StrategyEvent::BookUpdate {
                symbol: Symbol("AAPL".into()),
                book,
            },
            &ctx(false),
        );
        assert!(cmds.iter().any(|c| matches!(
            c,
            StrategyCommand::PlaceOrder {
                side: Side::Buy,
                price: Some(PriceTicks(100)),
                ..
            }
        )));
        assert!(cmds.iter().any(|c| matches!(
            c,
            StrategyCommand::PlaceOrder {
                side: Side::Sell,
                price: Some(PriceTicks(110)),
                ..
            }
        )));
    }

    #[test]
    fn suppresses_when_killed() {
        let mut s = MarketMakingStrategy::new(1, MarketMakingConfig::default());
        s.quotes.insert(
            "AAPL".into(),
            QuotePair {
                bid_id: Some(10),
                ask_id: Some(11),
                bid_px: Some(PriceTicks(100)),
                ask_px: Some(PriceTicks(110)),
            },
        );
        let cmds = s.on_event(
            &StrategyEvent::BookUpdate {
                symbol: Symbol("AAPL".into()),
                book: BookSnapshot {
                    symbol: Symbol("AAPL".into()),
                    bids: vec![crate::book::PriceLevelSnapshot {
                        price: PriceTicks(100),
                        total_qty: Qty(1),
                        order_count: 1,
                        order_ids: vec![1],
                    }],
                    asks: vec![crate::book::PriceLevelSnapshot {
                        price: PriceTicks(110),
                        total_qty: Qty(1),
                        order_count: 1,
                        order_ids: vec![2],
                    }],
                },
            },
            &ctx(true),
        );
        assert!(cmds
            .iter()
            .all(|c| matches!(c, StrategyCommand::Cancel { .. })));
    }
}
