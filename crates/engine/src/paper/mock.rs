//! In-memory mock paper session for deterministic local tests (no network).

use std::collections::VecDeque;

use thiserror::Error;

use super::{MarketDataAdapter, PaperMdMessage, PaperReport, ReconnectPolicy};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MockPaperError {
    #[error("mock session disconnected: {0}")]
    Disconnected(String),
    #[error("mock session closed")]
    Closed,
}

/// Producer side: inject scripted messages into the mock duplex.
#[derive(Debug, Default)]
pub struct MockPaperSink {
    queue: VecDeque<PaperMdMessage>,
    closed: bool,
}

impl MockPaperSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, msg: PaperMdMessage) {
        if !self.closed {
            self.queue.push_back(msg);
        }
    }

    pub fn extend<I: IntoIterator<Item = PaperMdMessage>>(&mut self, iter: I) {
        for msg in iter {
            self.push(msg);
        }
    }

    pub fn close(&mut self) {
        self.closed = true;
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

/// Consumer side implementing [`MarketDataAdapter`].
#[derive(Debug)]
pub struct MockPaperSource {
    sink: MockPaperSink,
    reports: Vec<PaperReport>,
    reconnect: ReconnectPolicy,
}

impl MockPaperSource {
    pub fn new(sink: MockPaperSink) -> Self {
        let mut reconnect = ReconnectPolicy::default();
        reconnect.on_connected();
        Self {
            sink,
            reports: Vec::new(),
            reconnect,
        }
    }

    pub fn with_reconnect(sink: MockPaperSink, policy: ReconnectPolicy) -> Self {
        let mut policy = policy;
        policy.on_connected();
        Self {
            sink,
            reports: Vec::new(),
            reconnect: policy,
        }
    }

    pub fn reports(&self) -> &[PaperReport] {
        &self.reports
    }

    pub fn push_report(&mut self, report: PaperReport) {
        self.reports.push(report);
    }

    pub fn reconnect_policy(&self) -> &ReconnectPolicy {
        &self.reconnect
    }

    /// Handle a disconnect message using the reconnect policy (no sleeps).
    ///
    /// Does not auto-complete reconnect; call [`ReconnectPolicy::on_connected`]
    /// (via [`Self::mark_reconnected`]) when the demo session is ready again.
    pub fn handle_disconnect(&mut self, reason: impl Into<String>) -> Vec<PaperReport> {
        let reason = reason.into();
        let mut out = vec![PaperReport::Disconnected {
            reason: reason.clone(),
        }];
        match self.reconnect.on_disconnect() {
            Some(attempt) => {
                out.push(PaperReport::Reconnecting { attempt });
            }
            None => {
                self.reconnect.mark_failed();
                out.push(PaperReport::AdapterError {
                    message: format!("reconnect exhausted after disconnect: {reason}"),
                });
            }
        }
        self.reports.extend(out.iter().cloned());
        out
    }

    pub fn mark_reconnected(&mut self) {
        self.reconnect.on_connected();
    }
}

impl MarketDataAdapter for MockPaperSource {
    type Error = MockPaperError;

    fn next_message(&mut self) -> Result<Option<PaperMdMessage>, Self::Error> {
        if self.sink.closed && self.sink.queue.is_empty() {
            return Err(MockPaperError::Closed);
        }
        Ok(self.sink.queue.pop_front())
    }
}

/// Paired sink/source for scripted paper sessions.
#[derive(Debug)]
pub struct MockPaperSession {
    pub source: MockPaperSource,
}

impl MockPaperSession {
    pub fn from_messages(messages: Vec<PaperMdMessage>) -> Self {
        let mut sink = MockPaperSink::new();
        sink.extend(messages);
        Self {
            source: MockPaperSource::new(sink),
        }
    }

    pub fn drain_runtime_events(
        &mut self,
    ) -> Result<Vec<crate::runtime::RuntimeEvent>, DrainError> {
        use super::{paper_message_to_runtime_event, PaperConvertError};

        let mut events = Vec::new();
        loop {
            match self.source.next_message().map_err(DrainError::Mock)? {
                None => break,
                Some(PaperMdMessage::Disconnect { reason }) => {
                    let _ = self.source.handle_disconnect(reason);
                }
                Some(PaperMdMessage::Heartbeat { .. }) => {}
                Some(msg) => match paper_message_to_runtime_event(&msg) {
                    Ok(batch) => events.extend(batch),
                    Err(PaperConvertError::NotAnEvent) => {}
                    Err(e) => return Err(DrainError::Convert(e)),
                },
            }
        }
        Ok(events)
    }
}

/// Errors while draining a mock paper session into runtime events.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DrainError {
    #[error(transparent)]
    Mock(#[from] MockPaperError),
    #[error(transparent)]
    Convert(#[from] super::PaperConvertError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Side;

    #[test]
    fn mock_preserves_fifo_order() {
        let mut sink = MockPaperSink::new();
        sink.push(PaperMdMessage::Heartbeat { seq: 1, ts_ns: 1 });
        sink.push(PaperMdMessage::PaperOrder {
            seq: 2,
            ts_ns: 2,
            order_id: 10,
            symbol: "AAPL".into(),
            side: Side::Buy,
            order_type: crate::types::OrderType::Limit,
            price: Some(100),
            qty: 1,
        });
        let mut src = MockPaperSource::new(sink);
        let a = src.next_message().unwrap().unwrap();
        let b = src.next_message().unwrap().unwrap();
        assert!(matches!(a, PaperMdMessage::Heartbeat { seq: 1, .. }));
        assert!(matches!(b, PaperMdMessage::PaperOrder { order_id: 10, .. }));
        assert!(src.next_message().unwrap().is_none());
    }

    #[test]
    fn reconnect_policy_bounds_attempts() {
        let policy = ReconnectPolicy::new(2);
        let mut src = MockPaperSource::with_reconnect(MockPaperSink::new(), policy);
        let r1 = src.handle_disconnect("bye");
        assert!(r1
            .iter()
            .any(|r| matches!(r, PaperReport::Reconnecting { attempt: 1 })));
        let r2 = src.handle_disconnect("bye2");
        assert!(r2
            .iter()
            .any(|r| matches!(r, PaperReport::Reconnecting { attempt: 2 })));
        let r3 = src.handle_disconnect("bye3");
        assert!(r3
            .iter()
            .any(|r| matches!(r, PaperReport::AdapterError { .. })));
    }
}
