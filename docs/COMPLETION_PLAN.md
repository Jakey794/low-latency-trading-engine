# Completion Plan

Finish the event-driven trading engine as a recruiter-ready portfolio project.
Preserve all existing matching, cancel, invariant, and golden-replay behavior.

## Constraints

- Integer tick prices and quantities; `i128` for notional/cash/P&L
- Deterministic core: no wall clock, randomness, or HashMap output order
- No unsafe in production path; elite experiments behind features
- Strategies never bypass risk; cancels remain available after kill switch
- Commit after each phase when gates pass; never push/merge unless requested
- No profitability or exchange-grade latency claims

## Phases

| Phase | Scope | Status |
| --- | --- | --- |
| 1 | Portfolio + P&L (avg-cost, realized/unrealized, equity, snapshots) | Done |
| 2 | Risk limits + kill switch (pre-trade, post-trade loss trip) | Done |
| 3 | Runtime orchestration + strategy trait | Done |
| 4 | Market-making + momentum demos + CLI + scenarios | Done |
| 5 | Multi-symbol router/replay | Done |
| 6 | Criterion benches, latency metrics, Python baseline, charts | Done |
| 7 | proptest, order-pool feature, lock-free queue experiment | Done |
| 8 | Docs, dashboard, CI, demo/verify scripts | Done |
| 9 | Paper WebSocket adapter, simulate/benchmark CLI, packaging polish | Done |

## Architecture sketch

```text
JSONL / CLI / Paper MD / Strategy intents
        ↓
   Runtime (router)
        ↓
  RiskManager ──reject──→ outputs (no book/portfolio mutation)
        ↓ allow
  MatchingEngine(s) per symbol
        ↓ trades
  Portfolio ──loss breach──→ kill switch
        ↓
  Strategy callbacks (read-only context → bounded intents)
```

Full documentation: [docs/architecture.md](./architecture.md)

## Verification gates

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
cargo test --workspace --release
cargo bench --workspace --no-run
./scripts/verify_final.sh
```
