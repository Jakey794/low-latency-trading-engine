# Portfolio Accounting

The `Portfolio` module tracks cash, positions, and P&L across symbols. It integrates with the runtime: fills from matching update portfolio state; risk checks use projected positions before orders are accepted.

## Design principles

- **Integer ticks** for prices and quantities in the matching path.
- **`i128` money** for cash, notional, and P&L with checked arithmetic.
- **Average-cost accounting** for open, add, close, cover, and cross-zero flows.
- **Deterministic snapshots** with positions sorted by symbol.

## Position model

Each symbol has a `Position`:

| Field | Meaning |
| --- | --- |
| `qty` | Signed quantity (positive = long, negative = short) |
| `avg_cost` | Average entry price in ticks when `qty != 0` |

Opening, adding, reducing, closing, and flipping sign update average cost according to standard average-cost rules. Cross-zero transitions reset average cost on the new side.

## P&L

| Metric | Definition |
| --- | --- |
| Realized P&L | Accumulated on closes/reductions at trade prices vs average cost |
| Unrealized P&L | Mark-to-market using last trade or explicit mark prices |
| Equity | Cash + unrealized P&L (and position value at marks) |
| Cash | Updated on buys (debit) and sells (credit) using tick notional |

## Snapshots

`PortfolioSnapshot` is the read-only external view:

- Sorted `positions` by symbol string
- `cash`, `realized_pnl`, `unrealized_pnl`, `equity`
- Per-position `mark` when available

Strategies receive snapshots via `StrategyContext`; they must not mutate portfolio directly.

## Integration with runtime

```text
Trade fill → Portfolio::apply_fill → updated cash/position/P&L
                ↓
Post-trade → RiskManager::check_post_trade (max_total_loss → kill switch)
                ↓
Strategy callbacks receive updated PortfolioSnapshot
```

## Error handling

`PortfolioError` covers overflow, invalid quantity/price, and missing mark for unrealized calculations. Runtime propagates these as `RuntimeError::Portfolio`.

## Testing

- Unit tests: long/short open/add/close/cover, cross-zero, overflow, multi-symbol
- Integration: `portfolio_accounting` wires fills from `MatchingEngine`
- CLI: `strategy-replay --portfolio` prints final snapshot JSON to stderr

## Limitations

- No multi-currency or FX conversion
- Marks are simplified (last trade or explicit); no external market data feed
- No corporate actions, fees, or borrow costs
- Not a tax or regulatory accounting system

## See also

- [architecture.md](./architecture.md) — runtime integration
- [risk.md](./risk.md) — pre-trade position and notional checks
- [strategies.md](./strategies.md) — read-only portfolio context for strategies
