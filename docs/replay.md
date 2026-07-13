# Deterministic replay

Replay processes explicit, ordered JSONL events through the same `MatchingEngine` used by normal order entry. Each non-empty line contains one object. Blank lines are ignored; malformed lines report their physical line number.

## Input schema

Every input has an envelope:

| Field | Type | Meaning |
| --- | --- | --- |
| `seq` | `u64` | Strictly increasing replay sequence |
| `ts_ns` | `u64` | Event timestamp supplied by the input |
| `kind` | string | `new_order` or `cancel` |

New order:

```json
{"seq":1,"ts_ns":100,"kind":"new_order","order":{"order_id":1001,"symbol":"AAPL","side":"Buy","order_type":"Limit","price":10000,"qty":10,"timestamp_ns":90,"strategy_id":null}}
```

The nested order contains `order_id`, `symbol`, `side`, `order_type`, `price`, `qty`, `timestamp_ns`, and optional `strategy_id`. Limit orders require a positive integer-tick price. Market orders use `"order_type":"Market"` and `"price":null`. Quantity must be positive.

Cancel:

```json
{"seq":2,"ts_ns":200,"kind":"cancel","order_id":1001,"symbol":"AAPL"}
```

## Output schema

Output records retain the triggering input's `seq` and `ts_ns`. One input may emit multiple records.

| `kind` | Payload |
| --- | --- |
| `accepted` | `order_id` |
| `rejected` | optional `order_id`, typed `reason` |
| `trade` | symbol, taker/maker IDs, integer price and quantity, aggressor side, timestamp |
| `cancelled` | `order_id` |
| `expired` | `order_id`, unfilled `remaining` quantity |

```json
{"seq":2,"ts_ns":200,"kind":"trade","trade":{"symbol":"AAPL","taker_order_id":1002,"maker_order_id":1001,"price":10000,"qty":10,"aggressor_side":"Buy","timestamp_ns":200}}
```

Resting and fill execution reports are normalized into replay outputs: accepted orders emit `accepted`, each fill emits one `trade`, and an unfilled market remainder emits `expired`. A limit residual is represented in the final book snapshot rather than a separate replay event.

## Determinism guarantees

- Replay is synchronous and single-threaded.
- Events are processed in validated sequence order.
- Bid/ask ladders use ordered maps; price levels use FIFO queues.
- Trades execute best price first, then oldest resting order, at the resting price.
- Prices use integer ticks; no floating-point comparisons or JSON values are introduced.
- Event and trade timestamps come from replay input; no wall clock is read.
- IDs come from replay input; no random IDs are generated.
- Output vectors and book snapshots have stable ordering and serde field order.
- The replay driver delegates matching and cancellation to `MatchingEngine`.

## Sequence-number rules

Sequence numbers may start at any `u64` value but must strictly increase across the lifetime of a `ReplayDriver`.

- Equal adjacent/current values return `ReplayError::DuplicateSequence`.
- A lower value returns `ReplayError::OutOfOrderSequence`.
- The complete batch is validated before matching starts.
- A sequence failure emits no output, leaves the book unchanged, and does not advance the driver's last sequence.

## Scenario suite

Successful scenarios compare exact `.out.jsonl` files. Fatal sequence scenarios compare `.err.txt` files and separately assert the typed error and atomic state.

| Scenario | Setup | What it proves |
| --- | --- | --- |
| `basic_cross` | Resting sell, then crossing buy | Trade uses the resting price |
| `partial_fills` | One sell consumed by two buys | Remaining quantity is correct across fills |
| `cancels` | Resting buy, then cancel | Active resting liquidity can be removed |
| `empty_book_market_order` | Market buy without liquidity | Empty-book rejection; market orders never rest |
| `multi_level_fill` | Two ask levels, then aggressive buy | Best price executes before the next level |
| `fifo_priority` | Two same-price asks, then consuming buy | Earlier resting maker executes first |
| `cancel_after_partial_fill` | Partial fill, then cancel maker residual | Partially filled resting orders remain cancellable |
| `crossed_book_prevention` | Two crossing phases | Crosses trade instead of remaining on both sides |
| `duplicate_seq_rejected` | Sequence 1 followed by sequence 1 | Duplicate detection and atomic failure |
| `out_of_order_seq_rejected` | Sequence 2 followed by sequence 1 | Monotonic ordering and atomic failure |

## Add a scenario

1. Add a minimal input under `data/scenarios/<name>.jsonl` with fixed IDs and timestamps.
2. Add `data/expected/<name>.out.jsonl`, or `<name>.err.txt` for a fatal replay error.
3. Add a test to `crates/engine/tests/deterministic_replay.rs` using the shared success or error helper.
4. Assert final snapshot or typed-error state when output alone does not prove the invariant.
5. Add the scenario to the table above.
6. Run:

```bash
cargo test -p engine --test deterministic_replay
cargo test
cargo fmt --check
cargo clippy -- -D warnings
```

Do not update a golden merely to accept unexplained behavior. Diagnose mismatches and fix a real engine/replay defect when one exists.

## Known limitations

- One `MatchingEngine` and symbol are used per driver; the CLI infers the symbol from the first event.
- The parser and driver hold a complete replay batch in memory for atomic sequence validation.
- Empty CLI input cannot supply a symbol and is rejected, although the driver itself supports an empty batch.
- There is no streaming replay, checkpointing, schema version negotiation, or backward-compatibility migration layer.
- There is no live exchange adapter or persistence layer.
- Strategy, risk, portfolio, and P&L modules are not connected to the replay path.
- Replay is a correctness tool, not evidence of profitability or production trading readiness.
