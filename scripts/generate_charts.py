#!/usr/bin/env python3
"""Generate benchmark charts from out/bench_summary.json when available.

Writes PNG artifacts under docs/artifacts/. When bench data is missing,
creates clearly labeled placeholder charts (no fabricated measured results).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import matplotlib.pyplot as plt

REPO_ROOT = Path(__file__).resolve().parents[1]
BENCH_SUMMARY_PATH = REPO_ROOT / "out" / "bench_summary.json"
ARTIFACTS_DIR = REPO_ROOT / "docs" / "artifacts"

LATENCY_CHART = ARTIFACTS_DIR / "latency_histogram.png"
THROUGHPUT_CHART = ARTIFACTS_DIR / "throughput_by_workload.png"
RUST_VS_PYTHON_CHART = ARTIFACTS_DIR / "rust_vs_python.png"
MISSING_DATA_NOTE = ARTIFACTS_DIR / "latency_histogram.NO_DATA.txt"


def load_bench_summary() -> dict | None:
    if not BENCH_SUMMARY_PATH.is_file():
        return None
    try:
        with BENCH_SUMMARY_PATH.open(encoding="utf-8") as handle:
            payload = json.load(handle)
    except (OSError, json.JSONDecodeError):
        return None
    return payload if isinstance(payload, dict) else None


def workloads_from_summary(summary: dict | None) -> list[dict]:
    if summary is None:
        return []

    workloads = summary.get("workloads")
    if isinstance(workloads, list):
        return [item for item in workloads if isinstance(item, dict)]

    if "throughput" in summary or "latency" in summary:
        return [summary]

    return []


def latency_percentiles_ns(workload: dict) -> dict[str, float] | None:
    latency = workload.get("latency")
    if not isinstance(latency, dict):
        return None

    mapping = {
        "p50": latency.get("p50_ns"),
        "p90": latency.get("p90_ns"),
        "p95": latency.get("p95_ns"),
        "p99": latency.get("p99_ns"),
        "max": latency.get("max_ns"),
    }
    cleaned: dict[str, float] = {}
    for label, value in mapping.items():
        if isinstance(value, (int, float)) and value >= 0:
            cleaned[label] = float(value)
    return cleaned or None


def throughput_events_per_sec(workload: dict) -> float | None:
    throughput = workload.get("throughput")
    if not isinstance(throughput, dict):
        return None
    value = throughput.get("events_per_sec")
    if isinstance(value, (int, float)) and value >= 0:
        return float(value)
    return None


def comparison_entries(summary: dict | None) -> tuple[str | None, float | None, str | None, float | None]:
    if summary is None:
        return None, None, None, None

    comparisons = summary.get("comparisons")
    if isinstance(comparisons, dict):
        rust = comparisons.get("rust_engine") or comparisons.get("rust")
        python = comparisons.get("python_baseline") or comparisons.get("python")
    else:
        rust = summary.get("rust_engine") or summary.get("rust")
        python = summary.get("python_baseline") or summary.get("python")

    def eps(entry: object) -> float | None:
        if not isinstance(entry, dict):
            return None
        for key in ("events_per_second", "events_per_sec"):
            value = entry.get(key)
            if isinstance(value, (int, float)) and value >= 0:
                return float(value)
        throughput = entry.get("throughput")
        if isinstance(throughput, dict):
            value = throughput.get("events_per_sec")
            if isinstance(value, (int, float)) and value >= 0:
                return float(value)
        return None

    def label(entry: object, fallback: str) -> str | None:
        if not isinstance(entry, dict):
            return None
        text = entry.get("label") or entry.get("implementation") or fallback
        return str(text) if text else fallback

    return (
        label(rust, "Rust engine"),
        eps(rust),
        label(python, "naive baseline"),
        eps(python),
    )


def write_latency_histogram(workloads: list[dict]) -> None:
    latency_rows: list[tuple[str, dict[str, float]]] = []
    for workload in workloads:
        percentiles = latency_percentiles_ns(workload)
        if percentiles is None:
            continue
        name = str(workload.get("workload") or workload.get("name") or "workload")
        latency_rows.append((name, percentiles))

    if not latency_rows:
        if MISSING_DATA_NOTE.exists():
            MISSING_DATA_NOTE.unlink()
        note = (
            "latency_histogram.png was not generated: no latency samples in "
            f"{BENCH_SUMMARY_PATH.relative_to(REPO_ROOT)}.\n"
            "Run Criterion benchmarks and write bench_summary.json, then rerun "
            "scripts/generate_charts.py.\n"
        )
        MISSING_DATA_NOTE.write_text(note, encoding="utf-8")
        print(note.strip())
        return

    if MISSING_DATA_NOTE.exists():
        MISSING_DATA_NOTE.unlink()

    labels = ["p50", "p90", "p95", "p99", "max"]
    x = range(len(labels))

    fig, ax = plt.subplots(figsize=(9, 5))
    width = 0.8 / max(len(latency_rows), 1)
    for idx, (name, percentiles) in enumerate(latency_rows):
        values_us = [percentiles.get(label, 0.0) / 1000.0 for label in labels]
        offsets = [pos + (idx - (len(latency_rows) - 1) / 2) * width for pos in x]
        ax.bar(offsets, values_us, width=width, label=name)

    ax.set_xticks(list(x))
    ax.set_xticklabels(labels)
    ax.set_xlabel("Latency percentile")
    ax.set_ylabel("Latency (microseconds)")
    ax.set_title("Rust engine latency percentiles (measured)")
    ax.legend()
    ax.grid(axis="y", alpha=0.3)
    fig.tight_layout()
    fig.savefig(LATENCY_CHART, dpi=150)
    plt.close(fig)
    print(f"Wrote {LATENCY_CHART.relative_to(REPO_ROOT)}")


def write_throughput_chart(workloads: list[dict]) -> None:
    names: list[str] = []
    values: list[float] = []
    for workload in workloads:
        eps = throughput_events_per_sec(workload)
        if eps is None:
            continue
        names.append(str(workload.get("workload") or workload.get("name") or "workload"))
        values.append(eps)

    fig, ax = plt.subplots(figsize=(9, 5))
    if names:
        ax.bar(names, values, color="#4c72b0")
        ax.set_ylabel("Events per second")
        ax.set_title("Throughput by workload (measured)")
        ax.tick_params(axis="x", rotation=20)
    else:
        ax.text(
            0.5,
            0.5,
            "No measured throughput data yet\n(run benches → out/bench_summary.json)",
            ha="center",
            va="center",
            transform=ax.transAxes,
            fontsize=12,
        )
        ax.set_xticks([])
        ax.set_yticks([])
        ax.set_ylabel("Events per second (no measured data yet)")
        ax.set_title("Throughput by workload (placeholder layout)")

    ax.grid(axis="y", alpha=0.3)
    fig.tight_layout()
    fig.savefig(THROUGHPUT_CHART, dpi=150)
    plt.close(fig)
    print(f"Wrote {THROUGHPUT_CHART.relative_to(REPO_ROOT)}")


def write_rust_vs_python(summary: dict | None) -> None:
    rust_label, rust_eps, python_label, python_eps = comparison_entries(summary)

    fig, ax = plt.subplots(figsize=(8, 5))
    if rust_eps is not None and python_eps is not None:
        labels = [rust_label or "Rust engine", python_label or "naive baseline"]
        values = [rust_eps, python_eps]
        ax.bar(labels, values, color=["#4c72b0", "#dd8452"])
        ax.set_ylabel("Events per second")
        ax.set_title("Rust engine vs naive Python baseline (measured)")
    else:
        ax.text(
            0.5,
            0.5,
            "No measured comparison data yet\n"
            "Populate comparisons.rust_engine and comparisons.python_baseline\n"
            "in out/bench_summary.json",
            ha="center",
            va="center",
            transform=ax.transAxes,
            fontsize=11,
        )
        ax.set_xticks([])
        ax.set_yticks([])
        ax.set_ylabel("Events per second (no measured data yet)")
        ax.set_title("Rust vs Python baseline (placeholder layout)")

    ax.grid(axis="y", alpha=0.3)
    fig.tight_layout()
    fig.savefig(RUST_VS_PYTHON_CHART, dpi=150)
    plt.close(fig)
    print(f"Wrote {RUST_VS_PYTHON_CHART.relative_to(REPO_ROOT)}")


def main() -> int:
    ARTIFACTS_DIR.mkdir(parents=True, exist_ok=True)
    summary = load_bench_summary()
    workloads = workloads_from_summary(summary)

    if summary is None:
        print(
            f"Missing {BENCH_SUMMARY_PATH.relative_to(REPO_ROOT)} — "
            "generating placeholder charts where needed."
        )

    write_latency_histogram(workloads)
    write_throughput_chart(workloads)
    write_rust_vs_python(summary)
    return 0


if __name__ == "__main__":
    sys.exit(main())
