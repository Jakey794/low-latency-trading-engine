#!/usr/bin/env bash
# Runnable portfolio demo: replay, book, risk, kill switch, strategies, multi-symbol.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== Trading engine demo =="
echo "Repository: $ROOT"
echo

run() {
  echo ">> $*"
  "$@"
  echo
}

echo "--- 1. Deterministic replay ---"
run cargo run --release --bin engine-cli -- replay \
  data/scenarios/basic_cross.jsonl --summary-only

echo "--- 2. Final book snapshot ---"
run cargo run --release --bin engine-cli -- replay \
  data/scenarios/basic_cross.jsonl --book

echo "--- 3. Deterministic byte compare ---"
TMP1="$(mktemp)"
TMP2="$(mktemp)"
trap 'rm -f "$TMP1" "$TMP2"' EXIT
cargo run --release --bin engine-cli -- replay \
  data/scenarios/basic_cross.jsonl >"$TMP1" 2>/dev/null
cargo run --release --bin engine-cli -- replay \
  data/scenarios/basic_cross.jsonl >"$TMP2" 2>/dev/null
cmp "$TMP1" "$TMP2"
echo "Deterministic replay: identical stdout bytes"
echo

echo "--- 4. Risk rejection (max order qty = 1) ---"
run cargo run --release --bin engine-cli -- strategy-replay \
  data/scenarios/market_making_seed.jsonl \
  --strategy market_making \
  --max-order-qty 1 \
  --summary-only

echo "--- 5. Kill-switch behavior (unit-tested; CLI shows risk_rejected under tight limits) ---"
echo "See cargo test -p engine --test risk_controls"
echo

echo "--- 6. Market-making strategy ---"
run cargo run --release --bin engine-cli -- strategy-replay \
  data/scenarios/market_making_seed.jsonl \
  --strategy market_making \
  --summary-only

echo "--- 7. Momentum strategy ---"
run cargo run --release --bin engine-cli -- strategy-replay \
  data/scenarios/momentum_seed.jsonl \
  --strategy momentum \
  --summary-only

echo "--- 8. Multi-symbol replay ---"
run cargo run --release --bin engine-cli -- replay \
  data/scenarios/multi_symbol_interleaved.jsonl --multi --summary-only

echo "--- Benchmark / report locations ---"
echo "  docs/benchmarks/latest.json"
echo "  docs/benchmark_report.md"
echo "  docs/artifacts/dashboard.html"
echo "  docs/artifacts/latency_histogram.png"
echo "  docs/artifacts/throughput_chart.png"
echo "  docs/artifacts/rust_vs_python.png"
echo "  docs/artifacts/profile_summary.md"
echo

echo "Demo complete. See docs/demo.md for details."
