//! Criterion before/after for order pool vs Vec storage (feature order_pool).

use std::hint::black_box;

use criterion::{criterion_group, Criterion};
use engine::elite::order_pool::{sample_order, OrderPool};

fn bench_order_pool(c: &mut Criterion) {
    let mut group = c.benchmark_group("order_pool_experiment");

    group.bench_function("vec_push_pop_churn", |b| {
        b.iter(|| {
            let mut v = Vec::with_capacity(256);
            for i in 0..256u64 {
                v.push(sample_order(i));
            }
            while v.pop().is_some() {}
            black_box(v);
        });
    });

    group.bench_function("order_pool_insert_remove_reuse", |b| {
        b.iter(|| {
            let mut pool = OrderPool::with_capacity(256);
            let mut handles = Vec::with_capacity(256);
            for i in 0..256u64 {
                handles.push(pool.insert(sample_order(i)));
            }
            for h in handles {
                let _ = pool.remove(h);
            }
            // reuse
            for i in 0..256u64 {
                let _ = pool.insert(sample_order(1000 + i));
            }
            black_box(pool);
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30).warm_up_time(std::time::Duration::from_millis(200));
    targets = bench_order_pool
}

fn main() {
    if std::env::var_os("BENCH_FULL").is_none() {
        eprintln!("order_pool_bench: smoke ok (set BENCH_FULL=1 to measure)");
        return;
    }
    benches();
}
