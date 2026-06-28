# Low-Latency Event-Driven Trading Engine

## What this is

A Rust event-driven trading engine focused on deterministic replay, market microstructure correctness, risk controls, benchmarking, and clean systems design.

## What this is not

This is not a profitable trading bot and does not connect to real capital.

## Current Engine Status

Week 2 complete:

- Single-symbol limit order book
- Bid/ask price levels
- FIFO queues per price
- Best bid / best ask
- Order ID index
- Book snapshots
- Internal invariant checks
- Unit and integration tests

## Planned features

- Limit order book
- Price-time priority matching
- Market, limit, and cancel orders
- Partial fills
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
