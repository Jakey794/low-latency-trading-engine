use crate::book::BookSnapshot;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReplaySummary {
    pub input_events: u64,
    pub output_events: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub trades: u64,
    pub cancelled: u64,
    pub expired: u64,
    pub final_resting_orders: u64,
    pub final_bid_levels: u64,
    pub final_ask_levels: u64,
}

impl ReplaySummary {
    pub(crate) fn record_final_book(&mut self, snapshot: &BookSnapshot) {
        self.final_bid_levels =
            u64::try_from(snapshot.bids.len()).expect("snapshot bid level count must fit in u64");
        self.final_ask_levels =
            u64::try_from(snapshot.asks.len()).expect("snapshot ask level count must fit in u64");
        self.final_resting_orders = snapshot
            .bids
            .iter()
            .chain(&snapshot.asks)
            .map(|level| {
                u64::try_from(level.order_count).expect("snapshot order count must fit in u64")
            })
            .try_fold(0_u64, |total, count| total.checked_add(count))
            .expect("snapshot resting order count must fit in u64");
    }
}
