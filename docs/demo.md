# Demo Walkthrough

Step-by-step commands to demonstrate the engine for reviewers. All commands assume the repository root.

## Prerequisites

```bash
rustc --version   # stable, 2021 edition
cargo build --release
```

Optional: `pip install -r python/requirements.txt` for chart generation.

## 1. Golden replay (matching correctness)

Basic crossing scenario:

```bash
cargo run --release --bin engine-cli -- replay data/scenarios/basic_cross.jsonl --summary-only
```

Full output events on stdout, summary on stderr:

```bash
cargo run --release --bin engine-cli -- replay data/scenarios/basic_cross.jsonl
```

Final book snapshot:

```bash
cargo run --release --bin engine-cli -- replay data/scenarios/basic_cross.jsonl --book --summary-only
```

## 2. Determinism check

Two runs must produce identical stdout bytes:

```bash
cargo run --release --bin engine-cli -- replay data/scenarios/basic_cross.jsonl > /tmp/run1.jsonl
cargo run --release --bin engine-cli -- replay data/scenarios/basic_cross.jsonl > /tmp/run2.jsonl
cmp /tmp/run1.jsonl /tmp/run2.jsonl && echo "deterministic OK"
```

## 3. Multi-symbol replay

```bash
cargo run --release --bin engine-cli -- replay \
  data/scenarios/multi_symbol_interleaved.jsonl \
  --multi \
  --summary-only
```

## 4. Strategy replay

Market making:

```bash
cargo run --release --bin engine-cli -- strategy-replay \
  data/scenarios/market_making_seed.jsonl \
  --strategy market_making \
  --summary-only
```

Momentum with portfolio JSON:

```bash
cargo run --release --bin engine-cli -- strategy-replay \
  data/scenarios/momentum_seed.jsonl \
  --strategy momentum \
  --portfolio
```

## 5. Risk rejection demo

Cap order size to force pre-trade rejections:

```bash
cargo run --release --bin engine-cli -- strategy-replay \
  data/scenarios/market_making_seed.jsonl \
  --strategy market_making \
  --max-order-qty 1 \
  --summary-only
```

Compare `risk_rejected` count with an unrestricted run.

## 6. Test suite highlights

```bash
# All tests including elite features
cargo test --workspace --all-targets --all-features

# Golden replay scenarios
cargo test -p engine --test deterministic_replay

# Risk, portfolio, strategies, multi-symbol
cargo test -p engine --test risk_controls
cargo test -p engine --test portfolio_accounting
cargo test -p engine --test strategy_demos
cargo test -p engine --test multi_symbol_replay

# Property-based invariants
cargo test -p engine --test property_invariants
```

## 7. Benchmarks (smoke vs full)

```bash
cargo bench --workspace --no-run
cargo bench -p engine --bench engine_hot_path
BENCH_FULL=1 cargo bench -p engine --bench engine_hot_path
```

## 8. Python baseline

```bash
python3 python/baseline_lob.py --events 10000
```

## 9. Charts

```bash
python3 scripts/generate_charts.py
open docs/artifacts/throughput_by_workload.png   # macOS; adjust for your OS
```

## 10. Full automated demo

```bash
chmod +x scripts/demo.sh
./scripts/demo.sh
```

## 11. Full verification

```bash
chmod +x scripts/verify_final.sh
./scripts/verify_final.sh
```

## 12. Static dashboard

Open [artifacts/dashboard.html](./artifacts/dashboard.html) for the measured report dashboard
(charts, architecture SVG, and profiler links).

## What to tell reviewers

- **Correctness:** golden-file replay, 200+ integration/unit/property tests.
- **Determinism:** integer ticks, no RNG or wall clock in engine paths.
- **Scope:** simulated stack through risk and strategies; not live trading.
- **Performance:** Criterion + Python baseline with machine-specific disclosure.
- **Honesty:** no profitability or exchange-grade latency claims.

## See also

- [README.md](../README.md)
- [replay.md](./replay.md)
- [strategies.md](./strategies.md)
- [risk.md](./risk.md)
