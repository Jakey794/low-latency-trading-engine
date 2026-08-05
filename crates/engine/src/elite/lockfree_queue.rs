//! Lock-free queue experiment using `crossbeam_queue::ArrayQueue`.
//!
//! This is an infrastructure prototype only. The deterministic core remains
//! single-threaded; this module does not prove lower end-to-end latency.

use crossbeam_queue::ArrayQueue;

use crate::events::InputEvent;

/// Bounded lock-free event queue for producer/consumer experiments.
pub struct EventQueue {
    inner: ArrayQueue<InputEvent>,
}

impl EventQueue {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            inner: ArrayQueue::new(cap),
        }
    }

    pub fn push(&self, event: InputEvent) -> Result<(), InputEvent> {
        self.inner.push(event)
    }

    pub fn pop(&self) -> Option<InputEvent> {
        self.inner.pop()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        events::NewOrderEvent,
        types::{Order, OrderType, PriceTicks, Qty, Side, Symbol},
    };

    fn sample_event(id: u64) -> InputEvent {
        InputEvent::NewOrder(NewOrderEvent {
            seq: id,
            order: Order {
                order_id: id,
                symbol: Symbol("AAPL".into()),
                side: Side::Buy,
                order_type: OrderType::Limit,
                price: Some(PriceTicks(100)),
                qty: Qty(1),
                timestamp_ns: id,
                strategy_id: None,
            },
        })
    }

    #[test]
    fn push_pop_preserves_order_single_thread() {
        let q = EventQueue::with_capacity(8);
        for i in 0..5 {
            q.push(sample_event(i)).unwrap();
        }
        for i in 0..5 {
            match q.pop().unwrap() {
                InputEvent::NewOrder(e) => assert_eq!(e.seq, i),
                _ => panic!("expected new order"),
            }
        }
        assert!(q.is_empty());
    }

    #[test]
    fn bounded_queue_rejects_when_full() {
        let q = EventQueue::with_capacity(2);
        q.push(sample_event(1)).unwrap();
        q.push(sample_event(2)).unwrap();
        assert!(q.push(sample_event(3)).is_err());
        assert_eq!(q.len(), 2);
        assert!(q.pop().is_some());
        q.push(sample_event(4)).unwrap();
    }
}
