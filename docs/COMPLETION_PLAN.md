# Completion Plan

Finish the Week-5 baseline as a recruiter-ready event-driven trading engine.
Preserve all existing matching, cancel, invariant, and golden-replay behavior.

## Constraints

- Integer tick prices and quantities; `i128` for notional/cash/P&L
- Deterministic core: no wall clock, randomness, or HashMap output order
- No unsafe in production path; elite experiments behind features
- Strategies never bypass risk; cancels remain available after kill switch
- Commit after each phase when gates pass; never push/merge

## Phases

| Phase | Scope | Commit when |
| --- | --- | --- |
| 1 | Portfolio + P&L (avg-cost, realized/unrealized, equity, snapshots) | Unit + integration tests pass |
| 2 | Risk limits + kill switch (pre-trade, post-trade loss trip) | Boundary/atomicity tests pass |
| 3 | Runtime orchestration + strategy trait | Deterministic strategy integration tests pass |
| 4 | Market-making + momentum demos + CLI + scenarios | Golden/strategy tests pass |
| 5 | Multi-symbol router/replay | Isolation + interleaved scenarios pass |
| 6 | Criterion benches, latency metrics, Python baseline, charts | Benches compile; gates pass |
| 7 | proptest, order-pool feature, lock-free queue experiment | Feature tests pass |
| 8 | Docs, dashboard, CI, demo/verify scripts, flamegraph attempt | `scripts/verify_final.sh` passes |

## Architecture sketch

```text
JSONL / CLI / Strategy intents
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

## Verification gates (every phase)

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
```

Final phase additionally runs release tests, `cargo bench --no-run`, dual replay
byte-compare, CLI scenarios, Python smoke, and `scripts/verify_final.sh`.
