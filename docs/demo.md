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

Final book snapshot (events on stdout, book JSON on stderr):

```bash
cargo run --release --bin engine-cli -- replay data/scenarios/basic_cross.jsonl --book
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

## 4. Strategy simulation

Market making:

```bash
cargo run --release --bin engine-cli -- simulate \
  data/scenarios/market_making_seed.jsonl \
  --strategy market-maker \
  --summary-only
```

Momentum with portfolio JSON:

```bash
cargo run --release --bin engine-cli -- simulate \
  data/scenarios/momentum_seed.jsonl \
  --strategy momentum \
  --portfolio-summary
```

(`strategy-replay` remains available as a compatibility command.)

## 5. Risk rejection demo

Cap order size to force pre-trade rejections:

```bash
cargo run --release --bin engine-cli -- simulate \
  data/scenarios/market_making_seed.jsonl \
  --strategy market-maker \
  --max-order-qty 1 \
  --summary-only
```

Or load `data/config/risk_demo.json`:

```bash
cargo run --release --bin engine-cli -- simulate \
  data/scenarios/market_making_seed.jsonl \
  --strategy market-maker \
  --risk-config data/config/risk_demo.json \
  --summary-only
```

Compare `risk_rejected` count with an unrestricted run.

## 6. Paper WebSocket demo

Offline mock (default, no network):

```bash
cargo run --release --bin engine-cli -- websocket-demo
```

Localhost WebSocket server + client (loopback only):

```bash
cargo run --release --bin engine-cli -- websocket-demo --listen
```

## 7. Benchmark report

```bash
cargo run --release --bin engine-cli -- benchmark-report
# optional: --refresh --charts
```

## 8. Test suite highlights

```bash
# All tests including elite features
cargo test --workspace --all-targets --all-features

# Golden replay scenarios
cargo test -p engine --test deterministic_replay

# Risk, portfolio, strategies, multi-symbol, paper adapter
cargo test -p engine --test risk_controls
cargo test -p engine --test portfolio_accounting
cargo test -p engine --test strategy_demos
cargo test -p engine --test multi_symbol_replay
cargo test -p engine --test paper_adapter

# Property-based invariants
cargo test -p engine --test property_invariants
```

## 9. Benchmarks (smoke vs full)

```bash
cargo bench --workspace --no-run
cargo bench -p engine --bench engine_hot_path
BENCH_FULL=1 cargo bench -p engine --bench engine_hot_path
```

## 10. Python baseline

```bash
python3 python/baseline_lob.py --events 10000
```

## 11. Charts

```bash
python3 scripts/generate_charts.py
open docs/artifacts/throughput_by_workload.png   # macOS; adjust for your OS
```

## 12. Full automated demo

```bash
chmod +x scripts/demo.sh
./scripts/demo.sh
```

## 13. Full verification

```bash
chmod +x scripts/verify_final.sh
./scripts/verify_final.sh
```

## 14. Static dashboard

Open [artifacts/dashboard.html](./artifacts/dashboard.html) for the measured report dashboard
(charts, architecture SVG, and profiler links).

## What to tell reviewers

- **Correctness:** golden-file replay, integration/unit/property tests.
- **Determinism:** integer ticks, no RNG or wall clock in engine paths.
- **Scope:** simulated stack through risk, strategies, and paper MD adapter; not live trading.
- **Performance:** Criterion + Python baseline with machine-specific disclosure.
- **Honesty:** no profitability or exchange-grade latency claims.

## See also

- [README.md](../README.md)
- [replay.md](./replay.md)
- [strategies.md](./strategies.md)
- [risk.md](./risk.md)
