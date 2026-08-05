# Release Notes — complete elite engine

## Summary

Portfolio-quality Rust event-driven trading engine with deterministic matching,
replay, portfolio/P&amp;L, risk controls, strategy plugins, multi-symbol runtime,
benchmarks, and isolated elite experiments.

**Not** a profitable trading system, investment product, or exchange-grade
low-latency deployment.

## Delivered

- Limit order book with price-time priority, partial/full/multi-level fills
- Deterministic JSONL replay and golden tests (Weeks 1–5 preserved)
- Average-cost portfolio with realized/unrealized P&amp;L and equity
- Pre-trade risk limits and kill switch (cancels remain available)
- Runtime orchestration + market-making and momentum demonstration strategies
- Multi-symbol interleaved replay
- Criterion benches + `measure` harness with genuine hdrhistogram results
- Naive Python LOB baseline and measured comparison charts
- Property-based tests; order-pool and lock-free queue experiments
- Paper WebSocket / mock market-data adapter (localhost only; no live trading)
- CLI: `replay`, `simulate`, `strategy-replay`, `benchmark-report`, `websocket-demo`
- CI, demo, and `scripts/verify_final.sh`

## Measured results (machine-specific)

See [`docs/benchmarks/latest.json`](benchmarks/latest.json) and
[`docs/benchmark_report.md`](benchmark_report.md).

Host snapshot at measurement time:

- Apple M4 Pro, 24 GiB RAM, macOS 26.5.2
- `rustc 1.96.0`, release profile
- Example: `core_engine_10k` ≈ 4.6M events/s (local); naive Python 10k ≈ 1.2M events/s

## Profiling

Flamegraph via `cargo flamegraph` requires full Xcode/`xctrace` on this host.
Genuine macOS `sample` output is committed as
[`docs/artifacts/sample_profile.txt`](artifacts/sample_profile.txt) with
[`docs/artifacts/profile_summary.md`](artifacts/profile_summary.md).

## Order-pool experiment conclusion

On this machine (`BENCH_FULL=1 cargo bench --features order_pool --bench order_pool_bench`):

- `vec_push_pop_churn` ≈ 4.14 µs
- `order_pool_insert_remove_reuse` ≈ 14.5 µs

**Conclusion: worse** for this synthetic churn pattern (extra free-list bookkeeping).
Kept isolated behind the `order_pool` feature; **not** integrated into the default engine.

## Lock-free queue

Isolated `crossbeam_queue::ArrayQueue` prototype. Deterministic core remains
single-threaded. No end-to-end latency improvement claim.
