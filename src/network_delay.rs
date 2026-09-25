//! Shared queue-delay estimate for video adaptation and file-transfer pacing.
use std::collections::VecDeque;

const HISTORY_DELAY_LEN: usize = 2;

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
            self.response_elapsed = elapsed.min(u32::MAX as u128) as u32;
        }
    }

    pub fn baseline_rtt(&self) -> Option<u32> { self.rtt.get_rtt() }

    pub fn queue_delay(&self) -> u32 {
        if self.history.is_empty() {
            return self.response_elapsed.max(150);
        }
        let average = (self.history.iter().map(|&v| u64::from(v)).sum::<u64>()
            / self.history.len() as u64) as u32;
        average.max(self.response_elapsed).saturating_sub(self.baseline_rtt().unwrap_or(0))
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
    const MIN_SAMPLES: usize = 10; // Require at least 10 samples
    const ALPHA: f32 = 0.5; // Smoothing factor for weighted average

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
                let new_srtt =
                    ((1.0 - Self::ALPHA) * min as f32 + Self::ALPHA * window_min as f32) as u32;
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
        if self.samples.len() >= Self::MIN_SAMPLES {
            if let Some(rtt) = self.min_rtt {
                return Some(rtt);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_vpn_latency_and_transient_queueing_are_distinguished() {
        let mut delay = NetworkDelay::default();
        for _ in 0..60 { delay.record(250); }
        assert_eq!(delay.queue_delay(), 0);
        assert!(!delay.congested());
        delay.record(850);
        assert_eq!(delay.queue_delay(), 300);
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
    fn samples_are_bounded_and_large_delays_do_not_overflow() {
        let mut delay = NetworkDelay::default();
        for _ in 0..5 { delay.record(u32::MAX); }
        assert_eq!(delay.history.len(), HISTORY_DELAY_LEN);
        assert_eq!(delay.queue_delay(), u32::MAX);
        for _ in 0..100 { delay.record(250); }
        assert_eq!(delay.rtt.samples.len(), RttCalculator::WINDOW_SAMPLES);
    }
}
