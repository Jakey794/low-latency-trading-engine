# Architecture

The implemented Week 5 path is deliberately synchronous and narrow:

```text
JSONL input
    ↓
Parser → ReplayEvent
    ↓
ReplayDriver
    ↓
MatchingEngine
    ↓
OrderBook
    ↓
Execution reports + maker-aware trades
    ↓
ReplayOutputEvent + ReplaySummary + BookSnapshot
```

## Replay Driver

`ReplayDriver` owns a `MatchingEngine`, validates complete event batches, enforces strictly increasing sequence numbers across calls, and converts replay inputs to existing engine input events. It maps engine reports and trades into ordered replay outputs and captures the final summary and book snapshot. It contains no matching rules.

## Matching Engine

`MatchingEngine` is the canonical order-processing boundary. It validates limit, market, and cancel requests; applies price-time priority; records order lifecycle state; and returns execution reports. Replay uses a crate-private adapter to capture maker-aware trades from the same matching loop without changing the public matching API.

## Order Book

`OrderBook` owns the mutable bid/ask ladders, FIFO price levels, and active order-ID index. Bids and asks use ordered maps, and orders at one price use insertion order. Book invariants cover positive quantities, non-empty levels, index consistency, aggregate quantities, and an uncrossed market after matching.

## Output Events

`ReplayOutputEvent` is the external replay record. Its envelope carries the triggering sequence and input timestamp; its tagged payload is accepted, rejected, trade, cancelled, or expired. Multiple records from one input retain execution order.

## Book Snapshot

`BookSnapshot` and `PriceLevelSnapshot` are copied, read-only views returned by replay results and used by `--book`. Bids are best-to-worst (descending), asks are best-to-worst (ascending), and each level contains price, aggregate quantity, order count, and copied FIFO order IDs. Callers cannot mutate the engine through a snapshot.

Strategy, risk, portfolio, and exchange-adapter stubs are outside the Week 5 replay path.
