use serde::{Deserialize, Serialize};

use crate::types::{OrderId, PriceTicks, Qty, Symbol};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookSnapshot {
    pub symbol: Symbol,
    pub bids: Vec<PriceLevelSnapshot>,
    pub asks: Vec<PriceLevelSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceLevelSnapshot {
    pub price: PriceTicks,
    pub total_qty: Qty,
    pub order_count: usize,
    pub order_ids: Vec<OrderId>,
}
