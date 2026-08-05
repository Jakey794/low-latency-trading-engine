# Design Notes

## Core principles

1. **Correctness before latency claims.** Matching rules, invariants, and golden replay are the foundation.
2. **Deterministic replay before strategies.** Week 5 replay path remains the correctness oracle; runtime extends without breaking goldens.
3. **Integer tick prices.** No floating-point in matching, validation, or JSON price fields.
4. **Single-symbol correctness before multi-symbol scaling.** Each symbol owns a `MatchingEngine`; runtime routes deterministically.
5. **Benchmarks are reproducible and conservatively reported.** Wall clock only in metrics/bench paths.
6. **Strategies never bypass risk.** All intents pass through `RiskManager`; cancels remain after kill switch.
7. **No unsafe in the production path.** Elite experiments are feature-gated and isolated.

## Matching behavior

- `MatchingEngine` is the canonical path for limit orders, market orders, and cancels.
- Crossing orders match the best price first and FIFO order first within a price level.
- Every trade uses the resting order's price and is reported in execution order.
- Limit residuals rest with new FIFO priority; market residuals expire and never rest.
- Completed or cancelled orders leave the active index; empty levels are removed.
- Post-operation invariants require an uncrossed book, positive resting quantities, non-empty levels, and a consistent active-order index.

## Replay decisions

### Single-threaded execution

Replay and runtime event processing are single-threaded so scheduling cannot change observable order. Concurrency belongs only in isolated elite experiments behind features.

### No wall-clock time in deterministic paths

Replay and runtime output timestamps come from input envelopes. Reading the system clock would break golden tests. Order IDs from replay input or runtime counters — no random IDs.

### Integer tick prices

`PriceTicks(i64)` avoids floating-point equality, ordering, rounding, and JSON ambiguity.

### Golden files

Golden tests compare complete serialized output strings. They detect regressions in event order, maker/taker roles, resting-price execution, quantities, sequence propagation, schema fields, and JSON formatting.

## Runtime and strategies

- External events carry strictly increasing `seq`; duplicate/out-of-order fails atomically.
- Strategy callbacks receive read-only context; command budget prevents runaway loops.
- Runtime-assigned order IDs for strategy placements are deterministic.
- Demonstration strategies (`market_making`, `momentum`) illustrate the plugin interface — not profitability.

## Portfolio and risk

- Average-cost accounting with checked `i128` for cash and P&L.
- Pre-trade risk rejects before matching; book and portfolio unchanged on rejection.
- Post-trade loss monitoring can trip the kill switch automatically.
- Portfolio snapshots sort positions by symbol for stable output.

## Multi-symbol routing

- One `MatchingEngine` per symbol; shared portfolio and risk manager.
- CLI `replay --multi` or multi-symbol input files use runtime path.
- Symbol discovery uses sorted `BTreeSet` / stable ordering — not HashMap iteration in public output.

## Benchmarks and metrics

- Criterion bench `engine_hot_path`; full workloads require `BENCH_FULL=1`.
- Bench binary exits immediately in smoke mode so `cargo test --all-targets` stays fast.
- `LatencyCollector` uses hdrhistogram; never wired into matching logic.
- Python baseline is naive dict/list LOB for rough throughput comparison.

## Elite experiments

- `order_pool` — arena-style order reuse prototype.
- `lockfree_queue` — lock-free queue experiment (optional `crossbeam-queue`).
- Neither is required for correctness; both stay behind Cargo features.

## Property-based testing

proptest generates random valid order sequences and asserts book invariants and uncrossed state after operations.

## Current limitations

- No exchange adapter, persistence, or real-capital integration.
- No schema versioning or migration for replay JSONL.
- Batch replay holds full input in memory; no streaming checkpoint resume.
- Benchmark numbers are machine-specific; no exchange-grade latency claims.
- Demonstration strategies are not tuned for profit.
- Kill switch is process-local, not distributed.

## Future directions

- Replay schema versioning
- Richer bench export to `out/bench_summary.json`
- Additional strategies and runtime property tests
- Optional audit log for outputs
