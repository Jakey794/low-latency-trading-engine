//! Safe slab-style order pool experiment (feature `order_pool`).
//!
//! Not integrated into the default matching path. Measures whether reusing
//! order slots reduces allocations versus `Vec`/`Box` churn.
//!
//! Handles are generation-tagged so a stale handle cannot access a reused slot.

use crate::types::{Order, OrderId, OrderType, PriceTicks, Qty, Side, Symbol, TimestampNanos};

#[derive(Debug, Clone)]
struct Slot {
    order: Order,
    generation: u32,
    live: bool,
}

/// Dense free-list order pool with generation-safe handles.
#[derive(Debug, Default)]
pub struct OrderPool {
    slots: Vec<Slot>,
    free: Vec<usize>,
}

/// Stable index + generation. Stale handles fail closed after remove/reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoolHandle {
    index: usize,
    generation: u32,
}

impl PoolHandle {
    pub fn index(self) -> usize {
        self.index
    }

    pub fn generation(self) -> u32 {
        self.generation
    }
}

impl OrderPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            slots: Vec::with_capacity(cap),
            free: Vec::new(),
        }
    }

    pub fn insert(&mut self, order: Order) -> PoolHandle {
        if let Some(idx) = self.free.pop() {
            let slot = &mut self.slots[idx];
            let generation = slot.generation;
            slot.order = order;
            slot.live = true;
            PoolHandle {
                index: idx,
                generation,
            }
        } else {
            let idx = self.slots.len();
            self.slots.push(Slot {
                order,
                generation: 0,
                live: true,
            });
            PoolHandle {
                index: idx,
                generation: 0,
            }
        }
    }

    pub fn get(&self, handle: PoolHandle) -> Option<&Order> {
        let slot = self.slots.get(handle.index)?;
        if slot.live && slot.generation == handle.generation {
            Some(&slot.order)
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, handle: PoolHandle) -> Option<&mut Order> {
        let slot = self.slots.get_mut(handle.index)?;
        if slot.live && slot.generation == handle.generation {
            Some(&mut slot.order)
        } else {
            None
        }
    }

    pub fn remove(&mut self, handle: PoolHandle) -> Option<Order> {
        let slot = self.slots.get_mut(handle.index)?;
        if !slot.live || slot.generation != handle.generation {
            return None;
        }
        slot.live = false;
        // Bump generation so the old handle cannot observe a reused slot.
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(handle.index);
        Some(slot.order.clone())
    }

    pub fn live_count(&self) -> usize {
        self.slots.iter().filter(|s| s.live).count()
    }

    pub fn capacity_slots(&self) -> usize {
        self.slots.len()
    }
}

/// Build a sample order for pool parity tests.
pub fn sample_order(id: OrderId) -> Order {
    Order {
        order_id: id,
        symbol: Symbol("AAPL".into()),
        side: Side::Buy,
        order_type: OrderType::Limit,
        price: Some(PriceTicks(100)),
        qty: Qty(1),
        timestamp_ns: id as TimestampNanos,
        strategy_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_get_remove_reuses_slot() {
        let mut pool = OrderPool::new();
        let h1 = pool.insert(sample_order(1));
        let h2 = pool.insert(sample_order(2));
        assert_eq!(pool.live_count(), 2);
        assert_eq!(pool.get(h1).unwrap().order_id, 1);
        pool.remove(h1).unwrap();
        assert_eq!(pool.live_count(), 1);
        let h3 = pool.insert(sample_order(3));
        assert_eq!(h3.index(), h1.index()); // reused slot index
        assert_ne!(h3.generation(), h1.generation()); // new generation
        assert_eq!(pool.get(h3).unwrap().order_id, 3);
        assert!(pool.get(h1).is_none()); // stale handle fails closed
        assert!(pool.get(h2).is_some());
        assert_eq!(pool.capacity_slots(), 2);
    }

    #[test]
    fn stale_handle_cannot_remove_reused_slot() {
        let mut pool = OrderPool::new();
        let h1 = pool.insert(sample_order(1));
        pool.remove(h1).unwrap();
        let h2 = pool.insert(sample_order(2));
        assert!(pool.remove(h1).is_none());
        assert_eq!(pool.get(h2).unwrap().order_id, 2);
    }

    #[test]
    fn parity_with_vec_storage() {
        let mut pool = OrderPool::with_capacity(8);
        let mut handles = Vec::new();
        let mut vec_store = Vec::new();
        for i in 0..20u64 {
            let o = sample_order(i);
            vec_store.push(o.clone());
            handles.push(pool.insert(o));
        }
        for i in (0..20).step_by(2) {
            let _ = pool.remove(handles[i]);
        }
        assert_eq!(pool.live_count(), 10);
        assert_eq!(vec_store.len(), 20);
    }
}
