# Benchmark Report

> **Machine-specific results.** Values below come from
> [`docs/benchmarks/latest.json`](benchmarks/latest.json), collected with the
> release `measure` harness (`hdrhistogram`). This project makes **no
> exchange-grade latency claims** and **no profitability claims**.

## Environment

| Field | Value |
| --- | --- |
| Date (UTC) | 2026-08-05T15:49:51Z |
| Machine model | Mac16,8 |
| CPU | Apple M4 Pro |
| RAM | 24 GiB |
| OS | Darwin 25.5.0 arm64 / macOS 26.5.2 |
| Rust | rustc 1.96.0 (ac68faa20 2026-05-25) |
| Build | release |
| Harness | `cargo run --release -p engine-cli --bin measure` |

## Workloads (selected)

| Workload | Samples | p50 | p99 | Events/s |
| --- | --- | --- | --- | --- |
| add_non_crossing_limit | 20000 | 209 ns | (see JSON) | ≈4.04M |
| cancel_resting_order | 20000 | 83 ns | | ≈3.10M |
| full_match | 20000 | 125 ns | | ≈2.63M |
| partial_fill | 20000 | 125 ns | | ≈2.59M |
| multi_level_sweep | 20000 | 375 ns | | ≈0.83M |
| core_engine_10k | 10000 | 125 ns | | ≈4.61M |
| core_engine_100k | 100000 | 166 ns | | ≈2.32M |
| jsonl_parse_only | 5000 | 1167 ns | | ≈0.73M |
| jsonl_replay_only | 5000 | 417 ns | | ≈1.77M |
| strategy_runtime_seed | 2000 | 3543 ns | | ≈0.24M |
| multi_symbol_interleaved | 1000 | 101631 ns/batch | | ≈1.82M order-events |

Full percentiles including p90/p95/p99/p99.9/max are in `latest.json`.

## Rust vs naive Python (10k events)

| Implementation | Events/s |
| --- | --- |
| Rust `core_engine_10k` | ≈4.61M |
| Python `baseline_lob.py` | ≈1.16M–1.21M |

The Python LOB is a **naive correctness-oriented baseline**, not an optimized competitor.

## Charts

- [`artifacts/latency_histogram.png`](artifacts/latency_histogram.png)
- [`artifacts/throughput_chart.png`](artifacts/throughput_chart.png)
- [`artifacts/rust_vs_python.png`](artifacts/rust_vs_python.png)

## Regenerate

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r python/requirements.txt
cargo run --release -p engine-cli --bin measure
python scripts/generate_charts.py
# Optional Criterion companion:
BENCH_FULL=1 cargo bench --workspace
```

## Order-pool microbench (isolated)

| Variant | Time (approx) |
| --- | --- |
| Vec push/pop churn | ≈4.14 µs |
| OrderPool insert/remove/reuse | ≈14.5 µs |

**Factual conclusion: worse** on this synthetic pattern; not enabled by default.
