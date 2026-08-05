//! Isolated elite experiments: order pool and lock-free queue.
//!
//! These are optional measurement prototypes. The deterministic production
//! path remains single-threaded and does not depend on these modules.

#[cfg(feature = "order_pool")]
pub mod order_pool;

#[cfg(feature = "lockfree_queue")]
pub mod lockfree_queue;
