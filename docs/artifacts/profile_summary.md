# Profile summary (macOS `sample` fallback)

## Why not a flamegraph SVG?

`cargo flamegraph` is installed (`flamegraph 0.6.13`), but generation failed without sudo:

```text
xcode-select: error: tool 'xctrace' requires Xcode, but active developer directory
'/Library/Developer/CommandLineTools' is a command line tools instance
failed to sample program
```

Full Xcode / `xctrace` is not available in this environment. No flamegraph was fabricated.

## Profiler used

| Field | Value |
| --- | --- |
| Tool | `/usr/bin/sample` (macOS) |
| Artifact | [`sample_profile.txt`](./sample_profile.txt) |
| Command | see below |
| Binary | `target/release/measure` |
| Sampling | 5 seconds, 1 ms interval |
| Date | 2026-08-05 |
| Host | Apple M4 Pro, macOS 26.5.2, Darwin 25.5.0 arm64 |

### Exact command

```bash
cargo build --release -p engine-cli --bin measure
./target/release/measure --micro-iters 100000 &
MPID=$!
sample $MPID 5 -file docs/artifacts/sample_profile.txt
wait $MPID
```

## Top hot paths observed

From the call graph in `sample_profile.txt` (main thread), samples concentrated in:

1. `Runtime::process_events` / `Runtime::process_one` / `Runtime::submit_order`
2. `MatchingEngine::process_event_internal` / `submit_limit_order_inner`
3. `OrderBook::add_limit_order` (including `VecDeque` growth / allocator)
4. `OrderBook::validate_limit_order`
5. `MatchingEngine::execute_at_best` / `OrderBook::remove_best_opposite`
6. `OrderBook::snapshot` / `PriceLevel::total_qty` (book snapshots during strategy/runtime paths)
7. Allocator (`realloc` / `malloc`) under resting-order growth

This is a **local, machine-specific** observation for portfolio documentation. It is **not** evidence of exchange-grade or HFT latency.

## Manual flamegraph retry (requires full Xcode)

```bash
# Point to full Xcode if installed:
# sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
CARGO_PROFILE_RELEASE_DEBUG=true BENCH_FULL=1 \
  cargo flamegraph --bin measure -p engine-cli \
  -o docs/artifacts/flamegraph.svg -- --micro-iters 5000
```
