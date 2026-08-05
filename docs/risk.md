# Risk Controls and Kill Switch

Pre-trade risk runs before any order reaches the matching engine. Rejections are atomic: the book, portfolio, and strategy state remain unchanged.

## RiskManager

`RiskManager` holds configurable `RiskLimits`, kill-switch state, and starting equity for loss monitoring.

### Pre-trade checks

For each new order, risk evaluates (when limits are set):

| Check | Rejection reason |
| --- | --- |
| Kill switch active | `KillSwitchActive` |
| Order quantity | `MaxOrderQty`, `PerSymbolMaxOrderQty` |
| Projected absolute position | `MaxAbsPosition`, `PerSymbolMaxAbsPosition` |
| Gross notional (`|price| * qty`) | `MaxGrossNotional` |
| Missing/invalid price for notional | `InvalidPriceForNotional` |
| Arithmetic overflow | `Overflow` |

`None` on a limit field disables that check.

### Post-trade monitoring

After fills update the portfolio, `max_total_loss` compares current equity to starting equity. Breach activates the kill switch automatically.

### Kill switch semantics

- **New orders blocked** while killed (manual or automatic).
- **Cancels always allowed** — resting liquidity can be withdrawn after a trip.
- Structured `RiskRejectReason` on every pre-trade rejection.
- Runtime emits `risk_rejected` output events (distinct from matching `rejected`).

## Runtime integration

```text
NewOrder → RiskManager::check_new_order
              ├─ Reject → RuntimeOutput::RiskRejected (no matching)
              └─ Allow  → MatchingEngine → Portfolio → post-trade loss check
```

Strategy-generated orders follow the same path; strategies cannot bypass risk.

## CLI demonstration

Reject large orders via `--max-order-qty`:

```bash
cargo run --release --bin engine-cli -- strategy-replay \
  data/scenarios/market_making_seed.jsonl \
  --strategy market_making \
  --max-order-qty 1 \
  --summary-only
```

Compare `risk_rejected` vs a run without the limit.

## Configuration example

```rust
use engine::risk::RiskLimits;

let limits = RiskLimits {
    max_order_qty: Some(100),
    max_abs_position: Some(500),
    max_gross_notional: Some(1_000_000),
    max_total_loss: Some(50_000),
    ..RiskLimits::default()
};
```

Per-symbol overrides use `BTreeMap<String, _>` fields for deterministic iteration in tests.

## Testing

`crates/engine/tests/risk_controls.rs` covers:

- Rejected orders do not mutate book or portfolio
- Cancels allowed after kill switch activation
- Boundary values at exact limits
- Post-trade loss trip and atomicity

## Limitations

- No dynamic limit updates from external config service
- No order-rate throttling or credit checks beyond configured limits
- Kill switch is process-local (no distributed coordination)
- Not a regulatory compliance or exchange risk framework

## See also

- [architecture.md](./architecture.md)
- [demo.md](./demo.md) — step-by-step risk demo
- [strategies.md](./strategies.md) — strategies receive `risk_killed` in context
