#!/usr/bin/env python3
"""Generate benchmark charts from measured docs/benchmarks/latest.json.

Rejects missing/placeholder data. Never fabricates latency or throughput values.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import matplotlib.pyplot as plt

REPO_ROOT = Path(__file__).resolve().parents[1]
LATEST = REPO_ROOT / "docs" / "benchmarks" / "latest.json"
SUMMARY = REPO_ROOT / "out" / "bench_summary.json"
ARTIFACTS = REPO_ROOT / "docs" / "artifacts"

LATENCY_CHART = ARTIFACTS / "latency_histogram.png"
THROUGHPUT_CHART = ARTIFACTS / "throughput_chart.png"
# Keep alias for older docs/scripts
THROUGHPUT_ALIAS = ARTIFACTS / "throughput_by_workload.png"
RUST_VS_PYTHON = ARTIFACTS / "rust_vs_python.png"


def load_payload() -> dict:
    for path in (LATEST, SUMMARY):
        if path.is_file():
            with path.open(encoding="utf-8") as handle:
                payload = json.load(handle)
            if not isinstance(payload, dict):
                continue
            if "workloads" not in payload:
                continue
            return payload
    raise SystemExit(
        "ERROR: measured benchmark JSON not found. "
        "Run: cargo run --release -p engine-cli --bin measure"
    )


def env_caption(payload: dict) -> str:
    env = payload.get("environment") or {}
    parts = []
    for key in ("cpu", "macos_version", "operating_system", "rust_version", "date_utc"):
        val = env.get(key)
        if val:
            parts.append(str(val))
    ram = env.get("ram_gib")
    if isinstance(ram, (int, float)):
        parts.append(f"{ram:.1f} GiB RAM")
    return " | ".join(parts) if parts else "environment undisclosed"


def require_latency(workloads: list[dict]) -> tuple[str, dict]:
    for wl in workloads:
        lat = wl.get("latency")
        if not isinstance(lat, dict):
            continue
        if not all(k in lat for k in ("p50_ns", "p90_ns", "p95_ns", "p99_ns", "max_ns")):
            continue
        if lat.get("samples", 0) <= 0:
            continue
        name = wl.get("name") or wl.get("workload") or "workload"
        return str(name), lat
    # Prefer 10k core engine
    for wl in workloads:
        if wl.get("name") == "core_engine_10k" and isinstance(wl.get("latency"), dict):
            return "core_engine_10k", wl["latency"]
    raise SystemExit("ERROR: no workload with measured latency percentiles")


def plot_latency(name: str, lat: dict, caption: str) -> None:
    labels = ["p50", "p90", "p95", "p99"]
    values_ns = [float(lat["p50_ns"]), float(lat["p90_ns"]), float(lat["p95_ns"]), float(lat["p99_ns"])]
    if lat.get("p999_ns") is not None:
        labels.append("p99.9")
        values_ns.append(float(lat["p999_ns"]))
    labels.append("max")
    values_ns.append(float(lat["max_ns"]))
    values_us = [v / 1000.0 for v in values_ns]

    fig, ax = plt.subplots(figsize=(9, 5))
    ax.bar(labels, values_us, color="#2a6f97")
    ax.set_ylabel("Latency (µs)")
    ax.set_xlabel("Percentile")
    ax.set_title(f"Latency histogram — {name}\n(genuine measured data)")
    ax.annotate(
        caption,
        xy=(0.5, -0.18),
        xycoords="axes fraction",
        ha="center",
        fontsize=8,
        wrap=True,
    )
    fig.tight_layout()
    fig.savefig(LATENCY_CHART, dpi=140, bbox_inches="tight")
    plt.close(fig)


def plot_throughput(workloads: list[dict], caption: str) -> None:
    names: list[str] = []
    eps: list[float] = []
    for wl in workloads:
        thr = wl.get("throughput")
        if not isinstance(thr, dict):
            continue
        value = thr.get("events_per_sec")
        if not isinstance(value, (int, float)) or value <= 0:
            continue
        names.append(str(wl.get("name", "workload")))
        eps.append(float(value))
    if not names:
        raise SystemExit("ERROR: no positive throughput measurements")

    fig, ax = plt.subplots(figsize=(11, 5))
    ax.barh(names, eps, color="#1b4332")
    ax.set_xlabel("Events per second")
    ax.set_title("Throughput by workload (genuine measured data)")
    ax.annotate(caption, xy=(0.5, -0.12), xycoords="axes fraction", ha="center", fontsize=8)
    fig.tight_layout()
    fig.savefig(THROUGHPUT_CHART, dpi=140, bbox_inches="tight")
    fig.savefig(THROUGHPUT_ALIAS, dpi=140, bbox_inches="tight")
    plt.close(fig)


def plot_rust_vs_python(payload: dict, caption: str) -> None:
    comps = payload.get("comparisons") or {}
    rust = comps.get("rust_engine") or {}
    py = comps.get("python_baseline") or {}
    rust_eps = rust.get("events_per_sec")
    py_eps = py.get("events_per_second") or py.get("events_per_sec")
    if not isinstance(rust_eps, (int, float)) or rust_eps <= 0:
        raise SystemExit("ERROR: missing Rust comparison events_per_sec")
    if not isinstance(py_eps, (int, float)) or py_eps <= 0:
        raise SystemExit("ERROR: missing Python baseline events_per_second — run measure with venv")

    fig, ax = plt.subplots(figsize=(7, 5))
    labels = ["Rust engine\n(core_engine_10k)", "Python naive baseline\n(10k events)"]
    values = [float(rust_eps), float(py_eps)]
    ax.bar(labels, values, color=["#1d3557", "#e63946"])
    ax.set_ylabel("Events per second")
    ax.set_title("Rust vs naive Python baseline (genuine measured data)")
    ax.annotate(
        caption + "\nNaive Python is a correctness-oriented reference, not an optimized competitor.",
        xy=(0.5, -0.22),
        xycoords="axes fraction",
        ha="center",
        fontsize=8,
    )
    fig.tight_layout()
    fig.savefig(RUST_VS_PYTHON, dpi=140, bbox_inches="tight")
    plt.close(fig)


def validate_png(path: Path) -> None:
    if not path.is_file() or path.stat().st_size < 1000:
        raise SystemExit(f"ERROR: invalid/too-small PNG: {path}")
    header = path.read_bytes()[:8]
    if header != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"ERROR: malformed PNG header: {path}")


def main() -> int:
    ARTIFACTS.mkdir(parents=True, exist_ok=True)
    # Remove known placeholders
    note = ARTIFACTS / "latency_histogram.NO_DATA.txt"
    if note.exists():
        note.unlink()

    payload = load_payload()
    caption = env_caption(payload)
    workloads = [w for w in payload.get("workloads", []) if isinstance(w, dict)]
    if not workloads:
        raise SystemExit("ERROR: workloads empty")

    name, lat = require_latency(workloads)
    # Prefer core_engine_10k for latency chart if present
    for wl in workloads:
        if wl.get("name") == "core_engine_10k" and isinstance(wl.get("latency"), dict):
            name, lat = "core_engine_10k", wl["latency"]
            break

    plot_latency(name, lat, caption)
    plot_throughput(workloads, caption)
    plot_rust_vs_python(payload, caption)

    for path in (LATENCY_CHART, THROUGHPUT_CHART, RUST_VS_PYTHON):
        validate_png(path)

    print(f"Wrote {LATENCY_CHART}")
    print(f"Wrote {THROUGHPUT_CHART}")
    print(f"Wrote {RUST_VS_PYTHON}")
    print("Charts generated from measured data.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
