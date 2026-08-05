#!/usr/bin/env bash
# Attempt a non-sudo flamegraph on macOS. Documents blockers if unavailable.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "Profiling helper for engine_hot_path (requires cargo-flamegraph + permissions)"
echo "Manual install: cargo install flamegraph"
echo "macOS may require: sudo dtrace privileges or Instruments; do not use sudo here."

if ! command -v cargo-flamegraph >/dev/null 2>&1 && ! cargo flamegraph -h >/dev/null 2>&1; then
  echo "BLOCKER: cargo-flamegraph not installed"
  echo "Install: cargo install flamegraph"
  echo "Then: BENCH_FULL=1 cargo flamegraph --bench engine_hot_path -o docs/artifacts/flamegraph.svg"
  exit 0
fi

echo "Attempting flamegraph (may fail without profiler entitlements)..."
if BENCH_FULL=1 cargo flamegraph --bench engine_hot_path -o docs/artifacts/flamegraph.svg; then
  echo "Wrote docs/artifacts/flamegraph.svg"
else
  echo "BLOCKER: flamegraph generation failed (permissions or missing tools)"
  echo "Exact command to retry after fixing environment:"
  echo "  BENCH_FULL=1 cargo flamegraph --bench engine_hot_path -o docs/artifacts/flamegraph.svg"
fi
