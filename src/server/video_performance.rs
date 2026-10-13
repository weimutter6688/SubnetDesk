//! Bounded, approximate local stage timings. No frame payloads or peer addresses.
#![deny(warnings, clippy::all)]

use hbb_common::log;
use std::time::{Duration, Instant};

const REPORT_INTERVAL: Duration = Duration::from_secs(5);
pub(super) const LOG_TARGET: &str = "subnetdesk::video_performance";

pub(super) fn enabled() -> bool {
    log::log_enabled!(target: LOG_TARGET, log::Level::Debug)
}

const STAGES: [&str; 8] = [
    "capture",
    "convert",
    "encode",
    "queue",
    "serialize",
    "encrypt",
    "feed",
    "flush",
];

#[derive(Clone, Copy)]
pub(super) enum Stage {
    Capture,
    Convert,
    Encode,
    Queue,
    Serialize,
    Encrypt,
    Feed,
    Flush,
}

#[derive(Default)]
struct Histogram {
    bins: [u64; 32],
    count: u64,
    total_us: u64,
    max_us: u64,
}

#[derive(Default, Clone, Copy)]
struct Summary {
    count: u64,
    average_us: u64,
    p95_upper_us: u64,
    max_us: u64,
}

impl Histogram {
    fn record(&mut self, elapsed: Duration) {
        let micros = elapsed.as_micros().min(u64::MAX as u128) as u64;
        let bucket = (u64::BITS - micros.leading_zeros()).min(31) as usize;
        self.bins[bucket] = self.bins[bucket].saturating_add(1);
        self.count = self.count.saturating_add(1);
        self.total_us = self.total_us.saturating_add(micros);
        self.max_us = self.max_us.max(micros);
    }

    fn summary(&self) -> Summary {
        if self.count == 0 {
            return Summary::default();
        }
        let target = self.count - self.count / 20;
        let mut cumulative: u64 = 0;
        let mut p95_upper_us = self.max_us;
        for (bucket, count) in self.bins.iter().enumerate() {
            cumulative = cumulative.saturating_add(*count);
            if cumulative >= target {
                p95_upper_us = if bucket == 31 {
                    self.max_us
                } else {
                    ((1u64 << bucket) - 1).min(self.max_us)
                };
                break;
            }
        }
        Summary {
            count: self.count,
            average_us: self.total_us / self.count,
            p95_upper_us,
            max_us: self.max_us,
        }
    }
}

#[derive(Default)]
pub(super) struct VideoPerformance {
    stages: [Histogram; 8],
    started: Option<Instant>,
    frames: u64,
    wire_bytes: u64,
}

impl VideoPerformance {
    pub fn record(&mut self, stage: Stage, elapsed: Duration) {
        if enabled() {
            self.started.get_or_insert_with(Instant::now);
            self.stages[stage as usize].record(elapsed);
        }
    }

    pub fn frame_flushed(&mut self, wire_bytes: usize, now: Instant) -> Option<Report> {
        if !enabled() {
            return None;
        }
        self.frames = self.frames.saturating_add(1);
        self.wire_bytes = self.wire_bytes.saturating_add(wire_bytes as u64);
        self.take_report(now)
    }

    fn take_report(&mut self, now: Instant) -> Option<Report> {
        let elapsed = now.saturating_duration_since(self.started?);
        if elapsed < REPORT_INTERVAL {
            return None;
        }
        let report = Report {
            elapsed,
            frames: self.frames,
            wire_bytes: self.wire_bytes,
            stages: std::array::from_fn(|i| self.stages[i].summary()),
        };
        *self = Self::default();
        Some(report)
    }
}

pub(super) struct Report {
    elapsed: Duration,
    frames: u64,
    wire_bytes: u64,
    stages: [Summary; 8],
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "window_ms={} flushed_frames={} wire_bytes={} timings_us=[n,avg,p95_upper,max]",
            self.elapsed.as_millis(),
            self.frames,
            self.wire_bytes
        )?;
        for (name, s) in STAGES.iter().zip(&self.stages) {
            write!(
                f,
                " {name}=[{},{},{},{}]",
                s.count, s.average_us, s.p95_upper_us, s.max_us
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_preserve_tail_costs_with_bounded_storage() {
        let mut histogram = Histogram::default();
        for _ in 0..94 {
            histogram.record(Duration::from_micros(100));
        }
        for _ in 0..6 {
            histogram.record(Duration::from_micros(10_000));
        }
        let summary = histogram.summary();
        assert_eq!(summary.count, 100);
        assert_eq!(summary.average_us, 694);
        assert_eq!(summary.p95_upper_us, 10_000);
        assert_eq!(summary.max_us, 10_000);
        histogram.record(Duration::MAX);
        assert_eq!(histogram.summary().max_us, u64::MAX);
    }

    #[test]
    fn reports_are_rate_limited_and_reset_all_stages() {
        let now = Instant::now();
        let mut performance = VideoPerformance {
            started: Some(now),
            ..Default::default()
        };
        performance.stages[Stage::Queue as usize].record(Duration::from_micros(500));
        performance.frames = 12;
        performance.wire_bytes = 4096;
        assert!(performance
            .take_report(now + Duration::from_secs(4))
            .is_none());
        let report = performance
            .take_report(now + Duration::from_secs(5))
            .unwrap();
        assert_eq!(report.frames, 12);
        assert_eq!(report.wire_bytes, 4096);
        assert_eq!(report.stages[Stage::Queue as usize].count, 1);
        assert!(performance.started.is_none());
        assert_eq!(performance.frames, 0);
        assert_eq!(performance.stages[Stage::Queue as usize].summary().count, 0);
    }
}
