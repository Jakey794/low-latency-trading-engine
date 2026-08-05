# Performance and Benchmarking

Performance work in this repository is **conservatively reported**. Benchmarks measure this codebase on your machine; they do not claim exchange-grade latency or production throughput.

## Principles

1. Correctness and determinism come before speed claims.
2. Wall-clock timing is confined to `metrics` and Criterion benches — never in replay or matching logic.
3. Results are machine-specific; always disclose hardware and compiler when publishing numbers.
4. Python baseline provides a naive reference point, not a rigorous apples-to-apples feature comparison.

## Criterion benchmark suite

Bench target: `engine_hot_path` (`crates/engine/benches/engine_hot_path.rs`).

### Groups

| Group | What it measures |
| --- | --- |
| `hot_path` | Single-event operations: add, cancel, full match, partial fill, multi-level sweep |
| `workloads` | Synthetic mixed add/cancel workloads |
| `replay` | JSONL parse + `basic_cross` replay |
| `strategy_runtime_seed` | Runtime with market-making strategy |
| `multi_symbol_interleaved` | Two-symbol interleaved routing |

### Running benchmarks

Compile only (CI-safe):

```bash
cargo bench --workspace --no-run
```

Smoke (bench binary exits unless opted in):

```bash
cargo bench -p engine --bench engine_hot_path
# stderr: engine_hot_path: smoke ok (set BENCH_FULL=1 for Criterion measurements)
```

Full measurements:

```bash
BENCH_FULL=1 cargo bench -p engine --bench engine_hot_path
```

With `BENCH_FULL=1`, workload sizes include 10,000 and 100,000 events (default smoke uses 100).

## Latency and throughput metrics

The `metrics` module provides:

- `LatencyCollector` — hdrhistogram percentiles (p50, p90, p95, p99, max, mean)
- `throughput_report` — events/sec, trades/sec, cancels/sec
- `MetricsSummary` — serializable workload report

These types are used inside bench helpers and optional export to `out/bench_summary.json`.

## Python baseline

Naive dict/list limit order book:

```bash
python3 python/baseline_lob.py --events 10000
```

Outputs JSON with `events_per_second`, trade count, and elapsed time. Uses `time.perf_counter()` for measurement only in the baseline script — not in Rust deterministic paths.

## Chart generation

```bash
python3 scripts/generate_charts.py
```

Reads `out/bench_summary.json` when present; writes:

- `docs/artifacts/latency_histogram.png`
- `docs/artifacts/throughput_by_workload.png`
- `docs/artifacts/rust_vs_python.png`

Charts are generated only from measured `docs/benchmarks/latest.json`. The chart
script rejects missing metadata and refuses to invent latency values.

## Profiling and flamegraphs

Helper script:

```bash
./scripts/profile_flamegraph.sh
```

Manual command (requires `cargo install flamegraph` and OS profiler permissions):

```bash
BENCH_FULL=1 cargo flamegraph --bench engine_hot_path -o docs/artifacts/flamegraph.svg
```

macOS may block DTrace without elevated privileges; the script documents blockers instead of requiring sudo.

## Elite experiments

Optional features for isolated prototypes:

```bash
cargo test -p engine --features order_pool
cargo test -p engine --features lockfree_queue
```

These modules are not on the default hot path and exist for portfolio discussion of optimization techniques.

## Reporting checklist

When publishing benchmark results:

1. Record CPU model, OS, Rust version (`rustc --version`).
2. Note `BENCH_FULL=1` and exact command used.
3. Run multiple iterations; report variance if significant.
4. State clearly: **not exchange-grade latency**.
5. Regenerate charts from your `out/bench_summary.json`.

See [benchmark_report.md](./benchmark_report.md) for the report template.

## See also

- [benchmark_report.md](./benchmark_report.md)
- [demo.md](./demo.md)
- [architecture.md](./architecture.md) — metrics module boundaries
