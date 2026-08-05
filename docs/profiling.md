# Profiling

This document describes how to collect genuine profiler artifacts for the
release-mode engine. Do not fabricate flamegraphs or sample output.

## Goals

- Identify hot functions under a deterministic synthetic workload
- Keep wall-clock profiling **out** of the deterministic matching/runtime path
- Commit real artifacts when the environment supports them; otherwise document
  the exact retry command

## Recommended workloads

```bash
# Measurement harness (also used for latency JSON)
cargo build --release -p engine-cli --bin measure
./target/release/measure --micro-iters 100000

# Strategy/runtime path
cargo run --release --bin engine-cli -- simulate \
  data/scenarios/market_making_seed.jsonl \
  --strategy market-maker \
  --summary-only
```

## Linux (`perf` / `cargo flamegraph`)

```bash
# Install once (example)
cargo install flamegraph

CARGO_PROFILE_RELEASE_DEBUG=true BENCH_FULL=1 \
  cargo flamegraph --bin measure -p engine-cli \
  -o docs/artifacts/flamegraph.svg -- --micro-iters 5000
```

Alternative with `perf` directly:

```bash
cargo build --release -p engine-cli --bin measure
perf record -F 99 -g -- ./target/release/measure --micro-iters 100000
perf script | stackcollapse-perf.pl | flamegraph.pl > docs/artifacts/flamegraph.svg
```

## macOS

Preferred when full Xcode / `xctrace` is available:

```bash
# Point xcode-select at full Xcode if needed:
# sudo xcode-select -s /Applications/Xcode.app/Contents/Developer

CARGO_PROFILE_RELEASE_DEBUG=true \
  cargo flamegraph --bin measure -p engine-cli \
  -o docs/artifacts/flamegraph.svg -- --micro-iters 5000
```

Fallback when only Command Line Tools are installed (no `xctrace`):

```bash
cargo build --release -p engine-cli --bin measure
./target/release/measure --micro-iters 100000 &
MPID=$!
sample $MPID 5 -file docs/artifacts/sample_profile.txt
wait $MPID
```

## Committed artifacts

| Artifact | Status |
| --- | --- |
| [`docs/artifacts/sample_profile.txt`](./artifacts/sample_profile.txt) | Present (macOS `sample`) |
| [`docs/artifacts/profile_summary.md`](./artifacts/profile_summary.md) | Present |
| [`docs/artifacts/flamegraph.svg`](./artifacts/flamegraph.svg) | Environment-dependent |

See [profile_summary.md](./artifacts/profile_summary.md) for observed hot paths
and the exact commands used on the measurement host.

## Interpretation rules

- Profiles are **machine-specific**.
- They are **not** evidence of exchange-grade or HFT latency.
- Optimize only with measurement; keep elite experiments feature-gated.
- The deterministic core must remain independently testable without profilers.
