//! Shared queue-delay estimate for video adaptation and file-transfer pacing.
use std::collections::VecDeque;

const HISTORY_DELAY_LEN: usize = 3;

#[derive(Default, Debug, Clone)]
pub(crate) struct NetworkDelay {
    history: VecDeque<u32>,
    rtt: RttCalculator,
    response_elapsed: u32,
}

impl NetworkDelay {
    pub fn record(&mut self, delay: u32) {
        let delay = delay.max(10);
        self.response_elapsed = 0;
        self.rtt.update(delay);
        if self.history.len() >= HISTORY_DELAY_LEN {
            self.history.pop_front();
        }
        self.history.push_back(delay);
    }

    pub fn stalled(&mut self, elapsed: u128) {
        if elapsed >= 1000 {
            // An outstanding response is not a completed RTT sample.
            // Several outstanding operations can report in one timer tick.
            // Keep the strongest lower bound until a response resets it.
            self.response_elapsed = self.response_elapsed.max(elapsed.min(u32::MAX as u128) as u32);
        }
    }

    pub fn baseline_rtt(&self) -> Option<u32> { self.rtt.get_rtt() }

    pub fn queue_delay(&self) -> u32 {
        if self.history.is_empty() {
            return self.response_elapsed.max(150);
        }
        // A single completed spike must not look like sustained queueing. Use
        // the lower median during warmup; outstanding probes bypass this filter.
        let mut samples = [0; HISTORY_DELAY_LEN];
        for (sample, delay) in samples.iter_mut().zip(&self.history) { *sample = *delay; }
        let samples = &mut samples[..self.history.len()];
        samples.sort_unstable();
        samples[(samples.len() - 1) / 2].max(self.response_elapsed)
            .saturating_sub(self.baseline_rtt().unwrap_or(0))
    }

    pub fn congested(&self) -> bool { self.queue_delay() >= 200 }
}

#[derive(Default, Debug, Clone)]
struct RttCalculator {
    min_rtt: Option<u32>,        // Historical minimum RTT ever observed
    window_min_rtt: Option<u32>, // Minimum RTT within last 60 samples
    smoothed_rtt: Option<u32>,   // Smoothed RTT estimation
    samples: VecDeque<u32>,      // Last 60 RTT samples
}

impl RttCalculator {
    const WINDOW_SAMPLES: usize = 60; // Keep last 60 samples

    /// Update RTT estimates with a new sample
    pub fn update(&mut self, delay: u32) {
        // 1. Update historical minimum RTT
        match self.min_rtt {
            Some(min_rtt) if delay < min_rtt => self.min_rtt = Some(delay),
            None => self.min_rtt = Some(delay),
            _ => {}
        }

        // 2. Update sample window
        if self.samples.len() >= Self::WINDOW_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(delay);

        // 3. Calculate minimum RTT within the window
        self.window_min_rtt = self.samples.iter().min().copied();

        // 4. Calculate smoothed RTT
        // Use weighted average if we have enough samples
        if self.samples.len() >= Self::WINDOW_SAMPLES {
            if let (Some(min), Some(window_min)) = (self.min_rtt, self.window_min_rtt) {
                // Weighted average of historical minimum and window minimum
                let new_srtt = ((u64::from(min) + u64::from(window_min)) / 2) as u32;
                self.smoothed_rtt = Some(new_srtt);
            }
        }
    }

    /// Get current RTT estimate
    /// Returns None if no valid estimation is available
    pub fn get_rtt(&self) -> Option<u32> {
        if let Some(rtt) = self.smoothed_rtt {
            return Some(rtt);
        }
        // The first response is a provisional baseline, not queueing delay.
        // A later faster response immediately lowers it.
        self.min_rtt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_vpn_latency_and_transient_queueing_are_distinguished() {
        let mut delay = NetworkDelay::default();
        for _ in 0..60 {
            delay.record(250);
            assert_eq!(delay.baseline_rtt(), Some(250));
            assert_eq!(delay.queue_delay(), 0);
        }
        assert_eq!(delay.queue_delay(), 0);
        assert!(!delay.congested());
        delay.record(850);
        assert_eq!(delay.queue_delay(), 0);
        assert!(!delay.congested());
        delay.record(850);
        assert_eq!(delay.queue_delay(), 600);
        assert!(delay.congested());
        delay.record(250);
        delay.record(250);
        assert!(!delay.congested());
    }

    #[test]
    fn an_outstanding_probe_does_not_raise_the_rtt_baseline() {
        let mut delay = NetworkDelay::default();
        for _ in 0..60 { delay.record(250); }
        delay.stalled(3000);
        assert!(delay.congested());
        assert_eq!(delay.baseline_rtt(), Some(250));
        delay.record(250);
        assert!(!delay.congested());
    }

    #[test]
    fn a_younger_pending_operation_cannot_hide_an_older_stall() {
        let mut delay = NetworkDelay::default();
        delay.record(250);
        delay.stalled(4000);
        delay.stalled(1000);
        assert_eq!(delay.queue_delay(), 3750);
        delay.record(250);
        assert_eq!(delay.queue_delay(), 0);
    }

    #[test]
    fn samples_are_bounded_and_large_delays_do_not_overflow() {
        let mut delay = NetworkDelay::default();
        delay.record(10);
        for _ in 0..5 { delay.record(u32::MAX); }
        assert_eq!(delay.history.len(), HISTORY_DELAY_LEN);
        assert_eq!(delay.queue_delay(), u32::MAX - 10);
        for _ in 0..100 { delay.record(250); }
        assert_eq!(delay.rtt.samples.len(), RttCalculator::WINDOW_SAMPLES);
    }

    #[test]
    fn a_slow_first_response_is_replaced_by_a_faster_baseline() {
        let mut delay = NetworkDelay::default();
        delay.record(900);
        assert_eq!(delay.queue_delay(), 0);
        delay.record(20);
        assert_eq!(delay.baseline_rtt(), Some(20));
        assert_eq!(delay.queue_delay(), 0);
        for _ in 0..120 { delay.record(820); }
        assert!(delay.congested(), "persistent queueing must not become a healthy baseline");
    }

    #[test]
    fn baseline_blending_does_not_round_large_delays_or_overflow() {
        let mut delay = NetworkDelay::default();
        for _ in 0..120 { delay.record(u32::MAX); }
        assert_eq!(delay.baseline_rtt(), Some(u32::MAX));
        assert_eq!(delay.queue_delay(), 0);
        delay.stalled(u128::MAX);
        assert_eq!(delay.response_elapsed, u32::MAX);
    }
}
