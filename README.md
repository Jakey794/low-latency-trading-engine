# Low-Latency Event-Driven Trading Engine

## What this is

A portfolio-quality Rust trading engine focused on deterministic behavior, market-microstructure correctness, explicit invariants, and reproducible tests. The project name describes the domain; it does not claim measured low latency.

## What this is not

This is not a profitable trading bot, investment advice, or a system connected to real capital or an exchange.

## Current status: Week 5 complete

- Single-symbol limit order book with ordered bid/ask ladders
- FIFO price levels and an active order-ID index
- Limit and market matching with full, partial, FIFO, and multi-level fills
- Resting-price execution, residual resting, expiration, and cancellation
- Typed acceptance, rejection, trade, cancellation, and expiration outputs
- Structural and uncrossed-book invariant checks
- Deterministic JSONL replay with strict sequence validation
- Replay summaries and copied final book snapshots
- CLI replay command and exact golden-file scenario tests

## Deterministic replay

Replay reads one event per JSONL line, validates that sequence numbers strictly increase, and passes each event to the existing `MatchingEngine`. It does not reimplement matching. A successful run returns ordered output events, aggregate counters, and a copied final book snapshot. Duplicate or decreasing sequences fail before any event in the batch mutates the engine.

Prices are signed integer ticks, quantities and timestamps are integers, and replay never reads the system clock or creates random IDs. Replay `kind` values use snake_case; reused domain enums retain Rust-style values such as `"Buy"` and `"Limit"`.

Example limit order:

```json
{"seq":1,"ts_ns":100,"kind":"new_order","order":{"order_id":1001,"symbol":"AAPL","side":"Sell","order_type":"Limit","price":10000,"qty":10,"timestamp_ns":90,"strategy_id":null}}
```

Example cancel:

```json
{"seq":2,"ts_ns":200,"kind":"cancel","order_id":1001,"symbol":"AAPL"}
```

See [docs/replay.md](docs/replay.md) for the complete input and output formats.

## Run replay

Run the basic crossing scenario:

```bash
cargo run --release --bin engine-cli -- replay data/scenarios/basic_cross.jsonl
```

Replay events are written as JSONL to stdout. The summary is written to stderr, keeping redirected stdout valid JSONL.

Example event output:

```json
{"seq":1,"ts_ns":100,"kind":"accepted","order_id":1001}
{"seq":2,"ts_ns":200,"kind":"accepted","order_id":1002}
{"seq":2,"ts_ns":200,"kind":"trade","trade":{"symbol":"AAPL","taker_order_id":1002,"maker_order_id":1001,"price":10000,"qty":10,"aggressor_side":"Buy","timestamp_ns":200}}
```

Print only the summary:

```bash
cargo run --release --bin engine-cli -- replay data/scenarios/basic_cross.jsonl --summary-only
```

```text
input_events: 2
output_events: 3
accepted: 2
rejected: 0
trades: 1
cancelled: 0
expired: 0
final_resting_orders: 0
final_bid_levels: 0
final_ask_levels: 0
```

Use `--output <path>` to write events to a file and `--book` to print the final book snapshot.

## Golden-file correctness suite

Integration tests replay committed inputs from `data/scenarios/` and compare the complete ordered output string with `data/expected/`. Fatal sequence errors use text goldens and also assert the typed error and unchanged book.

The suite covers:

- resting-price crossing;
- partial fills;
- FIFO priority at one price;
- best-price ordering across levels;
- cancellation before and after a partial fill;
- market-order rejection on an empty book;
- duplicate and out-of-order sequence rejection; and
- crossed-book prevention.

Run all checks from the repository root:

```bash
cargo test
cargo fmt --check
cargo clippy -- -D warnings
```

## Planned work

- Strategy interfaces
- Position and P&L accounting
- Risk limits and kill switch
- Reproducible benchmarks and profiling
