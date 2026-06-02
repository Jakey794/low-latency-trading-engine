# Low-Latency Event-Driven Trading Engine

A Rust-based event-driven trading engine focused on market-infrastructure design, deterministic replay, correctness, benchmarking, and clean systems architecture.

## What this is

This project is a portfolio-grade exchange simulator / trading-engine infrastructure project. It will include a limit order book, matching engine, replay system, strategy interface, risk checks, P&L tracking, benchmarks, and profiling artifacts.

## What this is not

This is not a profitable trading bot. It does not make claims about alpha generation or real-money trading performance.

## Planned features

- Limit order book
- Price-time priority matching
- Market, limit, and cancel orders
- Partial fills
- Deterministic event replay
- Strategy plugin interface
- Market-making and momentum demo strategies
- Position and P&L tracking
- Risk limits and kill switch
- Benchmarks
- Latency histogram
- Throughput chart
- Flamegraph/profiler output

## Current status

Week 1, Day 1: repository skeleton, Cargo workspace, crates, folders, and initial documentation.

## Build

cargo build

## Test

cargo test
