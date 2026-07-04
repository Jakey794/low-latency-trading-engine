# Low-Latency Event-Driven Trading Engine

## What this is

A Rust event-driven trading engine focused on deterministic replay, market microstructure correctness, risk controls, benchmarking, and clean systems design.

## What this is not

This is not a profitable trading bot and does not connect to real capital.

## Current Engine Status

Week 4 complete:

- Single-symbol limit order book
- Bid/ask price levels
- FIFO queues per price
- Best bid / best ask
- Order ID index
- Book snapshots
- Price-time priority limit-order matching
- Full, partial, multi-order, and multi-level fills
- Resting-price execution reports
- Residual quantity resting
- Market-order matching and multi-level sweeps
- Active-order cancellation
- Typed execution and rejection reports
- Structural and uncrossed-book invariant checks
- Deterministic unit and scenario integration tests

## Planned features

- Deterministic replay
- Strategy plugin interface
- Position and P&L tracking
- Risk limits and kill switch
- Benchmark suite
- Latency histogram
- Throughput chart
- Flamegraph profiling

## Matching Engine Semantics

### Limit orders

A limit order matches immediately if it crosses the opposite side of the book. Matching uses price-time priority. Trade price is the resting order price. Any unfilled remainder rests on the book at its limit price.

### Market orders

A market order consumes available liquidity from the opposite side of the book, starting at the best price and sweeping price levels as needed. Market orders never rest. If insufficient liquidity exists, the engine fills the available quantity and expires the remainder through an execution report.

### Cancels

Cancel requests operate only on active resting orders. Cancelling an unknown, fully filled, already cancelled, expired, or never-rested aggressive order returns a typed rejection. Cancelling a partially filled resting order removes only its remaining quantity.

### Invariants

After processing orders, the book must not be crossed, must not contain zero-quantity orders, must not contain empty price levels, and the active order lookup must match the actual resting book contents.

## Build

```bash
cargo build
```
