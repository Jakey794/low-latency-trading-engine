//! Criterion: lock-free ArrayQueue vs `std::sync::mpsc` baseline.
//!
//! Requires feature `lockfree_queue`. Results are machine-specific.

use std::{hint::black_box, sync::mpsc, thread};

use criterion::{criterion_group, Criterion};
use engine::{
    elite::lockfree_queue::EventQueue,
    events::{InputEvent, NewOrderEvent},
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

fn bench_queues(c: &mut Criterion) {
    let mut group = c.benchmark_group("ingress_queue");
    const N: u64 = 10_000;

    group.bench_function("std_mpsc_spsc", |b| {
        b.iter(|| {
            let (tx, rx) = mpsc::sync_channel(N as usize);
            let producer = thread::spawn(move || {
                for i in 0..N {
                    tx.send(sample_event(i)).unwrap();
                }
            });
            let mut count = 0u64;
            while count < N {
                let _ = black_box(rx.recv().unwrap());
                count += 1;
            }
            producer.join().unwrap();
            black_box(count);
        });
    });

    group.bench_function("crossbeam_array_queue_spsc", |b| {
        b.iter(|| {
            let q = EventQueue::with_capacity(N as usize);
            let q_prod = &q;
            let producer = thread::scope(|s| {
                s.spawn(|| {
                    for i in 0..N {
                        while q_prod.push(sample_event(i)).is_err() {
                            std::hint::spin_loop();
                        }
                    }
                });
                let mut count = 0u64;
                while count < N {
                    if let Some(ev) = q.pop() {
                        black_box(ev);
                        count += 1;
                    } else {
                        std::hint::spin_loop();
                    }
                }
                count
            });
            black_box(producer);
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20).warm_up_time(std::time::Duration::from_millis(200));
    targets = bench_queues
}

fn main() {
    if std::env::var_os("BENCH_FULL").is_none() {
        eprintln!("lockfree_queue_bench: smoke ok (set BENCH_FULL=1 to measure)");
        return;
    }
    benches();
}
