# AGENTS.md

## Mission

Finish this repository as a portfolio-quality Rust event-driven trading engine. Weeks 1–5 are complete. Preserve their behavior and extend the project through the final elite version.

## Required final scope

- Limit order book and matching engine
- Limit, market, and cancel orders
- Partial fills and price-time priority
- Deterministic replay and golden tests
- Position, cash, realized P&L, and unrealized P&L
- Pre-trade risk limits and kill switch
- Strategy plugin interface
- Market-making and momentum demonstration strategies
- Multi-symbol replay
- Criterion benchmark suite
- Latency histogram and throughput reporting
- Naive Python baseline
- Property-based tests
- Profiling and flamegraph workflow
- Architecture documentation
- Benchmark report and charts
- CI and reproducible demo commands
- Carefully isolated elite experiments such as an arena/order pool and lock-free queue

## Engineering rules

- Correctness and determinism come before performance.
- Never use floating-point prices. Preserve integer ticks.
- Use checked arithmetic or wider integer types for notional and P&L.
- Do not use system time or randomness in deterministic execution paths.
- Do not expose HashMap iteration order through public output.
- Avoid unsafe Rust in the production engine.
- Keep experimental optimization code isolated behind a feature or module.
- Do not claim profitability, exchange-grade latency, or production readiness.
- Do not connect to real capital or submit live orders.
- Every behavior change requires tests.
- Do not delete or weaken existing tests.
- Do not push, merge, force-reset, clean untracked files, use sudo, or modify secrets.

## Required verification

Before claiming completion, run:

cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
cargo test --workspace --release
cargo bench --workspace --no-run

Do not claim success if any required command fails.
