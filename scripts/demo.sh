#!/usr/bin/env bash
# Runnable portfolio demo: replay, strategies, multi-symbol, risk.
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

echo "--- 1. Basic replay (summary) ---"
run cargo run --release --bin engine-cli -- replay \
  data/scenarios/basic_cross.jsonl --summary-only

echo "--- 2. Deterministic replay byte compare ---"
TMP1="$(mktemp)"
TMP2="$(mktemp)"
trap 'rm -f "$TMP1" "$TMP2"' EXIT
cargo run --release --bin engine-cli -- replay \
  data/scenarios/basic_cross.jsonl > "$TMP1" 2>/dev/null
cargo run --release --bin engine-cli -- replay \
  data/scenarios/basic_cross.jsonl > "$TMP2" 2>/dev/null
cmp "$TMP1" "$TMP2"
echo "Deterministic replay: identical stdout bytes"
echo

echo "--- 3. Multi-symbol replay ---"
run cargo run --release --bin engine-cli -- replay \
  data/scenarios/multi_symbol_interleaved.jsonl --multi --summary-only

echo "--- 4. Market-making strategy ---"
run cargo run --release --bin engine-cli -- strategy-replay \
  data/scenarios/market_making_seed.jsonl \
  --strategy market_making \
  --summary-only

echo "--- 5. Momentum strategy ---"
run cargo run --release --bin engine-cli -- strategy-replay \
  data/scenarios/momentum_seed.jsonl \
  --strategy momentum \
  --summary-only

echo "--- 6. Risk rejection (max order qty = 1) ---"
run cargo run --release --bin engine-cli -- strategy-replay \
  data/scenarios/market_making_seed.jsonl \
  --strategy market_making \
  --max-order-qty 1 \
  --summary-only

echo "--- 7. Python baseline (10k events) ---"
run python3 python/baseline_lob.py --events 10000

echo "Demo complete. See docs/demo.md for details."
