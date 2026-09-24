use super::*;
use scrap::codec::{Quality, BR_BALANCED, BR_BEST, BR_SPEED};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/*
LAN/VPN links can be bandwidth constrained. Start conservatively, reduce the
bitrate before the frame budget, and recover gradually after congestion clears.
Client quality/FPS choices are upper bounds; decoder feedback is also respected.
*/

// Constants
pub const FPS: u32 = 60;
pub const MIN_FPS: u32 = 1;
pub const MAX_FPS: u32 = 120;
pub const INIT_FPS: u32 = 30;

// Bitrate ratio constants for different quality levels
const BR_MAX: f32 = 40.0; // 2000 * 2 / 100
const BR_MIN: f32 = 0.2;
const BR_MIN_HIGH_RESOLUTION: f32 = 0.1; // For high resolution, BR_MIN is still too high, so we set a lower limit
const MAX_BR_MULTIPLE: f32 = 1.0;
const LAN_DEFAULT_BITRATE_RATIO: f32 = 2.0; // 100% in the custom quality UI

const HISTORY_DELAY_LEN: usize = 2;
const ADJUST_RATIO_INTERVAL: usize = 3; // Adjust quality ratio every 3 seconds
const DYNAMIC_SCREEN_THRESHOLD: usize = 2; // Allow increase quality ratio if encode more than 2 times in one second
const DELAY_THRESHOLD_150MS: u32 = 150; // 150ms is the threshold for good network condition

#[derive(Default, Debug, Clone)]
struct UserDelay {
    delay_history: VecDeque<u32>,
    fps: Option<u32>,
    rtt_calculator: RttCalculator,
    response_elapsed: u32,
    healthy_samples: u32,
}

impl UserDelay {
    fn add_delay(&mut self, delay: u32) {
        self.rtt_calculator.update(delay);
        if self.delay_history.len() >= HISTORY_DELAY_LEN {
            self.delay_history.pop_front();
        }
        self.delay_history.push_back(delay);
    }

    // Average delay minus RTT
    fn avg_delay(&self) -> u32 {
        let len = self.delay_history.len();
        if len > 0 {
            let avg_delay = (self
                .delay_history
                .iter()
                .map(|v| u64::from(*v))
                .sum::<u64>()
                / len as u64) as u32;

            // If RTT is available, subtract it from average delay to get actual network latency
            if let Some(rtt) = self.rtt_calculator.get_rtt() {
                avg_delay.max(self.response_elapsed).saturating_sub(rtt)
            } else {
                avg_delay.max(self.response_elapsed)
            }
        } else {
            self.response_elapsed.max(DELAY_THRESHOLD_150MS)
        }
    }

    fn adjust_frame_budget(&mut self, ceiling: u32) {
        let current = self.fps.unwrap_or(INIT_FPS).min(ceiling);
        let delay = self.avg_delay();
        self.healthy_samples = if delay < 100 {
            self.healthy_samples.saturating_add(1)
        } else {
            0
        };
        let next = if delay >= 500 {
            (current / 2).max(5).min(current)
        } else if delay >= 200 {
            (current * 4 / 5).max(10).min(current)
        } else if self.healthy_samples >= 3 {
            self.healthy_samples = 0;
            current + 5
        } else {
            current
        };
        self.fps = Some(next.clamp(MIN_FPS, ceiling));
    }
}

// User session data structure
#[derive(Default, Debug, Clone)]
struct UserData {
    auto_adjust_fps: Option<u32>, // reserve for compatibility
    custom_fps: Option<u32>,
    quality: Option<(i64, Quality)>, // (time, quality)
    delay: UserDelay,
    record: bool,
}

#[derive(Default, Debug, Clone)]
struct DisplayData {
    send_counter: usize, // Number of times encode during period
    support_changing_quality: bool,
}

// Main QoS controller structure
pub struct VideoQoS {
    fps: u32,
    ratio: f32,
    users: HashMap<i32, UserData>,
    displays: HashMap<String, DisplayData>,
    bitrate_store: u32,
    adjust_ratio_instant: Instant,
    abr_config: bool,
    new_user_instant: Instant,
    profile: String,
    adaptive_resolution: bool,
    scale: u32,
    congested_samples: u32,
    clear_samples: u32,
    pub capture_ms: u32,
    pub encode_ms: u32,
    pub send_ms: u32,
    pub stream_size: (u32, u32),
}

impl Default for VideoQoS {
    fn default() -> Self {
        VideoQoS {
            fps: INIT_FPS,
            ratio: BR_BALANCED,
            users: Default::default(),
            displays: Default::default(),
            bitrate_store: 0,
            adjust_ratio_instant: Instant::now(),
            abr_config: true,
            new_user_instant: Instant::now(),
            profile: "latency".into(),
            adaptive_resolution: false,
            scale: 1,
            congested_samples: 0,
            clear_samples: 0,
            capture_ms: 0,
            encode_ms: 0,
            send_ms: 0,
            stream_size: (0, 0),
        }
    }
}

// Basic functionality
impl VideoQoS {
    pub fn set_profile(&mut self, profile: &str, adaptive_resolution: Option<bool>) {
        if matches!(profile, "latency" | "text" | "quality") {
            self.profile = profile.into();
        }
        if let Some(enabled) = adaptive_resolution {
            self.adaptive_resolution = enabled;
        }
        if self.profile != "latency" || !self.adaptive_resolution {
            self.scale = 1;
        }
    }

    pub fn resolution_divisor(&self) -> u32 {
        if self.record() { 1 } else { self.scale }
    }

    fn update_resolution(&mut self) {
        let delay = self.users.values().map(|u| u.delay.avg_delay()).max().unwrap_or(0);
        self.congested_samples = if delay >= 250 { self.congested_samples + 1 } else { 0 };
        self.clear_samples = if delay < 80 { self.clear_samples + 1 } else { 0 };
        if self.profile == "latency" && self.adaptive_resolution {
            if self.congested_samples >= 3 {
                self.scale = (self.scale * 2).min(4);
                self.congested_samples = 0;
                self.clear_samples = 0;
            } else if self.clear_samples >= 15 {
                self.scale = (self.scale / 2).max(1);
                self.clear_samples = 0;
            }
        }
    }

    // Calculate seconds per frame based on current FPS
    pub fn spf(&self) -> Duration {
        Duration::from_secs_f32(1. / (self.fps() as f32))
    }

    // Get current FPS within valid range
    pub fn fps(&self) -> u32 {
        let fps = self.fps;
        if fps >= MIN_FPS && fps <= MAX_FPS {
            fps
        } else {
            FPS
        }
    }

    // Store bitrate for later use
    pub fn store_bitrate(&mut self, bitrate: u32) {
        self.bitrate_store = bitrate;
    }

    // Get stored bitrate
    pub fn bitrate(&self) -> u32 {
        self.bitrate_store
    }

    // Get current bitrate ratio with bounds checking
    pub fn ratio(&mut self) -> f32 {
        if self.ratio < BR_MIN_HIGH_RESOLUTION || self.ratio > BR_MAX {
            self.ratio = BR_BALANCED;
        }
        self.ratio
    }

    // Check if any user is in recording mode
    pub fn record(&self) -> bool {
        self.users.iter().any(|u| u.1.record)
    }

    pub fn set_support_changing_quality(&mut self, video_service_name: &str, support: bool) {
        if let Some(display) = self.displays.get_mut(video_service_name) {
            display.support_changing_quality = support;
        }
    }

    // Check if variable bitrate encoding is supported and enabled
    pub fn in_vbr_state(&self) -> bool {
        self.abr_config && self.displays.iter().all(|e| e.1.support_changing_quality)
    }
}

// User session management
impl VideoQoS {
    // Initialize new user session
    pub fn on_connection_open(&mut self, id: i32) {
        self.users.insert(id, UserData::default());
        self.new_user_instant = Instant::now();
    }

    // Clean up user session
    pub fn on_connection_close(&mut self, id: i32) {
        self.users.remove(&id);
        if self.users.is_empty() {
            *self = Default::default();
        }
    }

    pub fn user_custom_fps(&mut self, id: i32, fps: u32) {
        if fps < MIN_FPS || fps > MAX_FPS {
            return;
        }
        if let Some(user) = self.users.get_mut(&id) {
            user.custom_fps = Some(fps);
        }
        self.adjust_fps();
    }

    pub fn user_auto_adjust_fps(&mut self, id: i32, fps: u32) {
        if fps < MIN_FPS || fps > MAX_FPS {
            return;
        }
        if let Some(user) = self.users.get_mut(&id) {
            user.auto_adjust_fps = Some(fps);
        }
        self.adjust_fps();
    }

    pub fn user_image_quality(&mut self, id: i32, image_quality: i32) {
        let convert_quality = |q: i32| -> Quality {
            if q == ImageQuality::Balanced.value() {
                Quality::Balanced
            } else if q == ImageQuality::Low.value() {
                Quality::Low
            } else if q == ImageQuality::Best.value() {
                Quality::Best
            } else {
                let b = ((q >> 8 & 0xFFF) * 2) as f32 / 100.0;
                Quality::Custom(b.clamp(BR_MIN, BR_MAX))
            }
        };

        let quality = Some((hbb_common::get_time(), convert_quality(image_quality)));
        if let Some(user) = self.users.get_mut(&id) {
            user.quality = quality;
            // update ratio directly
            self.ratio = self.ratio.min(self.latest_quality().ratio());
        }
    }

    pub fn user_record(&mut self, id: i32, v: bool) {
        if let Some(user) = self.users.get_mut(&id) {
            user.record = v;
        }
    }

    pub fn user_network_delay(&mut self, id: i32, delay: u32) {
        let highest_fps = self.highest_fps();
        if let Some(user) = self.users.get_mut(&id) {
            let delay = delay.max(10);
            user.delay.response_elapsed = 0;
            user.delay.add_delay(delay);
            user.delay.adjust_frame_budget(highest_fps);
        }
        self.adjust_fps();
        self.update_resolution();
    }

    pub fn user_delay_response_elapsed(&mut self, id: i32, elapsed: u128) {
        let highest_fps = self.highest_fps();
        if let Some(user) = self.users.get_mut(&id) {
            if elapsed >= 1000 {
                // An outstanding probe is a lower bound, not a completed RTT
                // sample. Do not let a stalled connection redefine baseline RTT.
                user.delay.response_elapsed = elapsed.min(u32::MAX as u128) as u32;
                user.delay.adjust_frame_budget(highest_fps);
            }
        }
        self.adjust_fps();
        self.update_resolution();
    }
}

// Common adjust functions
impl VideoQoS {
    pub fn new_display(&mut self, video_service_name: String) {
        self.displays
            .insert(video_service_name, DisplayData::default());
    }

    pub fn remove_display(&mut self, video_service_name: &str) {
        self.displays.remove(video_service_name);
    }

    pub fn update_display_data(&mut self, video_service_name: &str, send_counter: usize) {
        if let Some(display) = self.displays.get_mut(video_service_name) {
            display.send_counter += send_counter;
        }
        self.adjust_fps();
        let abr_enabled = self.in_vbr_state();
        if abr_enabled {
            let congested = self
                .users
                .values()
                .any(|u| u.delay.avg_delay() >= DELAY_THRESHOLD_150MS);
            let interval = if congested {
                1
            } else {
                ADJUST_RATIO_INTERVAL as u64
            };
            if self.adjust_ratio_instant.elapsed().as_secs() >= interval {
                let dynamic_screen = self
                    .displays
                    .iter()
                    .any(|d| d.1.send_counter >= ADJUST_RATIO_INTERVAL * DYNAMIC_SCREEN_THRESHOLD);
                self.displays.iter_mut().for_each(|d| {
                    d.1.send_counter = 0;
                });
                self.adjust_ratio(dynamic_screen);
            }
        } else {
            self.ratio = self.latest_quality().ratio();
        }
    }

    #[inline]
    fn highest_fps(&self) -> u32 {
        let user_fps = |u: &UserData| {
            let profile_cap = match self.profile.as_str() { "text" => 30, "quality" => 45, _ => MAX_FPS };
            let mut fps = u.custom_fps.unwrap_or(FPS).min(profile_cap);
            if let Some(auto_adjust_fps) = u.auto_adjust_fps {
                if fps == 0 || auto_adjust_fps < fps {
                    fps = auto_adjust_fps;
                }
            }
            fps
        };

        let fps = self
            .users
            .iter()
            .map(|(_, u)| user_fps(u))
            .filter(|u| *u >= MIN_FPS)
            .min()
            .unwrap_or(FPS);

        fps.clamp(MIN_FPS, MAX_FPS)
    }

    // Get latest quality settings from all users
    pub fn latest_quality(&self) -> Quality {
        self.users
            .iter()
            .map(|(_, u)| u.quality)
            .filter(|q| *q != None)
            .max_by(|a, b| a.unwrap_or_default().0.cmp(&b.unwrap_or_default().0))
            .flatten()
            .unwrap_or((0, Quality::Custom(LAN_DEFAULT_BITRATE_RATIO)))
            .1
    }

    // Adjust quality ratio based on network delay and screen changes
    fn adjust_ratio(&mut self, dynamic_screen: bool) {
        if !self.in_vbr_state() {
            return;
        }
        // Get maximum delay from all users
        let max_delay = self.users.iter().map(|u| u.1.delay.avg_delay()).max();
        let Some(max_delay) = max_delay else {
            return;
        };

        let target_quality = self.latest_quality();
        let target_ratio = self.latest_quality().ratio();
        let current_ratio = self.ratio;
        let current_bitrate = self.bitrate();

        // Calculate minimum ratio for high resolution (1Mbps baseline)
        let ratio_1mbps = if current_bitrate > 0 {
            Some((current_ratio * 1000.0 / current_bitrate as f32).max(BR_MIN_HIGH_RESOLUTION))
        } else {
            None
        };

        // Calculate ratio for adding 150kbps bandwidth
        let ratio_add_150kbps = if current_bitrate > 0 {
            Some((current_bitrate + 150) as f32 * current_ratio / current_bitrate as f32)
        } else {
            None
        };

        // Set minimum ratio based on quality mode
        let mut min = match target_quality {
            Quality::Best => {
                // For Best quality, ensure minimum 1Mbps for high resolution
                let mut min = BR_BEST / 2.5;
                if let Some(ratio_1mbps) = ratio_1mbps {
                    if min > ratio_1mbps {
                        min = ratio_1mbps;
                    }
                }
                min.max(BR_MIN)
            }
            Quality::Balanced => {
                let mut min = (BR_BALANCED / 2.0).min(0.4);
                if let Some(ratio_1mbps) = ratio_1mbps {
                    if min > ratio_1mbps {
                        min = ratio_1mbps;
                    }
                }
                min.max(BR_MIN_HIGH_RESOLUTION)
            }
            Quality::Low => BR_MIN_HIGH_RESOLUTION,
            Quality::Custom(_) => BR_MIN_HIGH_RESOLUTION,
        };
        let max = target_ratio * MAX_BR_MULTIPLE;
        if self.profile == "text" { min = min.max(0.2).min(max); }
        if self.profile == "quality" { min = min.max(0.4).min(max); }

        let mut v = current_ratio;

        // Adjust ratio based on network delay thresholds
        if max_delay < 50 {
            if dynamic_screen {
                v = current_ratio * 1.15;
            }
        } else if max_delay < 100 {
            if dynamic_screen {
                v = current_ratio * 1.1;
            }
        } else if max_delay < DELAY_THRESHOLD_150MS {
            if dynamic_screen {
                v = current_ratio * 1.05;
            }
        } else if max_delay < 200 {
            v = current_ratio * 0.9;
        } else if max_delay < 300 {
            v = current_ratio * 0.8;
        } else if max_delay < 500 {
            v = current_ratio * 0.65;
        } else {
            v = current_ratio * 0.5;
        }

        // Limit quality increase rate for better stability
        if let Some(ratio_add_150kbps) = ratio_add_150kbps {
            if v > ratio_add_150kbps
                && ratio_add_150kbps > current_ratio
                && current_ratio >= BR_SPEED
            {
                v = ratio_add_150kbps;
            }
        }

        self.ratio = v.clamp(min, max);
        self.adjust_ratio_instant = Instant::now();
    }

    // Adjust fps based on network delay and user response time
    fn adjust_fps(&mut self) {
        let highest_fps = self.highest_fps();
        // Get minimum fps from all users
        let mut fps = self
            .users
            .iter()
            .map(|u| u.1.delay.fps.unwrap_or(INIT_FPS))
            .min()
            .unwrap_or(INIT_FPS);

        // Keep startup bounded by the LAN profile, while respecting a lower
        // explicit/decoder-provided frame limit from `highest_fps` below.
        if self.new_user_instant.elapsed().as_secs() < 1 {
            if fps > INIT_FPS {
                fps = INIT_FPS;
            }
        }

        // Ensure fps stays within valid range
        self.fps = fps.clamp(MIN_FPS, highest_fps);
    }
}

/// Feedback is scoped to a viewer. A stalled viewer must not lower another's budget.
#[derive(Default)]
pub struct VideoQoSRegistry {
    peers: HashMap<i32, VideoQoS>,
}

impl VideoQoSRegistry {
    pub fn peer(&mut self, id: i32) -> &mut VideoQoS {
        self.peers.entry(id).or_insert_with(|| {
            let mut qos = VideoQoS::default();
            qos.on_connection_open(id);
            qos
        })
    }
    pub fn on_connection_open(&mut self, id: i32) { self.peer(id); }
    pub fn on_connection_close(&mut self, id: i32) { self.peers.remove(&id); }
    pub fn bitrate(&self, id: i32) -> u32 { self.peers.get(&id).map_or(0, VideoQoS::bitrate) }
    pub fn report(&self, id: i32) -> TestDelay {
        self.peers.get(&id).map_or_else(TestDelay::new, |q| TestDelay {
            target_bitrate: q.bitrate(), capture_ms: q.capture_ms,
            encode_ms: q.encode_ms, send_ms: q.send_ms,
            stream_width: q.stream_size.0, stream_height: q.stream_size.1,
            ..Default::default()
        })
    }
    pub fn user_custom_fps(&mut self, id: i32, fps: u32) {
        if let Some(q) = self.peers.get_mut(&id) { q.user_custom_fps(id, fps); }
    }
    pub fn user_auto_adjust_fps(&mut self, id: i32, fps: u32) {
        if let Some(q) = self.peers.get_mut(&id) { q.user_auto_adjust_fps(id, fps); }
    }
    pub fn user_image_quality(&mut self, id: i32, quality: i32) {
        if let Some(q) = self.peers.get_mut(&id) { q.user_image_quality(id, quality); }
    }
    pub fn user_record(&mut self, id: i32, record: bool) {
        if let Some(q) = self.peers.get_mut(&id) { q.user_record(id, record); }
    }
    pub fn user_network_delay(&mut self, id: i32, delay: u32) {
        if let Some(q) = self.peers.get_mut(&id) { q.user_network_delay(id, delay); }
    }
    pub fn user_delay_response_elapsed(&mut self, id: i32, elapsed: u128) {
        if let Some(q) = self.peers.get_mut(&id) { q.user_delay_response_elapsed(id, elapsed); }
    }
    pub fn remove_display(&mut self, name: &str) {
        for q in self.peers.values_mut() { q.remove_display(name); }
    }
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
#[path = "weak_network_tests.rs"]
mod weak_network_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_viewer_and_its_quality_changes_do_not_throttle_healthy_viewer() {
        let mut registry = VideoQoSRegistry::default();
        for id in [1, 2] {
            let q = registry.peer(id);
            q.new_display("monitor0".into());
            q.set_support_changing_quality("monitor0", true);
            q.new_user_instant = Instant::now() - Duration::from_secs(2);
            for _ in 0..10 { q.user_network_delay(id, 10); }
        }
        for _ in 0..40 {
            registry.user_network_delay(1, 10);
            registry.user_network_delay(2, 1000);
            registry.peer(1).adjust_ratio(true);
            registry.peer(2).adjust_ratio(true);
        }
        registry.user_custom_fps(2, 8);
        registry.user_image_quality(2, ImageQuality::Low.value());
        assert_eq!(registry.peer(1).fps(), 60);
        assert!(registry.peer(2).fps() <= 10);
        assert!(registry.peer(1).ratio() > registry.peer(2).ratio());
        registry.on_connection_close(2);
        assert_eq!(registry.peer(1).fps(), 60);
    }

    #[test]
    fn latency_scaling_requires_capability_and_sustained_congestion() {
        let mut q = active_qos();
        q.set_profile("latency", Some(true));
        q.user_network_delay(1, 800);
        assert_eq!(q.resolution_divisor(), 1);
        for _ in 0..5 { q.user_network_delay(1, 800); }
        assert_eq!(q.resolution_divisor(), 4);
        q.user_network_delay(1, 10);
        assert_eq!(q.resolution_divisor(), 4);
        for _ in 0..35 { q.user_network_delay(1, 10); }
        assert_eq!(q.resolution_divisor(), 1);
        q.set_profile("text", Some(true));
        for _ in 0..8 { q.user_network_delay(1, 800); }
        assert_eq!(q.resolution_divisor(), 1);
        q.set_profile("latency", Some(false));
        for _ in 0..8 { q.user_network_delay(1, 800); }
        assert_eq!(q.resolution_divisor(), 1);
    }

    fn active_qos() -> VideoQoS {
        let mut qos = VideoQoS::default();
        qos.on_connection_open(1);
        qos.new_display("monitor_0".into());
        qos.set_support_changing_quality("monitor_0", true);
        qos
    }

    #[test]
    fn startup_limits_burst_and_healthy_link_recovers() {
        let mut qos = active_qos();
        assert_eq!(qos.fps(), 30);
        assert_eq!(qos.ratio(), BR_BALANCED);
        qos.new_user_instant = Instant::now() - Duration::from_secs(2);
        for _ in 0..30 {
            qos.user_network_delay(1, 10);
            qos.adjust_ratio(true);
        }
        assert_eq!(qos.fps(), 60);
        assert_eq!(qos.ratio(), LAN_DEFAULT_BITRATE_RATIO);
    }

    #[test]
    fn congestion_reduces_bitrate_and_frame_budget_then_recovers_gradually() {
        let mut qos = active_qos();
        qos.new_user_instant = Instant::now() - Duration::from_secs(2);
        for _ in 0..30 {
            qos.user_network_delay(1, 20);
        }
        let initial_ratio = qos.ratio();
        for _ in 0..4 {
            qos.user_network_delay(1, 800);
            qos.adjust_ratio(true);
        }
        assert!(qos.fps() <= 10);
        assert!(qos.ratio() < initial_ratio / 2.0);
        assert!(qos.ratio() >= BR_MIN_HIGH_RESOLUTION);
        qos.user_network_delay(1, 20);
        assert!(qos.fps() <= 10); // Do not rebound on a single good sample.
        for _ in 0..40 {
            qos.user_network_delay(1, 20);
            qos.adjust_ratio(true);
        }
        assert_eq!(qos.fps(), 60);
        assert!(qos.ratio() > initial_ratio);
        assert!(qos.ratio() <= qos.latest_quality().ratio());
    }

    #[test]
    fn stable_vpn_rtt_is_not_mistaken_for_queued_video() {
        let mut qos = active_qos();
        qos.new_user_instant = Instant::now() - Duration::from_secs(2);
        for _ in 0..60 {
            qos.user_network_delay(1, 250);
        }
        assert_eq!(qos.users[&1].delay.avg_delay(), 0);
        assert_eq!(qos.fps(), 60);
    }

    #[test]
    fn missing_response_throttles_without_poisoning_rtt_baseline() {
        let mut qos = active_qos();
        qos.new_user_instant = Instant::now() - Duration::from_secs(2);
        for _ in 0..15 {
            qos.user_network_delay(1, 20);
        }
        let previous_fps = qos.fps();
        let baseline = qos.users[&1].delay.rtt_calculator.get_rtt();
        qos.user_delay_response_elapsed(1, 3000);
        qos.adjust_ratio(true);
        assert!(qos.fps() <= previous_fps / 2);
        assert!(qos.ratio() < BR_BALANCED);
        assert_eq!(qos.users[&1].delay.rtt_calculator.get_rtt(), baseline);
    }

    #[test]
    fn decoder_and_user_caps_are_respected_for_multiple_viewers() {
        let mut qos = active_qos();
        qos.on_connection_open(2);
        qos.user_custom_fps(1, 20);
        qos.user_auto_adjust_fps(2, 12);
        qos.user_image_quality(1, ImageQuality::Low.value());
        for _ in 0..20 {
            qos.user_network_delay(1, 10);
            qos.user_network_delay(2, 10);
            qos.adjust_ratio(true);
        }
        assert_eq!(qos.fps(), 12);
        assert!(qos.ratio() <= BR_SPEED);
        qos.user_network_delay(2, 1000);
        assert!(qos.fps() < 12);
        qos.on_connection_close(2);
        for _ in 0..20 {
            qos.user_network_delay(1, 10);
        }
        assert_eq!(qos.fps(), 20);
    }

    #[test]
    fn unsupported_encoder_still_adapts_fps_without_quality_restarts() {
        let mut qos = active_qos();
        qos.set_support_changing_quality("monitor_0", false);
        qos.user_network_delay(1, 900);
        let ratio = qos.ratio();
        qos.adjust_ratio(true);
        assert!(!qos.in_vbr_state());
        assert_eq!(qos.ratio(), ratio);
        assert!(qos.fps() < INIT_FPS);
    }

    #[test]
    fn delay_history_stays_bounded_and_handles_large_samples() {
        let mut delay = UserDelay::default();
        for _ in 0..5 {
            delay.add_delay(u32::MAX);
        }
        assert_eq!(delay.delay_history.len(), HISTORY_DELAY_LEN);
        assert_eq!(delay.avg_delay(), u32::MAX);
    }
}
