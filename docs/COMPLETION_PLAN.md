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
| 8 | Docs, dashboard, CI, demo/verify scripts | **In progress** |

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

Full documentation: [docs/architecture.md](./architecture.md)

## Verification gates (every phase)

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
```

## Final phase gates

```bash
cargo test --workspace --release
cargo bench --workspace --no-run
./scripts/verify_final.sh
```

`verify_final.sh` additionally runs:

- Deterministic replay twice + byte compare (`basic_cross`)
- CLI strategy-replay smoke
- Multi-symbol replay smoke
- Python baseline smoke
- Chart script smoke (if matplotlib available)
- `git diff --check`

## Phase 8 deliverables

| Deliverable | Path |
| --- | --- |
| README | `README.md` |
| Architecture | `docs/architecture.md`, `docs/architecture.mmd` |
| Topic docs | `docs/portfolio.md`, `docs/risk.md`, `docs/strategies.md`, `docs/performance.md`, `docs/demo.md` |
| Benchmark template | `docs/benchmark_report.md` |
| Updated replay/design notes | `docs/replay.md`, `docs/design_notes.md` |
| Dashboard shell | `docs/artifacts/dashboard.html` |
| Demo script | `scripts/demo.sh` |
| Verify script | `scripts/verify_final.sh` |
| CI workflow | `.github/workflows/ci.yml` |

## Post-completion

- User review of portfolio materials
- Optional: populate `out/bench_summary.json` and regenerate charts on target demo machine
- Optional: commit and open PR when requested
