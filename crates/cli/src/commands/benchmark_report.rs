//! `benchmark-report` — print or regenerate measured benchmark summary.

use std::{fs, path::PathBuf, process::Command};

use anyhow::{bail, Context, Result};
use clap::Args;

#[derive(Debug, Args)]
pub struct BenchmarkReportArgs {
    /// Path to measured JSON (default: docs/benchmarks/latest.json).
    #[arg(long, default_value = "docs/benchmarks/latest.json")]
    pub input: PathBuf,

    /// Re-run the release `measure` harness before reporting.
    #[arg(long)]
    pub refresh: bool,

    /// Also regenerate PNG charts via scripts/generate_charts.py.
    #[arg(long)]
    pub charts: bool,
}

pub fn run(args: BenchmarkReportArgs) -> Result<()> {
    if args.refresh {
        let status = Command::new("cargo")
            .args([
                "run",
                "--release",
                "-p",
                "engine-cli",
                "--bin",
                "measure",
                "--",
                "--output",
            ])
            .arg(&args.input)
            .status()
            .context("failed to spawn measure")?;
        if !status.success() {
            bail!("measure harness failed with {status}");
        }
    }

    let text = fs::read_to_string(&args.input)
        .with_context(|| format!("missing measured JSON {}", args.input.display()))?;
    let data: serde_json::Value = serde_json::from_str(&text).context("invalid benchmark JSON")?;

    if data.get("schema_version").and_then(|v| v.as_u64()) != Some(1) {
        bail!("unsupported schema_version in {}", args.input.display());
    }

    println!("Benchmark report ({})", args.input.display());
    if let Some(env) = data.get("environment") {
        let host = env
            .get("cpu")
            .or_else(|| env.get("machine_model"))
            .or_else(|| env.get("hostname"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let os = env
            .get("operating_system")
            .or_else(|| env.get("macos_version"))
            .or_else(|| env.get("os"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let rustc = env
            .get("rust_version")
            .or_else(|| env.get("rustc"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let profile = env
            .get("build_profile")
            .and_then(|v| v.as_str())
            .unwrap_or("release");
        println!("  host: {host} / {os} / {rustc} ({profile})");
    }
    println!();
    println!(
        "{:<28} {:>12} {:>14} {:>10}",
        "workload", "p50_ns", "events_per_s", "events"
    );
    println!("{}", "-".repeat(68));

    let workloads = data
        .get("workloads")
        .and_then(|v| v.as_array())
        .context("workloads missing")?;
    for w in workloads {
        let name = w.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let events = w.get("events").and_then(|v| v.as_u64()).unwrap_or(0);
        let p50 = w
            .pointer("/latency/p50_ns")
            .and_then(|v| v.as_u64())
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".into());
        let eps = w
            .pointer("/throughput/events_per_sec")
            .or_else(|| w.pointer("/throughput/events_per_second"))
            .and_then(|v| v.as_f64())
            .map(|v| format!("{v:.0}"))
            .unwrap_or_else(|| "-".into());
        println!("{name:<28} {p50:>12} {eps:>14} {events:>10}");
    }

    println!();
    println!("Source of truth: {}", args.input.display());
    println!("Full write-up: docs/benchmark_report.md");
    println!("Dashboard: docs/artifacts/dashboard.html");

    if args.charts {
        let py = if PathBuf::from(".venv/bin/python").is_file() {
            ".venv/bin/python"
        } else {
            "python3"
        };
        let status = Command::new(py)
            .arg("scripts/generate_charts.py")
            .status()
            .context("failed to spawn chart script")?;
        if !status.success() {
            bail!("chart generation failed with {status}");
        }
        println!("Charts refreshed under docs/artifacts/");
    }

    Ok(())
}
