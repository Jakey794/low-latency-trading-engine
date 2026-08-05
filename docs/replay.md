# Deterministic replay

Replay processes explicit, ordered JSONL events through the matching engine (single-symbol `ReplayDriver`) or the multi-symbol `Runtime` (CLI `--multi` or multiple symbols in input). Each non-empty line contains one object. Blank lines are ignored; malformed lines report their physical line number.

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

### ReplayDriver (single-symbol)

Output records retain the triggering input's `seq` and `ts_ns`. One input may emit multiple records.

| `kind` | Payload |
| --- | --- |
| `accepted` | `order_id` |
| `rejected` | optional `order_id`, typed `reason` |
| `trade` | symbol, taker/maker IDs, integer price and quantity, aggressor side, timestamp |
| `cancelled` | `order_id` |
| `expired` | `order_id`, unfilled `remaining` quantity |

### Runtime (multi-symbol / `--multi`)

Same kinds plus:

| `kind` | Payload |
| --- | --- |
| `risk_rejected` | optional `order_id`, `reason` (risk) |
| `strategy_commands_dropped` | `strategy_id`, `dropped` count |

```json
{"seq":2,"ts_ns":200,"kind":"trade","trade":{"symbol":"AAPL","taker_order_id":1002,"maker_order_id":1001,"price":10000,"qty":10,"aggressor_side":"Buy","timestamp_ns":200}}
```

Resting and fill execution reports are normalized into replay outputs: accepted orders emit `accepted`, each fill emits one `trade`, and an unfilled market remainder emits `expired`. A limit residual is represented in the final book snapshot rather than a separate replay event.

## CLI commands

Single-symbol replay (Week 5 path):

```bash
cargo run --release --bin engine-cli -- replay data/scenarios/basic_cross.jsonl
cargo run --release --bin engine-cli -- replay data/scenarios/basic_cross.jsonl --summary-only
cargo run --release --bin engine-cli -- replay data/scenarios/basic_cross.jsonl --book
cargo run --release --bin engine-cli -- replay data/scenarios/basic_cross.jsonl --output out/events.jsonl
```

Multi-symbol runtime replay:

```bash
cargo run --release --bin engine-cli -- replay data/scenarios/multi_symbol_interleaved.jsonl --multi --summary-only
```

Strategy replay (runtime + built-in strategy):

```bash
cargo run --release --bin engine-cli -- strategy-replay data/scenarios/market_making_seed.jsonl --strategy market_making
```

Stdout carries JSONL events (unless `--summary-only`); stderr carries summaries and optional `--book` / `--portfolio` JSON.

## Determinism guarantees

- Replay is synchronous and single-threaded.
- Events are processed in validated sequence order.
- Bid/ask ladders use ordered maps; price levels use FIFO queues.
- Trades execute best price first, then oldest resting order, at the resting price.
- Prices use integer ticks; no floating-point comparisons or JSON values are introduced.
- Event and trade timestamps come from replay input; no wall clock is read in engine paths.
- IDs come from replay input (or runtime counters for strategy orders); no random IDs are generated.
- Output vectors and book snapshots have stable ordering and serde field order.
- Two identical replays produce identical stdout bytes (verified in `scripts/verify_final.sh`).

## Sequence-number rules

Sequence numbers may start at any `u64` value but must strictly increase across the lifetime of a `ReplayDriver` or `Runtime` session.

- Equal adjacent/current values return duplicate-sequence errors.
- A lower value returns out-of-order-sequence errors.
- The complete batch is validated before matching starts (replay driver).
- A sequence failure emits no output, leaves state unchanged, and does not advance the last sequence.

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

### Strategy and multi-symbol scenarios

| Scenario | Path | What it proves |
| --- | --- | --- |
| `market_making_seed` | `strategy-replay --strategy market_making` | Deterministic strategy quotes |
| `momentum_seed` | `strategy-replay --strategy momentum` | Deterministic momentum signals |
| `multi_symbol_interleaved` | `replay --multi` | Per-symbol isolation, shared portfolio |

## Add a scenario

1. Add a minimal input under `data/scenarios/<name>.jsonl` with fixed IDs and timestamps.
2. Add `data/expected/<name>.out.jsonl`, or `<name>.err.txt` for a fatal replay error.
3. Add a test to `crates/engine/tests/deterministic_replay.rs` (matching goldens) or the appropriate integration test for runtime/strategy scenarios.
4. Assert final snapshot or typed-error state when output alone does not prove the invariant.
5. Add the scenario to the table above.
6. Run:

```bash
cargo test -p engine --test deterministic_replay
cargo test --workspace --all-targets --all-features
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Do not update a golden merely to accept unexplained behavior. Diagnose mismatches and fix a real engine/replay defect when one exists.

## Known limitations

- Batch replay validates and holds the complete input in memory; no streaming or checkpoint resume.
- Replay schemas are not versioned; no migration layer.
- No live exchange adapter or persistence layer.
- Strategy replay requires a registered built-in strategy name.
- Replay is a correctness and demonstration tool, not evidence of profitability or production trading readiness.

## See also

- [architecture.md](./architecture.md) — ReplayDriver vs Runtime
- [demo.md](./demo.md) — walkthrough commands
- [strategies.md](./strategies.md) — strategy-replay
