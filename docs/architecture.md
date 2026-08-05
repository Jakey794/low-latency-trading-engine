# Architecture

This document describes the full engine stack: matching, replay, runtime, portfolio, risk, strategies, benchmarks, and isolated elite experiments. The production deterministic path is synchronous and single-process.

## High-level data flow

```text
JSONL / CLI / Strategy intents
        ↓
   Runtime (router)  — or ReplayDriver (Week 5 single-symbol path)
        ↓
  RiskManager ──reject──→ outputs (no book/portfolio mutation)
        ↓ allow
  MatchingEngine(s) per symbol
        ↓ trades
  Portfolio ──loss breach──→ kill switch
        ↓
  Strategy callbacks (read-only context → bounded intents)
        ↓
  Ordered outputs + snapshots
```

See [architecture.mmd](./architecture.mmd) for the Mermaid source used in the README diagram.

## Crates

| Crate | Role |
| --- | --- |
| `engine` | Order book, matching, replay, runtime, portfolio, risk, strategies, metrics, elite |
| `engine-cli` | `replay` and `strategy-replay` commands |

## Order book

`OrderBook` owns mutable bid/ask ladders, FIFO price levels, and an active order-ID index.

- Bids and asks use ordered maps (`BTreeMap`); orders at one price use insertion order (FIFO).
- Invariants: positive quantities, non-empty levels, index consistency, aggregate quantities, uncrossed market after matching.
- `BookSnapshot` is a copied, read-only view (bids best-to-worst descending, asks ascending).

## Matching engine

`MatchingEngine` is the canonical order-processing boundary for one symbol.

- Validates limit, market, and cancel requests.
- Applies price-time priority: best price first, then oldest resting order at that price.
- Every trade executes at the **resting** order's price.
- Limit residuals rest with new FIFO priority; market residuals expire and never rest.
- Returns typed execution reports; replay adapts these into external output events.

## Replay (Week 5 path)

`ReplayDriver` wraps a single `MatchingEngine` for golden-file correctness.

```text
JSONL input → Parser → ReplayEvent → ReplayDriver → MatchingEngine → OrderBook
                                              ↓
                         ReplayOutputEvent + ReplaySummary + BookSnapshot
```

- Strictly increasing sequence numbers; duplicate or out-of-order sequences fail atomically.
- No matching logic in the driver; it delegates to `MatchingEngine`.
- Synchronous, single-threaded, no wall clock or random IDs.

The CLI `replay` command uses this path for single-symbol scenarios. With `--multi` or multiple symbols in the input, it routes through `Runtime` instead.

## Runtime

`Runtime` orchestrates multi-symbol simulation with portfolio, risk, and strategies.

### Components

| Component | Responsibility |
| --- | --- |
| Symbol router | Maps each symbol to its own `MatchingEngine` |
| Sequence validation | Monotonic `seq` across external events |
| `RiskManager` | Pre-trade checks; kill switch |
| `Portfolio` | Cash, positions, P&L updates on fills |
| Strategy registry | Deterministic callbacks with command budget |
| Output collector | Stable-ordered `RuntimeOutput` events |

### Event processing order

1. Validate external event sequence.
2. For new orders: risk check → matching (if allowed) → portfolio update on fills → strategy callbacks.
3. For cancels: always allowed (even under kill switch) → matching → strategy callbacks.
4. Strategy commands are capped per external event (`DEFAULT_MAX_STRATEGY_COMMANDS_PER_EVENT`); nested dispatch depth is bounded.
5. Strategy-generated order IDs are assigned deterministically by the runtime.

### Output kinds

`RuntimeOutput` extends replay outputs with `risk_rejected` and `strategy_commands_dropped` variants.

## Portfolio

`Portfolio` implements average-cost accounting with checked `i128` arithmetic.

- Tracks cash, per-symbol positions (signed qty, avg cost), realized and unrealized P&L, equity.
- Marks from last trade or explicit mark prices for unrealized P&L.
- `PortfolioSnapshot` sorts positions by symbol for deterministic serialization.
- Errors: overflow, invalid quantity/price, missing mark.

Fills from matching update portfolio; risk uses projected position and notional before accepting new orders.

## Risk

`RiskManager` runs **before** matching. Rejections must not mutate the book, portfolio, or strategy state.

### Configurable limits (`RiskLimits`)

| Limit | Behavior |
| --- | --- |
| `max_order_qty` | Single-order quantity cap |
| `max_abs_position` | Projected absolute position after fill |
| `max_gross_notional` | `|price| * qty` on new orders |
| Per-symbol overrides | Symbol-specific qty/position caps |
| `max_total_loss` | Post-trade equity vs starting equity; trips kill switch |

### Kill switch

- Manual activation or automatic trip on loss breach.
- Blocks new orders while active; **cancels remain allowed**.
- Structured `RiskRejectReason` on every rejection.

## Strategies

Strategies implement the `Strategy` trait: read-only `StrategyContext` in, bounded `StrategyCommand` out.

### Built-in demos

| Name | Purpose |
| --- | --- |
| `market_making` | Symmetric quotes around mid with inventory skew |
| `momentum` | Rolling trade-price window with threshold and cooldown |

These are **demonstration strategies only** — not profitability claims.

### Strategy events

`StrategyEvent` includes activation, book updates, trades, fills, order acceptance, timer ticks, and risk rejections. Callbacks must be deterministic (no wall clock, no RNG).

## Metrics and benchmarks

The `metrics` module provides `LatencyCollector` (hdrhistogram) and throughput helpers. Wall-clock timing is confined to benchmark and metrics paths — never used in deterministic replay or matching.

Criterion bench `engine_hot_path` covers:

- Hot path: add, cancel, full match, partial fill, multi-level sweep
- Synthetic workloads (100 events default; 10k/100k with `BENCH_FULL=1`)
- JSONL parse and replay
- Strategy runtime seed
- Multi-symbol interleaved routing

## Elite experiments

Behind optional Cargo features in `engine::elite`:

| Feature | Module | Notes |
| --- | --- | --- |
| `order_pool` | `order_pool` | Arena-style order reuse prototype |
| `lockfree_queue` | `lockfree_queue` | Lock-free queue experiment |

These are isolated measurement prototypes. The default deterministic path does not depend on them and avoids `unsafe` in production code.

## Python baseline

`python/baseline_lob.py` implements a naive dict/list limit order book with the same integer tick semantics for relative throughput comparison. It is not feature-equivalent to the Rust engine (no portfolio, risk, or multi-symbol runtime).

## Determinism guarantees

- Integer tick prices; no floating-point in matching.
- Checked or wider integers for notional and P&L.
- No system time or randomness in deterministic execution paths.
- Stable ordering for outputs and snapshots (no HashMap iteration in public output).
- Golden-file tests compare complete serialized output strings.

## Known gaps

- No exchange adapter, persistence, or live order submission.
- No schema versioning on replay JSONL.
- Batch-only replay (full input in memory).
- Benchmark numbers are machine-specific, not exchange-grade latency claims.
