# Design Notes

## Core Principles

1. Correctness before latency claims.
2. Deterministic replay before strategies.
3. Integer tick prices, not floating-point prices.
4. Single-symbol correctness before multi-symbol scaling.
5. Benchmarks must be reproducible and conservatively reported.

## Week 3 Matching Behavior

- `MatchingEngine` is the canonical entry point for single-symbol limit orders.
- Non-crossing quantity rests in price-time priority using ordered price maps and FIFO queues.
- Crossing orders match the best price first and the oldest order first within each price level.
- Every fill uses the resting order's price and reports fills in execution order.
- Matching continues across orders and price levels only while the incoming limit crosses.
- A positive incoming residual rests under the incoming order ID with its remaining quantity and new FIFO priority.
- Completed resting orders leave both their queue and the active order-ID index; empty levels are removed.
- Post-submit invariants require an uncrossed book, positive resting quantities, non-empty levels, and a bidirectionally consistent active-order index.

## Current Limitations

- The engine is synchronous and single-symbol.
- Market orders, cancels, replay, risk, portfolio, strategies, and benchmarks are outside Week 3.
- Execution reports describe the incoming order and do not expose maker order IDs.
- The order-ID index tracks active resting orders, so an ID may be reused after its order is fully completed.
- Direct `OrderBook::add_limit_order` bypasses matching; normal order entry should use `MatchingEngine`.
- FIFO insertion order, not the stored timestamp field, determines time priority within a price level.
