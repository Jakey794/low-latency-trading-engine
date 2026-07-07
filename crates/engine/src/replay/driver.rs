use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::{
    events::{CancelOrderEvent, ExecutionReport, InputEvent, NewOrderEvent},
    matching::MatchingEngine,
    types::SequenceNumber,
};

use super::{
    parse_jsonl, ReplayEvent, ReplayEventKind, ReplayOutputEvent, ReplayOutputKind,
    ReplayParseError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayResult {
    pub outputs: Vec<ReplayOutputEvent>,
    pub summary: ReplaySummary,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReplaySummary {
    pub input_events: u64,
    pub output_events: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub trades: u64,
    pub cancelled: u64,
    pub expired: u64,
}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("failed to open replay file {path}: {source}")]
    OpenFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Parse(#[from] ReplayParseError),
    #[error("duplicate replay sequence {seq}")]
    DuplicateSequence { seq: SequenceNumber },
    #[error("out-of-order replay sequence {seq} after {previous}")]
    OutOfOrderSequence {
        previous: SequenceNumber,
        seq: SequenceNumber,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayDriver {
    engine: MatchingEngine,
    last_seq: Option<SequenceNumber>,
}

impl ReplayDriver {
    pub fn new(engine: MatchingEngine) -> Self {
        Self {
            engine,
            last_seq: None,
        }
    }

    pub fn engine(&self) -> &MatchingEngine {
        &self.engine
    }

    pub fn engine_mut(&mut self) -> &mut MatchingEngine {
        &mut self.engine
    }

    pub fn into_engine(self) -> MatchingEngine {
        self.engine
    }

    pub fn last_seq(&self) -> Option<SequenceNumber> {
        self.last_seq
    }

    pub fn replay_events<I>(&mut self, events: I) -> Result<ReplayResult, ReplayError>
    where
        I: IntoIterator<Item = ReplayEvent>,
    {
        let events: Vec<_> = events.into_iter().collect();
        self.validate_sequences(&events)?;

        let final_seq = events.last().map(|event| event.seq);
        let mut result = ReplayResult {
            outputs: Vec::new(),
            summary: ReplaySummary {
                input_events: events.len() as u64,
                ..ReplaySummary::default()
            },
        };

        for event in events {
            self.replay_event(event, &mut result);
        }

        if let Some(final_seq) = final_seq {
            self.last_seq = Some(final_seq);
        }

        Ok(result)
    }

    pub fn replay_file<P: AsRef<Path>>(&mut self, path: P) -> Result<ReplayResult, ReplayError> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|source| ReplayError::OpenFile {
            path: path.to_path_buf(),
            source,
        })?;
        let events = parse_jsonl(BufReader::new(file))?;
        self.replay_events(events)
    }

    fn validate_sequences(&self, events: &[ReplayEvent]) -> Result<(), ReplayError> {
        let mut previous = self.last_seq;

        for event in events {
            if let Some(previous_seq) = previous {
                if event.seq == previous_seq {
                    return Err(ReplayError::DuplicateSequence { seq: event.seq });
                }
                if event.seq < previous_seq {
                    return Err(ReplayError::OutOfOrderSequence {
                        previous: previous_seq,
                        seq: event.seq,
                    });
                }
            }
            previous = Some(event.seq);
        }

        Ok(())
    }

    fn replay_event(&mut self, event: ReplayEvent, result: &mut ReplayResult) {
        let ReplayEvent { seq, ts_ns, kind } = event;
        let input = match kind {
            ReplayEventKind::NewOrder { order } => {
                InputEvent::NewOrder(NewOrderEvent { seq, order })
            }
            ReplayEventKind::Cancel { order_id, symbol } => InputEvent::Cancel(CancelOrderEvent {
                seq,
                order_id,
                symbol,
                timestamp_ns: ts_ns,
            }),
        };

        let (reports, trades) = self.engine.process_event_with_trades(input, ts_ns);
        let mut trades = trades.into_iter();

        for report in reports {
            let kind = match report {
                ExecutionReport::Accepted { order_id } => {
                    Some(ReplayOutputKind::Accepted { order_id })
                }
                ExecutionReport::Filled { .. } | ExecutionReport::PartiallyFilled { .. } => {
                    Some(ReplayOutputKind::Trade {
                        trade: trades
                            .next()
                            .expect("each engine fill report must have one trade"),
                    })
                }
                ExecutionReport::Rested { .. } => None,
                ExecutionReport::Cancelled { order_id } => {
                    Some(ReplayOutputKind::Cancelled { order_id })
                }
                ExecutionReport::Expired {
                    order_id,
                    remaining,
                } => Some(ReplayOutputKind::Expired {
                    order_id,
                    remaining,
                }),
                ExecutionReport::Rejected { order_id, reason } => {
                    Some(ReplayOutputKind::Rejected {
                        order_id: Some(order_id),
                        reason,
                    })
                }
            };

            if let Some(kind) = kind {
                Self::push_output(result, seq, ts_ns, kind);
            }
        }

        assert!(
            trades.next().is_none(),
            "each engine trade must have one fill report"
        );
    }

    fn push_output(
        result: &mut ReplayResult,
        seq: SequenceNumber,
        ts_ns: u64,
        kind: ReplayOutputKind,
    ) {
        result.summary.output_events += 1;
        match &kind {
            ReplayOutputKind::Accepted { .. } => result.summary.accepted += 1,
            ReplayOutputKind::Rejected { .. } => result.summary.rejected += 1,
            ReplayOutputKind::Trade { .. } => result.summary.trades += 1,
            ReplayOutputKind::Cancelled { .. } => result.summary.cancelled += 1,
            ReplayOutputKind::Expired { .. } => result.summary.expired += 1,
        }
        result.outputs.push(ReplayOutputEvent { seq, ts_ns, kind });
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;
    use crate::{
        events::RejectReason,
        replay::ReplayEventKind,
        types::{Order, OrderType, PriceTicks, Qty, Side, Symbol},
    };

    static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

    fn symbol() -> Symbol {
        Symbol("AAPL".to_owned())
    }

    fn order_event(
        seq: u64,
        order_id: u64,
        side: Side,
        price: Option<i64>,
        qty: u64,
    ) -> ReplayEvent {
        ReplayEvent {
            seq,
            ts_ns: seq * 100,
            kind: ReplayEventKind::NewOrder {
                order: Order {
                    order_id,
                    symbol: symbol(),
                    side,
                    order_type: if price.is_some() {
                        OrderType::Limit
                    } else {
                        OrderType::Market
                    },
                    price: price.map(PriceTicks),
                    qty: Qty(qty),
                    timestamp_ns: seq * 10,
                    strategy_id: None,
                },
            },
        }
    }

    fn cancel_event(seq: u64, order_id: u64) -> ReplayEvent {
        ReplayEvent {
            seq,
            ts_ns: seq * 100,
            kind: ReplayEventKind::Cancel {
                order_id,
                symbol: symbol(),
            },
        }
    }

    fn driver() -> ReplayDriver {
        ReplayDriver::new(MatchingEngine::new(symbol()))
    }

    #[test]
    fn empty_replay_has_empty_outputs_and_summary() {
        let mut driver = driver();

        let result = driver.replay_events(Vec::new()).unwrap();

        assert!(result.outputs.is_empty());
        assert_eq!(result.summary, ReplaySummary::default());
        assert_eq!(driver.last_seq(), None);
    }

    #[test]
    fn one_resting_order_emits_accepted() {
        let mut driver = driver();

        let result = driver
            .replay_events([order_event(1, 10, Side::Buy, Some(100), 5)])
            .unwrap();

        assert_eq!(
            result.outputs,
            vec![ReplayOutputEvent {
                seq: 1,
                ts_ns: 100,
                kind: ReplayOutputKind::Accepted { order_id: 10 },
            }]
        );
        assert_eq!(
            result.summary,
            ReplaySummary {
                input_events: 1,
                output_events: 1,
                accepted: 1,
                ..ReplaySummary::default()
            }
        );
        assert!(driver.engine().book().get_order(10).is_some());
    }

    #[test]
    fn successful_cancel_emits_cancelled() {
        let mut driver = driver();

        let result = driver
            .replay_events([
                order_event(1, 10, Side::Buy, Some(100), 5),
                cancel_event(2, 10),
            ])
            .unwrap();

        assert_eq!(result.outputs.len(), 2);
        assert_eq!(
            result.outputs[1],
            ReplayOutputEvent {
                seq: 2,
                ts_ns: 200,
                kind: ReplayOutputKind::Cancelled { order_id: 10 },
            }
        );
        assert_eq!(result.summary.cancelled, 1);
        assert!(driver.engine().book().get_order(10).is_none());
    }

    #[test]
    fn unknown_cancel_emits_rejected() {
        let mut driver = driver();

        let result = driver.replay_events([cancel_event(1, 99)]).unwrap();

        assert_eq!(
            result.outputs,
            vec![ReplayOutputEvent {
                seq: 1,
                ts_ns: 100,
                kind: ReplayOutputKind::Rejected {
                    order_id: Some(99),
                    reason: RejectReason::UnknownOrder,
                },
            }]
        );
        assert_eq!(result.summary.rejected, 1);
    }

    #[test]
    fn crossing_replay_emits_ordered_maker_aware_trade() {
        let mut driver = driver();

        let result = driver
            .replay_events([
                order_event(1, 10, Side::Sell, Some(100), 5),
                order_event(2, 20, Side::Buy, Some(105), 5),
            ])
            .unwrap();

        assert_eq!(result.outputs.len(), 3);
        assert_eq!(
            result.outputs[2],
            ReplayOutputEvent {
                seq: 2,
                ts_ns: 200,
                kind: ReplayOutputKind::Trade {
                    trade: crate::events::Trade {
                        symbol: symbol(),
                        taker_order_id: 20,
                        maker_order_id: 10,
                        price: PriceTicks(100),
                        qty: Qty(5),
                        aggressor_side: Side::Buy,
                        timestamp_ns: 200,
                    },
                },
            }
        );
        assert_eq!(result.summary.input_events, 2);
        assert_eq!(result.summary.output_events, 3);
        assert_eq!(result.summary.accepted, 2);
        assert_eq!(result.summary.trades, 1);
    }

    #[test]
    fn partially_filled_market_order_emits_expired_and_complete_summary() {
        let mut driver = driver();

        let result = driver
            .replay_events([
                order_event(1, 10, Side::Sell, Some(100), 2),
                order_event(2, 20, Side::Buy, None, 3),
            ])
            .unwrap();

        assert!(matches!(
            result.outputs.last(),
            Some(ReplayOutputEvent {
                kind: ReplayOutputKind::Expired {
                    order_id: 20,
                    remaining: Qty(1)
                },
                ..
            })
        ));
        assert_eq!(
            result.summary,
            ReplaySummary {
                input_events: 2,
                output_events: 4,
                accepted: 2,
                trades: 1,
                expired: 1,
                ..ReplaySummary::default()
            }
        );
    }

    #[test]
    fn duplicate_sequence_is_atomic() {
        let mut driver = driver();

        let error = driver
            .replay_events([
                order_event(1, 10, Side::Buy, Some(100), 5),
                order_event(1, 11, Side::Buy, Some(99), 5),
            ])
            .unwrap_err();

        assert!(matches!(error, ReplayError::DuplicateSequence { seq: 1 }));
        assert!(driver.engine().book().get_order(10).is_none());
        assert_eq!(driver.last_seq(), None);
    }

    #[test]
    fn out_of_order_sequence_is_atomic() {
        let mut driver = driver();

        let error = driver
            .replay_events([
                order_event(2, 10, Side::Buy, Some(100), 5),
                order_event(1, 11, Side::Buy, Some(99), 5),
            ])
            .unwrap_err();

        assert!(matches!(
            error,
            ReplayError::OutOfOrderSequence {
                previous: 2,
                seq: 1
            }
        ));
        assert!(driver.engine().book().get_order(10).is_none());
        assert_eq!(driver.last_seq(), None);
    }

    #[test]
    fn sequence_order_is_enforced_across_calls() {
        let mut driver = driver();
        driver
            .replay_events([order_event(5, 10, Side::Buy, Some(100), 5)])
            .unwrap();

        let duplicate = driver.replay_events([cancel_event(5, 10)]).unwrap_err();
        let out_of_order = driver.replay_events([cancel_event(4, 10)]).unwrap_err();

        assert!(matches!(
            duplicate,
            ReplayError::DuplicateSequence { seq: 5 }
        ));
        assert!(matches!(
            out_of_order,
            ReplayError::OutOfOrderSequence {
                previous: 5,
                seq: 4
            }
        ));
        assert!(driver.engine().book().get_order(10).is_some());
        assert_eq!(driver.last_seq(), Some(5));
    }

    #[test]
    fn replay_file_parses_and_executes_jsonl() {
        let file_number = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "engine-replay-{}-{file_number}.jsonl",
            std::process::id()
        ));
        let json = r#"{"seq":1,"ts_ns":100,"kind":"new_order","order":{"order_id":10,"symbol":"AAPL","side":"Buy","order_type":"Limit","price":100,"qty":5,"timestamp_ns":10,"strategy_id":null}}
"#;
        fs::write(&path, json).unwrap();

        let result = driver().replay_file(&path).unwrap();
        fs::remove_file(&path).unwrap();

        assert_eq!(result.summary.input_events, 1);
        assert_eq!(result.summary.accepted, 1);
    }
}
