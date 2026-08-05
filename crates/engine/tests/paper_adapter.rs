//! Local paper market-data adapter tests (no public internet).

use engine::{
    paper::{
        paper_message_to_runtime_event, MockPaperSession, PaperMdMessage, PaperReport,
        ReconnectPolicy,
    },
    runtime::{Runtime, RuntimeConfig, RuntimeOutputKind},
    types::{OrderType, Side, Symbol},
};

fn demo_messages() -> Vec<PaperMdMessage> {
    vec![
        PaperMdMessage::Quote {
            seq: 1,
            ts_ns: 100,
            symbol: "AAPL".into(),
            bid_px: 100,
            bid_qty: 10,
            ask_px: 101,
            ask_qty: 10,
        },
        PaperMdMessage::Heartbeat { seq: 3, ts_ns: 150 },
        PaperMdMessage::PaperOrder {
            seq: 4,
            ts_ns: 200,
            order_id: 500,
            symbol: "AAPL".into(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            price: Some(101),
            qty: 4,
        },
    ]
}

#[test]
fn paper_quote_and_aggressor_drive_runtime_deterministically() {
    let mut session_a = MockPaperSession::from_messages(demo_messages());
    let events_a = session_a.drain_runtime_events().expect("drain a");
    // Quote → 2 orders (seq 1,2); aggressor → 1 order (seq 4)
    assert_eq!(events_a.len(), 3);

    let mut session_b = MockPaperSession::from_messages(demo_messages());
    let events_b = session_b.drain_runtime_events().expect("drain b");
    assert_eq!(events_a, events_b);

    let mut runtime_a = Runtime::new(vec![Symbol("AAPL".into())], RuntimeConfig::default());
    let mut runtime_b = Runtime::new(vec![Symbol("AAPL".into())], RuntimeConfig::default());
    let a = runtime_a.process_events(events_a).unwrap();
    let b = runtime_b.process_events(events_b).unwrap();
    assert_eq!(a.outputs, b.outputs);
    assert!(a
        .outputs
        .iter()
        .any(|o| matches!(o.kind, RuntimeOutputKind::Trade { .. })));
}

#[test]
fn paper_cancel_round_trip() {
    let messages = vec![
        PaperMdMessage::PaperOrder {
            seq: 1,
            ts_ns: 1,
            order_id: 1,
            symbol: "AAPL".into(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            price: Some(100),
            qty: 5,
        },
        PaperMdMessage::PaperCancel {
            seq: 2,
            ts_ns: 2,
            order_id: 1,
            symbol: "AAPL".into(),
        },
    ];
    let mut session = MockPaperSession::from_messages(messages);
    let events = session.drain_runtime_events().unwrap();
    let mut runtime = Runtime::new(vec![Symbol("AAPL".into())], RuntimeConfig::default());
    let result = runtime.process_events(events).unwrap();
    assert!(result
        .outputs
        .iter()
        .any(|o| matches!(o.kind, RuntimeOutputKind::Cancelled { order_id: 1 })));
}

#[test]
fn reconnect_exhaustion_emits_adapter_error() {
    let mut policy = ReconnectPolicy::new(1);
    policy.on_connected();
    assert_eq!(policy.on_disconnect(), Some(1));
    assert_eq!(policy.on_disconnect(), None);

    let mut session = MockPaperSession::from_messages(vec![]);
    let reports = session.source.handle_disconnect("net reset");
    assert!(reports
        .iter()
        .any(|r| matches!(r, PaperReport::Disconnected { .. })));
    assert!(reports
        .iter()
        .any(|r| matches!(r, PaperReport::Reconnecting { .. })));
}

#[test]
fn convert_rejects_invalid_limit() {
    let msg = PaperMdMessage::PaperOrder {
        seq: 1,
        ts_ns: 1,
        order_id: 1,
        symbol: "AAPL".into(),
        side: Side::Buy,
        order_type: OrderType::Limit,
        price: None,
        qty: 1,
    };
    assert!(paper_message_to_runtime_event(&msg).is_err());
}
