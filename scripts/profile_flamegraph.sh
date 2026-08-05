#!/usr/bin/env bash
# Attempt flamegraph; fall back to macOS `sample` profiler without sudo.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT_SVG="$ROOT/docs/artifacts/flamegraph.svg"
OUT_SAMPLE="$ROOT/docs/artifacts/sample_profile.txt"
SUMMARY="$ROOT/docs/artifacts/profile_summary.md"

mkdir -p docs/artifacts

echo "Building release measure binary..."
cargo build --release -p engine-cli --bin measure

if command -v cargo-flamegraph >/dev/null 2>&1 || cargo flamegraph -V >/dev/null 2>&1; then
  echo "Attempting cargo flamegraph (no sudo)..."
  set +e
  CARGO_PROFILE_RELEASE_DEBUG=true cargo flamegraph --bin measure -p engine-cli \
    -o "$OUT_SVG" -- --micro-iters 5000
  status=$?
  set -e
  if [[ $status -eq 0 && -f "$OUT_SVG" && -s "$OUT_SVG" ]]; then
    if grep -q '<svg\|flamegraph\|title=' "$OUT_SVG"; then
      echo "Wrote genuine flamegraph: $OUT_SVG"
      exit 0
    fi
  fi
  echo "Flamegraph unavailable or invalid; falling back to /usr/bin/sample"
  rm -f "$OUT_SVG"
else
  echo "cargo-flamegraph not installed; falling back to /usr/bin/sample"
fi

echo "Sampling measure binary for 5s..."
./target/release/measure --micro-iters 100000 >/tmp/measure_profile_out.txt 2>&1 &
MPID=$!
sleep 0.15
sample "$MPID" 5 -file "$OUT_SAMPLE"
wait "$MPID" 2>/dev/null || true

if [[ ! -s "$OUT_SAMPLE" ]]; then
  echo "ERROR: sample profile empty"
  exit 1
fi

echo "Wrote $OUT_SAMPLE"
echo "See $SUMMARY for interpretation and flamegraph retry commands."
