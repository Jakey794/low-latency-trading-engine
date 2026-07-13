# Design Notes

## Core principles

1. Correctness before latency claims.
2. Deterministic replay before strategies.
3. Integer tick prices, not floating-point prices.
4. Single-symbol correctness before multi-symbol scaling.
5. Benchmarks must be reproducible and conservatively reported.

## Matching behavior

- `MatchingEngine` is the canonical path for limit orders, market orders, and cancels.
- Crossing orders match the best price first and FIFO order first within a price level.
- Every trade uses the resting order's price and is reported in execution order.
- Limit residuals rest with new FIFO priority; market residuals expire and never rest.
- Completed or cancelled orders leave the active index, and empty levels are removed.
- Post-operation invariants require an uncrossed book, positive resting quantities, non-empty levels, and a consistent active-order index.
- Stored timestamps are metadata; arrival/insertion order determines same-price priority.

## Week 5 replay decisions

### Single-threaded execution

Replay is single-threaded so scheduling and interleaving cannot change observable order. This keeps the correctness model explicit while the engine is single-symbol. Concurrency should only be introduced with a deterministic partitioning and ordering design.

### No wall-clock time

Replay output timestamps come from the input envelope. Reading the system clock would make identical files produce different output and would weaken golden tests. The replay path also generates no random IDs.

### Integer tick prices

`PriceTicks(i64)` avoids floating-point equality, ordering, rounding, and JSON representation ambiguity. The same integer price participates in validation, matching, snapshots, and golden output.

### Golden files

Golden tests compare the complete serialized output, not isolated counters. They detect regressions in event order, maker/taker roles, resting-price execution, quantities, sequence/timestamp propagation, schema fields, and JSON formatting. Typed assertions accompany fatal-error goldens so text alone is not the contract.

## Current limitations

- The engine and replay driver are synchronous and single-symbol.
- Replay validates and stores a complete batch in memory; there is no streaming or checkpoint resume.
- Replay schemas are not versioned and have no migration layer.
- Strategy, risk, portfolio, and P&L modules are not connected to order processing.
- There is no exchange adapter, persistence layer, production deployment model, or real-capital integration.
- Benchmarking and profiling remain future work; the project makes no measured latency claim.
- The system demonstrates engine correctness, not a profitable trading strategy.
