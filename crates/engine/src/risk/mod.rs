//! Pre-trade risk checks and kill-switch controls.
//!
//! Risk runs before matching. Rejections must not mutate the book, portfolio, or
//! strategy state. Cancels remain allowed while the kill switch is active.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    portfolio::{Money, Portfolio},
    types::{Order, OrderType, PriceTicks, Qty, Side},
};

/// Configurable risk limits. `None` means that check is disabled.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RiskLimits {
    /// Maximum quantity on a single new order.
    pub max_order_qty: Option<u64>,
    /// Maximum absolute position per symbol after the order.
    pub max_abs_position: Option<i64>,
    /// Maximum gross notional of a single order (`|price| * qty`).
    pub max_gross_notional: Option<Money>,
    /// Per-symbol overrides for absolute position.
    pub per_symbol_max_abs_position: BTreeMap<String, i64>,
    /// Per-symbol overrides for single-order quantity.
    pub per_symbol_max_order_qty: BTreeMap<String, u64>,
    /// Maximum total loss vs starting equity (`starting_equity - equity`).
    /// When breached post-trade, the kill switch activates automatically.
    pub max_total_loss: Option<Money>,
}

impl RiskLimits {
    pub fn unrestricted() -> Self {
        Self::default()
    }
}

/// Why a pre-trade check rejected an order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
pub enum RiskRejectReason {
    #[error("kill switch is active")]
    KillSwitchActive,
    #[error("order quantity {qty} exceeds max order quantity {limit}")]
    MaxOrderQty { qty: u64, limit: u64 },
    #[error("order quantity {qty} exceeds per-symbol max {limit} for {symbol}")]
    PerSymbolMaxOrderQty {
        symbol: String,
        qty: u64,
        limit: u64,
    },
    #[error("projected absolute position {projected} exceeds max {limit}")]
    MaxAbsPosition { projected: i64, limit: i64 },
    #[error("projected absolute position {projected} exceeds per-symbol max {limit} for {symbol}")]
    PerSymbolMaxAbsPosition {
        symbol: String,
        projected: i64,
        limit: i64,
    },
    #[error("order gross notional {notional} exceeds max {limit}")]
    MaxGrossNotional { notional: Money, limit: Money },
    #[error("order price required for notional check is missing or invalid")]
    InvalidPriceForNotional,
    #[error("risk arithmetic overflow")]
    Overflow,
}

/// Structured pre-trade decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskDecision {
    Allow,
    Reject { reason: RiskRejectReason },
}

impl RiskDecision {
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow)
    }

    pub fn reject_reason(&self) -> Option<&RiskRejectReason> {
        match self {
            Self::Allow => None,
            Self::Reject { reason } => Some(reason),
        }
    }
}

/// Pre-trade risk manager with manual and automatic kill switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskManager {
    limits: RiskLimits,
    killed: bool,
    starting_equity: Money,
    kill_reason: Option<String>,
}

impl RiskManager {
    pub fn new(limits: RiskLimits, starting_equity: Money) -> Self {
        Self {
            limits,
            killed: false,
            starting_equity,
            kill_reason: None,
        }
    }

    pub fn limits(&self) -> &RiskLimits {
        &self.limits
    }

    pub fn is_killed(&self) -> bool {
        self.killed
    }

    pub fn kill_reason(&self) -> Option<&str> {
        self.kill_reason.as_deref()
    }

    /// Manually activate the kill switch. New orders are rejected; cancels allowed.
    pub fn activate_kill_switch(&mut self, reason: impl Into<String>) {
        self.killed = true;
        self.kill_reason = Some(reason.into());
    }

    /// Explicit reset / re-arm. Does not mutate portfolio or book state.
    pub fn reset_kill_switch(&mut self) {
        self.killed = false;
        self.kill_reason = None;
    }

    /// Whether a cancel is permitted. Cancels are always allowed, including when killed.
    pub fn allow_cancel(&self) -> bool {
        true
    }

    /// Pre-trade check for a new order. Does not mutate portfolio or book.
    pub fn check_new_order(&self, order: &Order, portfolio: &Portfolio) -> RiskDecision {
        if self.killed {
            return RiskDecision::Reject {
                reason: RiskRejectReason::KillSwitchActive,
            };
        }

        if let Some(limit) = self.limits.max_order_qty {
            if order.qty.0 > limit {
                return RiskDecision::Reject {
                    reason: RiskRejectReason::MaxOrderQty {
                        qty: order.qty.0,
                        limit,
                    },
                };
            }
        }

        if let Some(&limit) = self.limits.per_symbol_max_order_qty.get(&order.symbol.0) {
            if order.qty.0 > limit {
                return RiskDecision::Reject {
                    reason: RiskRejectReason::PerSymbolMaxOrderQty {
                        symbol: order.symbol.0.clone(),
                        qty: order.qty.0,
                        limit,
                    },
                };
            }
        }

        if let Some(decision) = self.check_notional(order) {
            return decision;
        }

        if let Some(decision) = self.check_position(order, portfolio) {
            return decision;
        }

        RiskDecision::Allow
    }

    /// Post-trade loss check. May activate the kill switch deterministically.
    ///
    /// Returns `true` if the kill switch was activated by this call.
    pub fn check_post_trade_loss(
        &mut self,
        portfolio: &Portfolio,
    ) -> Result<bool, RiskRejectReason> {
        let Some(max_loss) = self.limits.max_total_loss else {
            return Ok(false);
        };

        let equity = portfolio.equity().map_err(|_| RiskRejectReason::Overflow)?;
        let loss = self
            .starting_equity
            .checked_sub(equity)
            .ok_or(RiskRejectReason::Overflow)?;

        if loss > max_loss {
            self.activate_kill_switch(format!(
                "max total loss breached: loss={loss} limit={max_loss}"
            ));
            return Ok(true);
        }
        Ok(false)
    }

    fn check_notional(&self, order: &Order) -> Option<RiskDecision> {
        let limit = self.limits.max_gross_notional?;

        let price = match order.order_type {
            OrderType::Limit => match order.price {
                Some(p) if p.0 >= 0 => p,
                _ => {
                    return Some(RiskDecision::Reject {
                        reason: RiskRejectReason::InvalidPriceForNotional,
                    });
                }
            },
            // Market orders: require an explicit price estimate via order.price if present;
            // otherwise skip notional (position checks still apply).
            OrderType::Market => match order.price {
                Some(p) if p.0 >= 0 => p,
                Some(_) => {
                    return Some(RiskDecision::Reject {
                        reason: RiskRejectReason::InvalidPriceForNotional,
                    });
                }
                None => return None,
            },
        };

        match Money::from(price.0).checked_mul(Money::from(order.qty.0)) {
            Some(notional) if notional > limit => Some(RiskDecision::Reject {
                reason: RiskRejectReason::MaxGrossNotional { notional, limit },
            }),
            Some(_) => None,
            None => Some(RiskDecision::Reject {
                reason: RiskRejectReason::Overflow,
            }),
        }
    }

    fn check_position(&self, order: &Order, portfolio: &Portfolio) -> Option<RiskDecision> {
        let qty = match i64::try_from(order.qty.0) {
            Ok(q) => q,
            Err(_) => {
                return Some(RiskDecision::Reject {
                    reason: RiskRejectReason::Overflow,
                });
            }
        };
        let delta = match order.side {
            Side::Buy => qty,
            Side::Sell => -qty,
        };
        let current = portfolio.position_qty(&order.symbol);
        let projected = match current.checked_add(delta) {
            Some(p) => p,
            None => {
                return Some(RiskDecision::Reject {
                    reason: RiskRejectReason::Overflow,
                });
            }
        };
        let abs_projected = projected.unsigned_abs();
        let abs_projected_i = match i64::try_from(abs_projected) {
            Ok(v) => v,
            Err(_) => {
                return Some(RiskDecision::Reject {
                    reason: RiskRejectReason::Overflow,
                });
            }
        };

        if let Some(&limit) = self.limits.per_symbol_max_abs_position.get(&order.symbol.0) {
            if abs_projected_i > limit {
                return Some(RiskDecision::Reject {
                    reason: RiskRejectReason::PerSymbolMaxAbsPosition {
                        symbol: order.symbol.0.clone(),
                        projected: abs_projected_i,
                        limit,
                    },
                });
            }
        }

        if let Some(limit) = self.limits.max_abs_position {
            if abs_projected_i > limit {
                return Some(RiskDecision::Reject {
                    reason: RiskRejectReason::MaxAbsPosition {
                        projected: abs_projected_i,
                        limit,
                    },
                });
            }
        }

        None
    }
}

/// Helper to estimate limit-order notional for tests and callers.
pub fn order_notional(price: PriceTicks, qty: Qty) -> Option<Money> {
    Money::from(price.0).checked_mul(Money::from(qty.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OrderId, OrderType, Symbol, TimestampNanos};

    fn sym(s: &str) -> Symbol {
        Symbol(s.to_owned())
    }

    fn order(id: OrderId, side: Side, price: i64, qty: u64) -> Order {
        Order {
            order_id: id,
            symbol: sym("AAPL"),
            side,
            order_type: OrderType::Limit,
            price: Some(PriceTicks(price)),
            qty: Qty(qty),
            timestamp_ns: id as TimestampNanos,
            strategy_id: None,
        }
    }

    #[test]
    fn max_order_qty_boundary() {
        let limits = RiskLimits {
            max_order_qty: Some(10),
            ..RiskLimits::default()
        };
        let rm = RiskManager::new(limits, 1_000_000);
        let portfolio = Portfolio::new(1_000_000);

        assert!(rm
            .check_new_order(&order(1, Side::Buy, 100, 10), &portfolio)
            .is_allow());
        let reject = rm.check_new_order(&order(2, Side::Buy, 100, 11), &portfolio);
        assert!(matches!(
            reject,
            RiskDecision::Reject {
                reason: RiskRejectReason::MaxOrderQty { qty: 11, limit: 10 }
            }
        ));
    }

    #[test]
    fn max_abs_position_boundary() {
        let limits = RiskLimits {
            max_abs_position: Some(10),
            ..RiskLimits::default()
        };
        let rm = RiskManager::new(limits, 1_000_000);
        let mut portfolio = Portfolio::new(1_000_000);
        portfolio
            .apply_fill(&sym("AAPL"), Side::Buy, PriceTicks(100), Qty(5))
            .unwrap();

        assert!(rm
            .check_new_order(&order(1, Side::Buy, 100, 5), &portfolio)
            .is_allow());
        let reject = rm.check_new_order(&order(2, Side::Buy, 100, 6), &portfolio);
        assert!(matches!(
            reject,
            RiskDecision::Reject {
                reason: RiskRejectReason::MaxAbsPosition {
                    projected: 11,
                    limit: 10
                }
            }
        ));
    }

    #[test]
    fn max_gross_notional_boundary() {
        let limits = RiskLimits {
            max_gross_notional: Some(1_000),
            ..RiskLimits::default()
        };
        let rm = RiskManager::new(limits, 1_000_000);
        let portfolio = Portfolio::new(1_000_000);

        assert!(rm
            .check_new_order(&order(1, Side::Buy, 100, 10), &portfolio)
            .is_allow());
        let reject = rm.check_new_order(&order(2, Side::Buy, 100, 11), &portfolio);
        assert!(matches!(
            reject,
            RiskDecision::Reject {
                reason: RiskRejectReason::MaxGrossNotional { .. }
            }
        ));
    }

    #[test]
    fn per_symbol_limits() {
        let mut limits = RiskLimits::default();
        limits.per_symbol_max_order_qty.insert("AAPL".into(), 3);
        limits.per_symbol_max_abs_position.insert("AAPL".into(), 5);

        let rm = RiskManager::new(limits, 1_000_000);
        let portfolio = Portfolio::new(1_000_000);

        assert!(matches!(
            rm.check_new_order(&order(1, Side::Buy, 100, 4), &portfolio),
            RiskDecision::Reject {
                reason: RiskRejectReason::PerSymbolMaxOrderQty { .. }
            }
        ));
        assert!(rm
            .check_new_order(&order(2, Side::Buy, 100, 3), &portfolio)
            .is_allow());
    }

    #[test]
    fn kill_switch_rejects_orders_allows_cancels() {
        let mut rm = RiskManager::new(RiskLimits::default(), 1_000_000);
        let portfolio = Portfolio::new(1_000_000);
        rm.activate_kill_switch("manual");
        assert!(rm.is_killed());
        assert!(rm.allow_cancel());
        assert!(matches!(
            rm.check_new_order(&order(1, Side::Buy, 100, 1), &portfolio),
            RiskDecision::Reject {
                reason: RiskRejectReason::KillSwitchActive
            }
        ));

        rm.reset_kill_switch();
        assert!(!rm.is_killed());
        assert!(rm
            .check_new_order(&order(2, Side::Buy, 100, 1), &portfolio)
            .is_allow());
    }

    #[test]
    fn automatic_kill_on_loss_breach() {
        let limits = RiskLimits {
            max_total_loss: Some(100),
            ..RiskLimits::default()
        };
        let mut rm = RiskManager::new(limits, 10_000);
        let mut portfolio = Portfolio::new(10_000);
        // Buy high, mark down to create unrealized loss > 100
        portfolio
            .apply_fill(&sym("AAPL"), Side::Buy, PriceTicks(100), Qty(10))
            .unwrap();
        portfolio.set_mark(&sym("AAPL"), PriceTicks(80)).unwrap();
        // equity = 10000 - 1000 + 800 = 9800; loss = 200 > 100
        assert!(rm.check_post_trade_loss(&portfolio).unwrap());
        assert!(rm.is_killed());
    }

    #[test]
    fn loss_at_boundary_does_not_trip() {
        let limits = RiskLimits {
            max_total_loss: Some(200),
            ..RiskLimits::default()
        };
        let mut rm = RiskManager::new(limits, 10_000);
        let mut portfolio = Portfolio::new(10_000);
        portfolio
            .apply_fill(&sym("AAPL"), Side::Buy, PriceTicks(100), Qty(10))
            .unwrap();
        portfolio.set_mark(&sym("AAPL"), PriceTicks(80)).unwrap();
        // loss == 200, breach is strict >
        assert!(!rm.check_post_trade_loss(&portfolio).unwrap());
        assert!(!rm.is_killed());
    }

    #[test]
    fn rejection_does_not_require_portfolio_mutation() {
        let limits = RiskLimits {
            max_order_qty: Some(1),
            ..RiskLimits::default()
        };
        let rm = RiskManager::new(limits, 1_000_000);
        let portfolio = Portfolio::new(1_000_000);
        let before = portfolio.clone();
        let _ = rm.check_new_order(&order(1, Side::Buy, 100, 5), &portfolio);
        assert_eq!(portfolio, before);
    }
}
