#!/usr/bin/env bash
# Final verification gate for the elite engine portfolio.
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

fail() {
  echo "ERROR: $*" >&2
  exit 1
}

validate_png() {
  local path="$1"
  [[ -f "$path" ]] || fail "missing PNG $path"
  local size
  size=$(wc -c <"$path" | tr -d ' ')
  [[ "$size" -ge 1000 ]] || fail "PNG too small ($size bytes): $path"
  # PNG magic
  local magic
  magic=$(dd if="$path" bs=8 count=1 2>/dev/null | xxd -p)
  [[ "$magic" == "89504e470d0a1a0a" ]] || fail "bad PNG magic: $path"
}

echo "--- Rust formatting ---"
step cargo fmt --check

echo "--- Clippy (all features, all targets, deny warnings) ---"
step cargo clippy --workspace --all-targets --all-features -- -D warnings

echo "--- Tests (debug, all features, all targets) ---"
step cargo test --workspace --all-targets --all-features

echo "--- Tests (release) ---"
step cargo test --workspace --release

echo "--- Elite feature tests ---"
step cargo test --workspace --features order_pool,lockfree_queue

echo "--- Benchmark compile (no run) ---"
step cargo bench --workspace --no-run
step cargo bench --workspace --no-run --features order_pool
step cargo bench --workspace --no-run --features lockfree_queue

echo "--- Deterministic replay: basic_cross twice + byte compare ---"
TMP1="$(mktemp)"
TMP2="$(mktemp)"
trap 'rm -f "$TMP1" "$TMP2"' EXIT
cargo run --release --bin engine-cli -- replay \
  data/scenarios/basic_cross.jsonl >"$TMP1" 2>/dev/null
cargo run --release --bin engine-cli -- replay \
  data/scenarios/basic_cross.jsonl >"$TMP2" 2>/dev/null
cmp "$TMP1" "$TMP2"
echo "Deterministic replay OK"
echo

echo "--- Risk rejection smoke ---"
cargo run --release --bin engine-cli -- simulate \
  data/scenarios/market_making_seed.jsonl \
  --strategy market-maker \
  --max-order-qty 1 \
  --summary-only >/tmp/risk_smoke.txt 2>&1
grep -E 'risk_rejected|accepted|output_events' /tmp/risk_smoke.txt
echo

echo "--- Strategy demos ---"
step cargo run --release --bin engine-cli -- simulate \
  data/scenarios/market_making_seed.jsonl --strategy market-maker --summary-only
step cargo run --release --bin engine-cli -- simulate \
  data/scenarios/momentum_seed.jsonl --strategy momentum --summary-only

echo "--- Multi-symbol replay smoke ---"
step cargo run --release --bin engine-cli -- replay \
  data/scenarios/multi_symbol_interleaved.jsonl --multi --summary-only

echo "--- Paper WebSocket demo (offline) ---"
step cargo run --release --bin engine-cli -- websocket-demo

echo "--- Benchmark report CLI ---"
step cargo run --release --bin engine-cli -- benchmark-report

echo "--- Python baseline smoke ---"
if [[ -x .venv/bin/python ]]; then
  step .venv/bin/python python/baseline_lob.py --events 1000
else
  step python3 python/baseline_lob.py --events 1000
fi

echo "--- Benchmark JSON validity ---"
[[ -f docs/benchmarks/latest.json ]] || fail "missing docs/benchmarks/latest.json — run measure"
python3 - <<'PY'
import json, sys
from pathlib import Path
p = Path("docs/benchmarks/latest.json")
data = json.loads(p.read_text())
assert data.get("schema_version") == 1
assert "environment" in data and "workloads" in data
assert len(data["workloads"]) >= 5
for w in data["workloads"]:
    assert "name" in w and "throughput" in w
# reject placeholder markers
text = p.read_text().lower()
for bad in ("placeholder", "todo", "fabricat", "dummy latency"):
    assert bad not in text, bad
print("benchmark JSON OK:", len(data["workloads"]), "workloads")
PY
echo

echo "--- Chart generation ---"
if [[ -x .venv/bin/python ]]; then
  step .venv/bin/python scripts/generate_charts.py
else
  python3 -c "import matplotlib" 2>/dev/null || fail "matplotlib required for charts"
  step python3 scripts/generate_charts.py
fi

echo "--- Artifact validity (no placeholders) ---"
[[ ! -f docs/artifacts/latency_histogram.NO_DATA.txt ]] || fail "placeholder NO_DATA file present"
validate_png docs/artifacts/latency_histogram.png
validate_png docs/artifacts/throughput_chart.png
validate_png docs/artifacts/rust_vs_python.png
[[ -f docs/artifacts/architecture.svg ]] || fail "missing architecture.svg"
[[ -s docs/artifacts/architecture.svg ]] || fail "empty architecture.svg"
[[ -f docs/artifacts/profile_summary.md ]] || fail "missing profile_summary.md"
if [[ -f docs/artifacts/flamegraph.svg ]]; then
  [[ -s docs/artifacts/flamegraph.svg ]] || fail "empty flamegraph.svg"
elif [[ -f docs/artifacts/sample_profile.txt ]]; then
  [[ -s docs/artifacts/sample_profile.txt ]] || fail "empty sample_profile.txt"
  grep -q "Call graph\|Thread_" docs/artifacts/sample_profile.txt || fail "sample profile lacks call graph"
else
  fail "missing flamegraph.svg and sample_profile.txt"
fi
# reject placeholder dashboard wording as sole evidence — dashboard may mention measured links
if grep -qi 'placeholder' docs/artifacts/dashboard.html; then
  echo "WARN: dashboard.html still mentions placeholder styling; ensure measured images are linked"
fi
echo "Artifacts OK"
echo

echo "--- Documentation presence ---"
for f in README.md docs/architecture.md docs/architecture.mmd docs/portfolio.md docs/risk.md \
  docs/strategies.md docs/replay.md docs/performance.md docs/profiling.md docs/benchmark_report.md \
  docs/benchmark-report.md docs/demo.md docs/design_notes.md docs/design-decisions.md \
  docs/RELEASE_NOTES.md docs/AGENT_PROGRESS.md data/config/risk_demo.json \
  data/scenarios/paper_ws_demo.jsonl; do
  [[ -f "$f" ]] || fail "missing $f"
done
echo "Docs OK"
echo

echo "--- git diff --check ---"
step git diff --check

echo "== verify_final.sh: ALL CHECKS PASSED =="
