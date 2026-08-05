# Strategies

The engine provides a plugin-style `Strategy` trait for deterministic, bounded order intents. Built-in demonstration strategies illustrate the interface; they are **not** profitability claims.

## Strategy trait

```rust
pub trait Strategy: Send {
    fn id(&self) -> StrategyId;
    fn name(&self) -> &str;
    fn on_event(&mut self, event: &StrategyEvent, ctx: &StrategyContext) -> Vec<StrategyCommand>;
}
```

Strategies:

- Receive **read-only** `StrategyContext` (portfolio snapshot, optional book, risk kill flag).
- Return `StrategyCommand` intents (place limit/market, cancel).
- Must **not** mutate book, portfolio, or risk state directly.
- Must be **deterministic** (no wall clock, no RNG in callbacks).

## Strategy events

| Event | When delivered |
| --- | --- |
| `Activated` / `Deactivated` | Strategy registered or removed |
| `BookUpdate` | After book changes for a symbol |
| `Trade` | Public trade notification |
| `Fill` | Strategy or external order fill |
| `OrderAccepted` | Runtime assigned ID for accepted strategy order |
| `Timer` | Explicit replay timer tick (not wall clock) |
| `RiskRejection` | Strategy order rejected by risk |

## Strategy commands

| Command | Runtime behavior |
| --- | --- |
| `PlaceOrder` | Assigns deterministic order ID → risk → matching |
| `Cancel` | Always allowed (even under kill switch) |

Command budget: `DEFAULT_MAX_STRATEGY_COMMANDS_PER_EVENT` (16) per external event; excess commands are dropped with `strategy_commands_dropped` output.

## Built-in strategies

### Market making (`market_making`)

- Places symmetric limit quotes around mid price.
- Configurable half-spread, quote size, inventory skew, max inventory.
- Cancels and replaces quotes when mid or inventory shifts.
- Seed scenario: `data/scenarios/market_making_seed.jsonl`

### Momentum (`momentum`)

- Maintains rolling window of trade prices per symbol.
- Emits directional limit orders when window delta exceeds threshold.
- Cooldown measured in external sequence numbers.
- Seed scenario: `data/scenarios/momentum_seed.jsonl`

Aliases: `mm` → `market_making`.

## CLI usage

```bash
# Market making
cargo run --release --bin engine-cli -- strategy-replay \
  data/scenarios/market_making_seed.jsonl \
  --strategy market_making \
  --strategy-id 1 \
  --starting-cash 1000000 \
  --summary-only

# Momentum with portfolio snapshot
cargo run --release --bin engine-cli -- strategy-replay \
  data/scenarios/momentum_seed.jsonl \
  --strategy momentum \
  --portfolio
```

## Factory

`create_builtin(name, strategy_id)` returns `Box<dyn Strategy>` for CLI and tests.

## Testing

- `runtime_strategy.rs` — null strategy and runtime wiring
- `strategy_demos.rs` — deterministic output for seed scenarios (double-run equality)

## Adding a custom strategy

1. Implement `Strategy` in `crates/engine/src/strategy/`.
2. Register in `create_builtin` or construct in tests/runtime setup.
3. Add a seed JSONL scenario under `data/scenarios/`.
4. Add integration test asserting deterministic serialized outputs.

## Limitations

- No external market data feeds or order-book depth beyond snapshots
- No smart order routing or cross-venue logic
- Demonstration configs are fixed defaults, not optimized parameters
- Strategies do not simulate latency, slippage models, or fees

## See also

- [architecture.md](./architecture.md) — runtime dispatch
- [risk.md](./risk.md) — all intents pass through risk
- [portfolio.md](./portfolio.md) — context snapshot fields
