# AGENTS.md

## Mission

Maintain this repository as a portfolio-quality Rust event-driven trading engine.
Weeks 1–5 (book, matching, deterministic replay) are complete and must not regress.
The elite final scope (portfolio, risk, strategies, multi-symbol, metrics, elite
experiments, paper WebSocket adapter, docs, CI) is implemented; extend carefully
with tests and honest performance claims.

## Architecture rules

- Clear boundaries: ingress/replay → runtime coordinator → risk → matching →
  portfolio → strategy callbacks → strategy intents (back through risk).
- One matching engine / book per symbol; default coordinator is single-threaded
  and deterministic.
- Integer-tick prices only in matching and accounting paths.
- Checked / wide integer arithmetic (`i128`) for notional, cash, and P&L.
- No wall-clock time or randomness on deterministic execution paths.
- Do not expose `HashMap` iteration order through public output (use sorted maps
  or explicit ordering for snapshots and reports).
- Avoid `unsafe` in the production engine. Elite experiments stay behind features
  (`order_pool`, `lockfree_queue`) or isolated modules.
- Strategies observe read-only context and emit bounded intents; they never mutate
  book, portfolio, or risk state directly.

## Invariants

- Book is never crossed after successful processing.
- No resting order with zero quantity; no empty price levels.
- Order-ID index stays consistent with resting orders.
- Price-time (FIFO) priority at a price level.
- Executed quantity never exceeds submitted quantity; fills conserve quantity.
- Risk rejection does not mutate book or portfolio.
- Cancels remain allowed while the kill switch is active.
- Repeated replay of the same input produces identical ordered output.
- Multi-symbol processing does not leak state between symbols.

## Non-goals

- Do not claim profitability, exchange-grade latency, or production readiness.
- Do not connect to real capital or submit live exchange orders.
- Do not add Kubernetes, databases, cloud deployment, or unrelated infrastructure.
- Do not fabricate benchmark or profiler artifacts.
- Do not weaken or delete existing tests to make new code pass.

## Contributor rules

- Correctness and determinism come before performance.
- Every behavior change requires tests.
- Prefer safe Rust; justify any new dependency.
- Keep experimental optimization code isolated.
- Do not push, merge, force-reset, clean untracked files, use sudo, or modify secrets
  unless the user explicitly requests that operation.

## Required verification

Before claiming completion of a change set, run:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
cargo test --workspace --release
cargo bench --workspace --no-run
```

Prefer `./scripts/verify_final.sh` for the full portfolio gate (demos, charts,
artifact checks). Do not claim success if any required command fails.
