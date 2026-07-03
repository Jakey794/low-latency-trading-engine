# Low-Latency Event-Driven Trading Engine

## What this is

A Rust event-driven trading engine focused on deterministic replay, market microstructure correctness, risk controls, benchmarking, and clean systems design.

## What this is not

This is not a profitable trading bot and does not connect to real capital.

## Current Engine Status

Week 3 complete:

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
- Structural and uncrossed-book invariant checks
- Deterministic unit and scenario integration tests

## Planned features

- Market and cancel orders
- Deterministic replay
- Strategy plugin interface
- Position and P&L tracking
- Risk limits and kill switch
- Benchmark suite
- Latency histogram
- Throughput chart
- Flamegraph profiling

## Build

```bash
cargo build
```
