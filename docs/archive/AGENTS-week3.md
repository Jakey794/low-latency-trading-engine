# AGENTS.md

## Purpose

This repository is a portfolio-quality, event-driven trading engine. Prefer correctness, deterministic behavior, clear invariants, and strong tests over claims of low latency. Week 1 and Week 2 are complete; preserve their behavior unless a Week 3 requirement makes a small, justified change necessary.

## Week 3 scope

Week 3 implements limit-order matching and partial fills only.

The `MatchingEngine` must:

- accept valid limit orders;
- rest non-crossing orders on the book;
- match crossing orders against resting liquidity;
- support full fills, partial fills, multiple fills at one price, fills across multiple price levels, and resting residual quantity;
- enforce price-time priority;
- price every trade at the resting order's price;
- leave the book uncrossed after each successfully processed order;
- remove fully filled orders and empty price levels; and
- keep the order-ID index consistent with all remaining resting orders.

Do not implement or expand any of the following during Week 3:

- market orders;
- cancels;
- replay;
- strategies;
- risk controls;
- portfolio or P&L;
- benchmarking or latency claims;
- async or multithreading;
- lock-free queues or memory pools;
- dashboards; or
- exchange adapters.

Some out-of-scope types and module stubs already exist. Leave them in place for API compatibility, but do not connect them to the Week 3 matching path.

## Repository map

- `crates/engine/src/types.rs`: shared order, side, integer price, and quantity types.
- `crates/engine/src/events.rs`: existing input/output and trade event types.
- `crates/engine/src/book/order_book.rs`: price ladders, resting-order index, snapshots, and book invariants.
- `crates/engine/src/book/price_level.rs`: FIFO queue for one price level.
- `crates/engine/src/matching/engine.rs`: Week 3 matching implementation.
- `crates/engine/tests/order_book_core.rs`: existing Week 2 integration coverage.
- `crates/engine/tests/integration_matching.rs`: required Week 3 end-to-end matching scenarios.

Keep unit tests in the same module as the implementation they exercise. Put public, multi-step matching scenarios in `crates/engine/tests/integration_matching.rs` (the engine crate's `tests/integration_matching.rs`).

## Data-structure and API rules

- Prices are integer ticks using `PriceTicks`; never introduce floating-point prices.
- Quantities use `Qty`. Reject zero-quantity input and never store a zero-quantity resting order.
- Preserve existing public APIs unless there is a strong correctness reason to change one. Keep changes narrow and explain any public API change.
- Prefer the existing `BTreeMap<PriceTicks, PriceLevel>` ladders and `VecDeque<RestingOrder>` price-level queues.
- Use the `BTreeMap` ordering directly: highest bid via reverse order and lowest ask via forward order.
- Use queue-front matching and append residual/new resting orders at the back. Do not sort a price level by timestamp or order ID.
- Do not rely on `HashMap` iteration order for externally observable behavior. The order-ID map is an index, not a sequencing mechanism.
- Keep the matching loop readable. Avoid premature abstractions or performance-oriented unsafe code.

## Matching semantics

For an incoming buy limit order, match while the best ask is less than or equal to the buy limit. For an incoming sell limit order, match while the best bid is greater than or equal to the sell limit.

At each match:

1. Select the best opposing price.
2. Select the oldest resting order at that price (the queue front).
3. Fill `min(incoming_remaining, resting_remaining)`.
4. Emit the trade at the resting order's price, with the resting order as maker and incoming order as taker.
5. Decrease both remaining quantities.
6. If the resting order reaches zero, remove it from the queue and order-ID index.
7. If its price level becomes empty, remove the price level.
8. Continue until the incoming quantity is zero or the next opposing price does not cross.
9. If a positive incoming residual remains, rest it at its original limit price at the back of that price level and index it exactly once.

Trade output order must match execution order: best price first, then FIFO within a price. Input timestamps may be retained as metadata, but arrival/insertion order determines time priority for orders at the same price.

Validate an incoming order before mutating the book. Error paths should not leave partial fills, stale index entries, empty levels, or other partial state unless an existing public API explicitly documents that behavior.

## Required invariants

After every successful operation, and in focused tests after important intermediate scenarios, verify:

- `best_bid < best_ask` whenever both sides are non-empty;
- every resting order has positive quantity;
- every price level is non-empty;
- each resting order is stored on the side and price recorded by the order itself;
- each resting order has exactly one correct order-ID index entry;
- every order-ID index entry points to an existing resting order;
- level aggregate quantity equals the sum of its resting orders without overflow;
- FIFO order is preserved among orders at the same price; and
- emitted trade quantities are positive and do not exceed either participant's remaining quantity.

Extend the existing invariant checker where practical instead of duplicating invariant logic in the matching engine.

## Testing requirements

Every behavior change needs a test. Do not delete an existing test unless it is incorrect; explain the reason if deletion is unavoidable.

Add focused unit tests close to `PriceLevel`, `OrderBook`, and `MatchingEngine` changes. Add integration tests covering at least:

- a non-crossing buy rests;
- a non-crossing sell rests;
- an exact full fill removes both completed quantities;
- an incoming order partially fills a resting order;
- an incoming order fully consumes a resting order and rests its residual;
- one incoming order fills multiple FIFO orders at the same price;
- one incoming order fills across multiple price levels in best-price order;
- same-price resting orders execute in insertion order;
- buy and sell paths are symmetric;
- each trade uses the resting order's price;
- a limit price stops matching before a non-crossing level;
- no crossed book, empty level, or zero-quantity order remains;
- fully filled resting IDs disappear from the index while partially filled and residual resting IDs remain correct; and
- invalid, zero-quantity, missing-price, non-limit, symbol-mismatched, and duplicate-ID inputs do not corrupt state.

Prefer assertions on the complete ordered trade list and final book snapshot over isolated counts. Test order IDs, maker/taker roles, prices, quantities, remaining quantities, level order, and index-backed lookup results.

## Required verification

Run all commands from the repository root before claiming the work is complete:

```bash
cargo test
cargo fmt --check
cargo clippy -- -D warnings
```

Do not claim completion if any command fails. Report the failing command and relevant error. Documentation-only changes should still preserve a clean working tree apart from the intended files and should not weaken these gates for subsequent Week 3 work.

## Completion standard

Week 3 is complete only when the matching behavior and invariants above are implemented, unit and integration tests cover the full scenario matrix, all existing tests still pass, and all three required verification commands succeed. Keep the final report factual: summarize behavior delivered, tests added, any justified API changes, and verification results.
