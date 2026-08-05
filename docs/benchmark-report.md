# Benchmark Report

Canonical measured report: **[benchmark_report.md](./benchmark_report.md)**.

Machine-readable source of truth: [`benchmarks/latest.json`](./benchmarks/latest.json).

Regenerate / print:

```bash
cargo run --release --bin engine-cli -- benchmark-report
cargo run --release --bin engine-cli -- benchmark-report --refresh --charts
```
