#!/usr/bin/env bash
# Final verification gate for the elite engine portfolio.
# Usage: chmod +x scripts/verify_final.sh && ./scripts/verify_final.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== verify_final.sh =="
echo "Repository: $ROOT"
echo

step() {
  echo ">> $*"
  "$@"
  echo
}

echo "--- Rust formatting ---"
step cargo fmt --check

echo "--- Clippy (all features, all targets, deny warnings) ---"
step cargo clippy --workspace --all-targets --all-features -- -D warnings

echo "--- Tests (debug, all features, all targets) ---"
step cargo test --workspace --all-targets --all-features

echo "--- Tests (release) ---"
step cargo test --workspace --release

echo "--- Benchmark compile (no run) ---"
step cargo bench --workspace --no-run

echo "--- Deterministic replay: basic_cross twice + byte compare ---"
TMP1="$(mktemp)"
TMP2="$(mktemp)"
trap 'rm -f "$TMP1" "$TMP2"' EXIT
cargo run --release --bin engine-cli -- replay \
  data/scenarios/basic_cross.jsonl > "$TMP1" 2>/dev/null
cargo run --release --bin engine-cli -- replay \
  data/scenarios/basic_cross.jsonl > "$TMP2" 2>/dev/null
cmp "$TMP1" "$TMP2"
echo "Deterministic replay OK"
echo

echo "--- CLI strategy-replay smoke ---"
step cargo run --release --bin engine-cli -- strategy-replay \
  data/scenarios/market_making_seed.jsonl \
  --strategy market_making \
  --summary-only

echo "--- Multi-symbol replay smoke ---"
step cargo run --release --bin engine-cli -- replay \
  data/scenarios/multi_symbol_interleaved.jsonl \
  --multi \
  --summary-only

echo "--- Python baseline smoke ---"
step python3 python/baseline_lob.py --events 1000

echo "--- Chart script smoke (if matplotlib available) ---"
if python3 -c "import matplotlib" 2>/dev/null; then
  step python3 scripts/generate_charts.py
else
  echo ">> SKIP: matplotlib not installed (pip install -r python/requirements.txt)"
  echo
fi

echo "--- git diff --check (no conflict markers / whitespace errors) ---"
step git diff --check

echo "== verify_final.sh: ALL CHECKS PASSED =="
