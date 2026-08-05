//! Portfolio accounting with average-cost positions and integer P&L.
//!
//! Prices and quantities remain integer ticks / units. Cash, notional, and P&L
//! use checked `i128` arithmetic. Snapshots sort positions by symbol for
//! deterministic output.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    events::Trade,
    types::{PriceTicks, Qty, Side, Symbol},
};

/// Signed position quantity (positive = long, negative = short).
pub type PositionQty = i64;

/// Cash, notional, and P&L amounts in tick-notional units (`price * qty`).
pub type Money = i128;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PortfolioError {
    #[error("arithmetic overflow in portfolio accounting")]
    Overflow,
    #[error("fill quantity must be positive")]
    InvalidQuantity,
    #[error("price must be non-negative for portfolio accounting")]
    InvalidPrice,
    #[error("mark price missing for symbol {0:?}")]
    MissingMark(Symbol),
}

/// Per-symbol average-cost position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    /// Signed quantity: positive long, negative short.
    pub qty: PositionQty,
    /// Average entry price in ticks (always non-negative when qty != 0).
    pub avg_cost: PriceTicks,
}

/// Sorted, read-only view of one position for external output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionSnapshot {
    pub symbol: Symbol,
    pub qty: PositionQty,
    pub avg_cost: PriceTicks,
    pub mark: Option<PriceTicks>,
    pub unrealized_pnl: Money,
    pub market_value: Money,
}

/// Deterministic portfolio snapshot with positions sorted by symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortfolioSnapshot {
    pub cash: Money,
    pub realized_pnl: Money,
    pub unrealized_pnl: Money,
    pub equity: Money,
    pub positions: Vec<PositionSnapshot>,
}

/// Portfolio state: cash, per-symbol positions, marks, and cumulative realized P&L.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Portfolio {
    cash: Money,
    realized_pnl: Money,
    positions: BTreeMap<String, Position>,
    marks: BTreeMap<String, PriceTicks>,
}

impl Portfolio {
    /// Create a portfolio with the given starting cash and empty positions.
    pub fn new(starting_cash: Money) -> Self {
        Self {
            cash: starting_cash,
            realized_pnl: 0,
            positions: BTreeMap::new(),
            marks: BTreeMap::new(),
        }
    }

    pub fn cash(&self) -> Money {
        self.cash
    }

    pub fn realized_pnl(&self) -> Money {
        self.realized_pnl
    }

    pub fn position_qty(&self, symbol: &Symbol) -> PositionQty {
        self.positions.get(&symbol.0).map(|p| p.qty).unwrap_or(0)
    }

    pub fn position(&self, symbol: &Symbol) -> Option<&Position> {
        self.positions.get(&symbol.0)
    }

    pub fn mark(&self, symbol: &Symbol) -> Option<PriceTicks> {
        self.marks.get(&symbol.0).copied()
    }

    /// Set the mark price used for unrealized P&L and equity.
    pub fn set_mark(&mut self, symbol: &Symbol, price: PriceTicks) -> Result<(), PortfolioError> {
        if price.0 < 0 {
            return Err(PortfolioError::InvalidPrice);
        }
        self.marks.insert(symbol.0.clone(), price);
        Ok(())
    }

    /// Apply a fill for this portfolio's side of a trade.
    ///
    /// `side` is the portfolio's trade side: [`Side::Buy`] increases (or covers)
    /// position; [`Side::Sell`] decreases (or opens short) position.
    pub fn apply_fill(
        &mut self,
        symbol: &Symbol,
        side: Side,
        price: PriceTicks,
        qty: Qty,
    ) -> Result<(), PortfolioError> {
        if qty.0 == 0 {
            return Err(PortfolioError::InvalidQuantity);
        }
        if price.0 < 0 {
            return Err(PortfolioError::InvalidPrice);
        }

        let fill_qty = i64::try_from(qty.0).map_err(|_| PortfolioError::Overflow)?;
        let signed_delta = match side {
            Side::Buy => fill_qty,
            Side::Sell => -fill_qty,
        };

        let notional = checked_notional(price, qty)?;
        self.cash = match side {
            Side::Buy => self
                .cash
                .checked_sub(notional)
                .ok_or(PortfolioError::Overflow)?,
            Side::Sell => self
                .cash
                .checked_add(notional)
                .ok_or(PortfolioError::Overflow)?,
        };

        let key = symbol.0.clone();
        let current = self.positions.get(&key).cloned();
        let (new_position, realized_delta) = match current {
            None | Some(Position { qty: 0, .. }) => (
                Position {
                    qty: signed_delta,
                    avg_cost: price,
                },
                0,
            ),
            Some(pos) => apply_position_delta(pos, signed_delta, price)?,
        };

        self.realized_pnl = self
            .realized_pnl
            .checked_add(realized_delta)
            .ok_or(PortfolioError::Overflow)?;

        if new_position.qty == 0 {
            self.positions.remove(&key);
        } else {
            self.positions.insert(key, new_position);
        }

        self.marks.insert(symbol.0.clone(), price);
        Ok(())
    }

    /// Apply a [`Trade`] as the given portfolio side (buyer or seller).
    pub fn apply_trade_as(&mut self, trade: &Trade, as_side: Side) -> Result<(), PortfolioError> {
        self.apply_fill(&trade.symbol, as_side, trade.price, trade.qty)
    }

    /// Total unrealized P&L across all positions using current marks.
    pub fn unrealized_pnl(&self) -> Result<Money, PortfolioError> {
        let mut total: Money = 0;
        for (sym, pos) in &self.positions {
            let mark = self
                .marks
                .get(sym)
                .copied()
                .ok_or_else(|| PortfolioError::MissingMark(Symbol(sym.clone())))?;
            let ur = position_unrealized(pos, mark)?;
            total = total.checked_add(ur).ok_or(PortfolioError::Overflow)?;
        }
        Ok(total)
    }

    /// Equity = cash + Σ(mark × signed_qty).
    pub fn equity(&self) -> Result<Money, PortfolioError> {
        let mut equity = self.cash;
        for (sym, pos) in &self.positions {
            let mark = self
                .marks
                .get(sym)
                .copied()
                .ok_or_else(|| PortfolioError::MissingMark(Symbol(sym.clone())))?;
            let mv = market_value(pos.qty, mark)?;
            equity = equity.checked_add(mv).ok_or(PortfolioError::Overflow)?;
        }
        Ok(equity)
    }

    /// Deterministic snapshot with positions sorted by symbol key.
    pub fn snapshot(&self) -> Result<PortfolioSnapshot, PortfolioError> {
        let mut positions = Vec::with_capacity(self.positions.len());
        let mut unrealized_total: Money = 0;

        for (sym, pos) in &self.positions {
            let mark = self
                .marks
                .get(sym)
                .copied()
                .ok_or_else(|| PortfolioError::MissingMark(Symbol(sym.clone())))?;
            let unrealized = position_unrealized(pos, mark)?;
            let mv = market_value(pos.qty, mark)?;
            unrealized_total = unrealized_total
                .checked_add(unrealized)
                .ok_or(PortfolioError::Overflow)?;
            positions.push(PositionSnapshot {
                symbol: Symbol(sym.clone()),
                qty: pos.qty,
                avg_cost: pos.avg_cost,
                mark: Some(mark),
                unrealized_pnl: unrealized,
                market_value: mv,
            });
        }

        Ok(PortfolioSnapshot {
            cash: self.cash,
            realized_pnl: self.realized_pnl,
            unrealized_pnl: unrealized_total,
            equity: self.equity()?,
            positions,
        })
    }

    /// Symbols with non-zero positions, in sorted order.
    pub fn symbols(&self) -> impl Iterator<Item = Symbol> + '_ {
        self.positions.keys().map(|s| Symbol(s.clone()))
    }
}

fn checked_notional(price: PriceTicks, qty: Qty) -> Result<Money, PortfolioError> {
    Money::from(price.0)
        .checked_mul(Money::from(qty.0))
        .ok_or(PortfolioError::Overflow)
}

fn market_value(qty: PositionQty, mark: PriceTicks) -> Result<Money, PortfolioError> {
    Money::from(qty)
        .checked_mul(Money::from(mark.0))
        .ok_or(PortfolioError::Overflow)
}

fn position_unrealized(pos: &Position, mark: PriceTicks) -> Result<Money, PortfolioError> {
    if pos.qty == 0 {
        return Ok(0);
    }
    let qty_abs = Money::from(pos.qty.unsigned_abs());
    let diff = Money::from(mark.0)
        .checked_sub(Money::from(pos.avg_cost.0))
        .ok_or(PortfolioError::Overflow)?;
    let signed = if pos.qty > 0 {
        diff
    } else {
        diff.checked_neg().ok_or(PortfolioError::Overflow)?
    };
    signed.checked_mul(qty_abs).ok_or(PortfolioError::Overflow)
}

/// Apply a signed quantity delta to an existing position.
///
/// Returns the updated position and realized P&L from any closed quantity.
fn apply_position_delta(
    pos: Position,
    signed_delta: PositionQty,
    price: PriceTicks,
) -> Result<(Position, Money), PortfolioError> {
    let old_qty = pos.qty;
    let new_qty = old_qty
        .checked_add(signed_delta)
        .ok_or(PortfolioError::Overflow)?;

    // Same direction or flat → open / add: weighted average cost.
    if old_qty == 0 || old_qty.signum() == signed_delta.signum() {
        let old_abs = Money::from(old_qty.unsigned_abs());
        let add_abs = Money::from(signed_delta.unsigned_abs());
        let old_cost = old_abs
            .checked_mul(Money::from(pos.avg_cost.0))
            .ok_or(PortfolioError::Overflow)?;
        let add_cost = add_abs
            .checked_mul(Money::from(price.0))
            .ok_or(PortfolioError::Overflow)?;
        let total_cost = old_cost
            .checked_add(add_cost)
            .ok_or(PortfolioError::Overflow)?;
        let new_abs = old_abs
            .checked_add(add_abs)
            .ok_or(PortfolioError::Overflow)?;
        let avg = if new_abs == 0 {
            0
        } else {
            // Integer average cost in ticks.
            i64::try_from(total_cost / new_abs).map_err(|_| PortfolioError::Overflow)?
        };
        return Ok((
            Position {
                qty: new_qty,
                avg_cost: PriceTicks(avg),
            },
            0,
        ));
    }

    // Opposite direction: reduce / close / possibly flip.
    let close_qty = old_qty.unsigned_abs().min(signed_delta.unsigned_abs());
    let close_qty_i = i64::try_from(close_qty).map_err(|_| PortfolioError::Overflow)?;
    let realized = realized_on_close(old_qty, pos.avg_cost, price, close_qty_i)?;

    if new_qty == 0 {
        return Ok((
            Position {
                qty: 0,
                avg_cost: PriceTicks(0),
            },
            realized,
        ));
    }

    if new_qty.signum() == old_qty.signum() {
        // Partial close: keep average cost.
        return Ok((
            Position {
                qty: new_qty,
                avg_cost: pos.avg_cost,
            },
            realized,
        ));
    }

    // Crossed through zero: remainder opens a new position at the fill price.
    Ok((
        Position {
            qty: new_qty,
            avg_cost: price,
        },
        realized,
    ))
}

fn realized_on_close(
    old_qty: PositionQty,
    avg_cost: PriceTicks,
    exit_price: PriceTicks,
    close_qty: PositionQty,
) -> Result<Money, PortfolioError> {
    let qty = Money::from(close_qty);
    let exit = Money::from(exit_price.0);
    let entry = Money::from(avg_cost.0);
    if old_qty > 0 {
        // Closing a long: (exit - entry) * qty
        exit.checked_sub(entry)
            .and_then(|d| d.checked_mul(qty))
            .ok_or(PortfolioError::Overflow)
    } else {
        // Covering a short: (entry - exit) * qty
        entry
            .checked_sub(exit)
            .and_then(|d| d.checked_mul(qty))
            .ok_or(PortfolioError::Overflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(s: &str) -> Symbol {
        Symbol(s.to_owned())
    }

    fn px(p: i64) -> PriceTicks {
        PriceTicks(p)
    }

    fn qty(q: u64) -> Qty {
        Qty(q)
    }

    #[test]
    fn open_and_add_to_long() {
        let mut p = Portfolio::new(1_000_000);
        p.apply_fill(&sym("AAPL"), Side::Buy, px(100), qty(10))
            .unwrap();
        assert_eq!(p.position_qty(&sym("AAPL")), 10);
        assert_eq!(p.position(&sym("AAPL")).unwrap().avg_cost, px(100));
        assert_eq!(p.cash(), 1_000_000 - 1_000);

        p.apply_fill(&sym("AAPL"), Side::Buy, px(110), qty(10))
            .unwrap();
        assert_eq!(p.position_qty(&sym("AAPL")), 20);
        assert_eq!(p.position(&sym("AAPL")).unwrap().avg_cost, px(105));
        assert_eq!(p.cash(), 1_000_000 - 1_000 - 1_100);
        assert_eq!(p.realized_pnl(), 0);
    }

    #[test]
    fn partially_and_fully_close_long() {
        let mut p = Portfolio::new(1_000_000);
        p.apply_fill(&sym("AAPL"), Side::Buy, px(100), qty(10))
            .unwrap();
        p.apply_fill(&sym("AAPL"), Side::Sell, px(120), qty(4))
            .unwrap();
        assert_eq!(p.position_qty(&sym("AAPL")), 6);
        assert_eq!(p.realized_pnl(), 4 * (120 - 100));
        assert_eq!(p.position(&sym("AAPL")).unwrap().avg_cost, px(100));

        p.apply_fill(&sym("AAPL"), Side::Sell, px(130), qty(6))
            .unwrap();
        assert_eq!(p.position_qty(&sym("AAPL")), 0);
        assert!(p.position(&sym("AAPL")).is_none());
        assert_eq!(p.realized_pnl(), 4 * 20 + 6 * 30);
    }

    #[test]
    fn open_and_add_to_short() {
        let mut p = Portfolio::new(1_000_000);
        p.apply_fill(&sym("AAPL"), Side::Sell, px(100), qty(10))
            .unwrap();
        assert_eq!(p.position_qty(&sym("AAPL")), -10);
        assert_eq!(p.cash(), 1_000_000 + 1_000);

        p.apply_fill(&sym("AAPL"), Side::Sell, px(90), qty(10))
            .unwrap();
        assert_eq!(p.position_qty(&sym("AAPL")), -20);
        assert_eq!(p.position(&sym("AAPL")).unwrap().avg_cost, px(95));
        assert_eq!(p.realized_pnl(), 0);
    }

    #[test]
    fn partially_and_fully_cover_short() {
        let mut p = Portfolio::new(1_000_000);
        p.apply_fill(&sym("AAPL"), Side::Sell, px(100), qty(10))
            .unwrap();
        p.apply_fill(&sym("AAPL"), Side::Buy, px(80), qty(4))
            .unwrap();
        assert_eq!(p.position_qty(&sym("AAPL")), -6);
        assert_eq!(p.realized_pnl(), 4 * (100 - 80));

        p.apply_fill(&sym("AAPL"), Side::Buy, px(70), qty(6))
            .unwrap();
        assert_eq!(p.position_qty(&sym("AAPL")), 0);
        assert_eq!(p.realized_pnl(), 4 * 20 + 6 * 30);
    }

    #[test]
    fn cross_long_to_short() {
        let mut p = Portfolio::new(1_000_000);
        p.apply_fill(&sym("AAPL"), Side::Buy, px(100), qty(10))
            .unwrap();
        p.apply_fill(&sym("AAPL"), Side::Sell, px(110), qty(15))
            .unwrap();
        assert_eq!(p.position_qty(&sym("AAPL")), -5);
        assert_eq!(p.position(&sym("AAPL")).unwrap().avg_cost, px(110));
        assert_eq!(p.realized_pnl(), 10 * (110 - 100));
    }

    #[test]
    fn cross_short_to_long() {
        let mut p = Portfolio::new(1_000_000);
        p.apply_fill(&sym("AAPL"), Side::Sell, px(100), qty(10))
            .unwrap();
        p.apply_fill(&sym("AAPL"), Side::Buy, px(90), qty(15))
            .unwrap();
        assert_eq!(p.position_qty(&sym("AAPL")), 5);
        assert_eq!(p.position(&sym("AAPL")).unwrap().avg_cost, px(90));
        assert_eq!(p.realized_pnl(), 10 * (100 - 90));
    }

    #[test]
    fn realized_pnl_correctness() {
        let mut p = Portfolio::new(0);
        p.apply_fill(&sym("X"), Side::Buy, px(50), qty(2)).unwrap();
        p.apply_fill(&sym("X"), Side::Sell, px(60), qty(2)).unwrap();
        assert_eq!(p.realized_pnl(), 20);
        assert_eq!(p.cash(), 20);
    }

    #[test]
    fn unrealized_pnl_and_equity() {
        let mut p = Portfolio::new(10_000);
        p.apply_fill(&sym("AAPL"), Side::Buy, px(100), qty(10))
            .unwrap();
        p.set_mark(&sym("AAPL"), px(115)).unwrap();
        assert_eq!(p.unrealized_pnl().unwrap(), 10 * 15);
        assert_eq!(p.equity().unwrap(), 10_000 - 1_000 + 10 * 115);

        p.apply_fill(&sym("AAPL"), Side::Sell, px(100), qty(20))
            .unwrap();
        // Closed 10 long @ 100 → realized 0; opened 10 short @ 100
        assert_eq!(p.position_qty(&sym("AAPL")), -10);
        p.set_mark(&sym("AAPL"), px(90)).unwrap();
        assert_eq!(p.unrealized_pnl().unwrap(), 10 * (100 - 90));
    }

    #[test]
    fn cash_correctness_round_trip() {
        let start = 500_000i128;
        let mut p = Portfolio::new(start);
        p.apply_fill(&sym("AAPL"), Side::Buy, px(200), qty(5))
            .unwrap();
        p.apply_fill(&sym("AAPL"), Side::Sell, px(200), qty(5))
            .unwrap();
        assert_eq!(p.cash(), start);
        assert_eq!(p.realized_pnl(), 0);
        assert_eq!(p.position_qty(&sym("AAPL")), 0);
    }

    #[test]
    fn arithmetic_overflow_rejection() {
        let mut p = Portfolio::new(0);
        let err = p
            .apply_fill(&sym("AAPL"), Side::Buy, px(i64::MAX), qty(u64::MAX))
            .unwrap_err();
        assert_eq!(err, PortfolioError::Overflow);
        assert_eq!(p.cash(), 0);
        assert_eq!(p.position_qty(&sym("AAPL")), 0);

        let err = p
            .apply_fill(&sym("AAPL"), Side::Buy, px(100), qty(0))
            .unwrap_err();
        assert_eq!(err, PortfolioError::InvalidQuantity);
    }

    #[test]
    fn multi_symbol_isolation() {
        let mut p = Portfolio::new(1_000_000);
        p.apply_fill(&sym("AAPL"), Side::Buy, px(100), qty(10))
            .unwrap();
        p.apply_fill(&sym("MSFT"), Side::Sell, px(200), qty(5))
            .unwrap();
        assert_eq!(p.position_qty(&sym("AAPL")), 10);
        assert_eq!(p.position_qty(&sym("MSFT")), -5);

        p.apply_fill(&sym("AAPL"), Side::Sell, px(110), qty(10))
            .unwrap();
        assert_eq!(p.realized_pnl(), 10 * 10);
        assert_eq!(p.position_qty(&sym("MSFT")), -5);
        assert_eq!(p.position(&sym("MSFT")).unwrap().avg_cost, px(200));
    }

    #[test]
    fn snapshot_is_sorted_and_deterministic() {
        let mut p = Portfolio::new(1_000_000);
        p.apply_fill(&sym("MSFT"), Side::Buy, px(200), qty(1))
            .unwrap();
        p.apply_fill(&sym("AAPL"), Side::Buy, px(100), qty(2))
            .unwrap();
        p.apply_fill(&sym("ZZZ"), Side::Sell, px(50), qty(3))
            .unwrap();

        let snap = p.snapshot().unwrap();
        let symbols: Vec<_> = snap.positions.iter().map(|x| x.symbol.0.as_str()).collect();
        assert_eq!(symbols, vec!["AAPL", "MSFT", "ZZZ"]);

        let snap2 = p.snapshot().unwrap();
        assert_eq!(snap, snap2);
        assert_eq!(
            snap.equity,
            snap.cash + snap.positions.iter().map(|x| x.market_value).sum::<i128>()
        );
    }

    #[test]
    fn apply_trade_as_buyer() {
        let mut p = Portfolio::new(10_000);
        let trade = Trade {
            symbol: sym("AAPL"),
            taker_order_id: 1,
            maker_order_id: 2,
            price: px(100),
            qty: qty(5),
            aggressor_side: Side::Buy,
            timestamp_ns: 1,
        };
        p.apply_trade_as(&trade, Side::Buy).unwrap();
        assert_eq!(p.position_qty(&sym("AAPL")), 5);
        assert_eq!(p.cash(), 10_000 - 500);
    }
}
