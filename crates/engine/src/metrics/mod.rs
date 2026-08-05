//! Latency histogram and throughput counters for benchmark/metrics paths only.
//!
//! Wall-clock measurement must never appear in deterministic engine behavior.

use std::time::{Duration, Instant};

use hdrhistogram::Histogram;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyReport {
    pub samples: u64,
    pub p50_ns: u64,
    pub p90_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub p999_ns: Option<u64>,
    pub max_ns: u64,
    pub mean_ns: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputReport {
    pub events: u64,
    pub trades: u64,
    pub cancels: u64,
    pub elapsed_secs: f64,
    pub events_per_sec: f64,
    pub trades_per_sec: f64,
    pub cancels_per_sec: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSummary {
    pub workload: String,
    pub latency: Option<LatencyReport>,
    pub throughput: ThroughputReport,
}

/// Collects per-event latencies using hdrhistogram (metrics/bench only).
#[derive(Debug)]
pub struct LatencyCollector {
    hist: Histogram<u64>,
}

impl LatencyCollector {
    pub fn new() -> Self {
        // 1ns..1s with 3 significant digits
        Self {
            hist: Histogram::new_with_bounds(1, 1_000_000_000, 3).expect("histogram bounds"),
        }
    }

    pub fn record_ns(&mut self, nanos: u64) {
        let _ = self.hist.record(nanos.max(1));
    }

    pub fn record_duration(&mut self, d: Duration) {
        self.record_ns(d.as_nanos() as u64);
    }

    pub fn report(&self) -> LatencyReport {
        let samples = self.hist.len();
        let p999 = if samples >= 1000 {
            Some(self.hist.value_at_percentile(99.9))
        } else {
            None
        };
        LatencyReport {
            samples,
            p50_ns: self.hist.value_at_percentile(50.0),
            p90_ns: self.hist.value_at_percentile(90.0),
            p95_ns: self.hist.value_at_percentile(95.0),
            p99_ns: self.hist.value_at_percentile(99.0),
            p999_ns: p999,
            max_ns: self.hist.max(),
            mean_ns: self.hist.mean(),
        }
    }
}

impl Default for LatencyCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Time a closure for metrics/benchmarks only.
pub fn timed<R>(f: impl FnOnce() -> R) -> (R, Duration) {
    let start = Instant::now();
    let result = f();
    (result, start.elapsed())
}

pub fn throughput_report(
    events: u64,
    trades: u64,
    cancels: u64,
    elapsed: Duration,
) -> ThroughputReport {
    let secs = elapsed.as_secs_f64().max(1e-12);
    ThroughputReport {
        events,
        trades,
        cancels,
        elapsed_secs: secs,
        events_per_sec: events as f64 / secs,
        trades_per_sec: trades as f64 / secs,
        cancels_per_sec: cancels as f64 / secs,
    }
}
