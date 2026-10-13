use super::*;
use scrap::codec::{Quality, BR_BALANCED, BR_BEST, BR_SPEED};
use std::{
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

const ADJUST_RATIO_INTERVAL: usize = 3; // Adjust quality ratio every 3 seconds
const DYNAMIC_SCREEN_THRESHOLD: usize = 2; // Allow increase quality ratio if encode more than 2 times in one second
const DELAY_THRESHOLD_150MS: u32 = 150; // 150ms is the threshold for good network condition
const FEEDBACK_MAX_GAP: Duration = Duration::from_secs(2);
const FRAME_ADJUST_INTERVAL: Duration = Duration::from_secs(1);
const HEALTHY_HOLD: Duration = Duration::from_secs(2);

#[derive(Default, Debug, Clone)]
struct UserDelay {
    network: crate::network_delay::NetworkDelay,
    fps: Option<u32>,
    healthy_since: Option<Instant>,
    congested_since: Option<Instant>,
    last_feedback: Option<Instant>,
    last_adjusted: Option<Instant>,
    last_decreased: Option<Instant>,
}

impl UserDelay {
    fn add_delay(&mut self, delay: u32) {
        self.network.record(delay);
    }

    fn avg_delay(&self) -> u32 {
        self.network.queue_delay()
    }

    fn feedback_max_gap(&self) -> Duration {
        // Probes are serialized: a normal response may span several timer
        // ticks on a high-RTT path. Allow its baseline RTT plus timer slack.
        FEEDBACK_MAX_GAP + Duration::from_millis(u64::from(self.network.baseline_rtt().unwrap_or(0)))
    }

    fn adjust_frame_budget(&mut self, ceiling: u32, now: Instant, stalled: bool) {
        let current = self.fps.unwrap_or(INIT_FPS).min(ceiling);
        let delay = self.avg_delay();
        if self.last_feedback.map_or(false, |t| now.saturating_duration_since(t) > self.feedback_max_gap()) {
            // Silence is not evidence of a continuously healthy/congested link.
            self.healthy_since = None;
            self.congested_since = None;
        }
        self.last_feedback = Some(now);
        let can_adjust = self.last_adjusted.map_or(true, |t| {
            now.saturating_duration_since(t) >= FRAME_ADJUST_INTERVAL
        });
        let can_decrease = self.last_decreased.map_or(true, |t| {
            now.saturating_duration_since(t) >= FRAME_ADJUST_INTERVAL
        });
        let next = if delay >= 200 {
            self.healthy_since = None;
            let since = *self.congested_since.get_or_insert(now);
            if can_decrease && (stalled || now.saturating_duration_since(since) >= FRAME_ADJUST_INTERVAL) {
                if delay >= 500 { (current / 2).max(5).min(current) }
                else { (current * 4 / 5).max(10).min(current) }
            } else { current }
        } else if delay < 100 && !stalled {
            self.congested_since = None;
            let since = *self.healthy_since.get_or_insert(now);
            if can_adjust && now.saturating_duration_since(since) >= HEALTHY_HOLD {
                // Recover in seconds rather than feedback counts, including VPN.
                current + (ceiling / 4).max(5)
            } else { current }
        } else {
            self.healthy_since = None;
            self.congested_since = None;
            current
        };
        let next = next.clamp(MIN_FPS, ceiling);
        if next != current { self.last_adjusted = Some(now); }
        if next < current { self.last_decreased = Some(now); }
        self.fps = Some(next);
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
    congested_since: Option<Instant>,
    clear_since: Option<Instant>,
    last_resolution_feedback: Option<Instant>,
    pub capture_ms: u32,
    pub encode_ms: u32,
    pub send_ms: u32,
    pub stream_size: (u32, u32),
    pub(super) performance: super::video_performance::VideoPerformance,
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
            congested_since: None,
            clear_since: None,
            last_resolution_feedback: None,
            capture_ms: 0,
            encode_ms: 0,
            send_ms: 0,
            stream_size: (0, 0),
            performance: Default::default(),
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
            self.congested_since = None;
            self.clear_since = None;
            self.last_resolution_feedback = None;
        }
        self.adjust_fps();
    }

    pub fn resolution_divisor(&self) -> u32 {
        if self.record() { 1 } else { self.scale }
    }

    fn update_resolution(&mut self, now: Instant) {
        if self.profile != "latency" || !self.adaptive_resolution { return; }
        let delay = self.users.values().map(|u| u.delay.avg_delay()).max().unwrap_or(0);
        let max_gap = self.users.values().map(|u| u.delay.feedback_max_gap()).max().unwrap_or(FEEDBACK_MAX_GAP);
        if self.last_resolution_feedback.map_or(false, |t| now.saturating_duration_since(t) > max_gap) {
            self.congested_since = None;
            self.clear_since = None;
        }
        self.last_resolution_feedback = Some(now);
        if delay >= 250 {
            self.congested_since.get_or_insert(now);
            self.clear_since = None;
        } else if delay < 80 {
            self.clear_since.get_or_insert(now);
            self.congested_since = None;
        } else {
            self.congested_since = None;
            self.clear_since = None;
        }
        if self.profile == "latency" && self.adaptive_resolution {
            if self.congested_since.map_or(false, |t| now.saturating_duration_since(t) >= Duration::from_secs(3)) {
                self.scale = (self.scale * 2).min(4);
                self.congested_since = Some(now);
                self.clear_since = None;
            } else if self.clear_since.map_or(false, |t| now.saturating_duration_since(t) >= Duration::from_secs(15)) {
                self.scale = (self.scale / 2).max(1);
                self.clear_since = Some(now);
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
        self.user_network_delay_at(id, delay, Instant::now());
    }

    fn user_network_delay_at(&mut self, id: i32, delay: u32, now: Instant) {
        let highest_fps = self.highest_fps();
        if let Some(user) = self.users.get_mut(&id) {
            let delay = delay.max(10);
            user.delay.add_delay(delay);
            user.delay.adjust_frame_budget(highest_fps, now, false);
        }
        self.adjust_fps_at(now);
        self.update_resolution(now);
    }

    pub fn user_delay_response_elapsed(&mut self, id: i32, elapsed: u128) {
        self.user_delay_response_elapsed_at(id, elapsed, Instant::now());
    }

    fn user_delay_response_elapsed_at(&mut self, id: i32, elapsed: u128, now: Instant) {
        if elapsed < 1000 { return; }
        let highest_fps = self.highest_fps();
        if let Some(user) = self.users.get_mut(&id) {
            // An outstanding probe is a lower bound, not a completed RTT
            // sample. Do not let a stalled connection redefine baseline RTT.
            user.delay.network.stalled(elapsed);
            // Waiting within the known RTT budget is not congestion and must
            // not interrupt recovery between otherwise healthy responses.
            if !user.delay.network.congested() { return; }
            user.delay.adjust_frame_budget(highest_fps, now, true);
        }
        self.adjust_fps_at(now);
        self.update_resolution(now);
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
        self.adjust_fps_at(Instant::now());
    }

    fn adjust_fps_at(&mut self, now: Instant) {
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
        if now.saturating_duration_since(self.new_user_instant) < Duration::from_secs(1) {
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
    pub fn congested(&self, id: i32) -> bool {
        self.peers.get(&id).map_or(false, |q| q.users.values().any(|u| u.delay.network.congested()))
    }
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

#[cfg(test)]
#[path = "weak_network_tests.rs"]
mod weak_network_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn active_qos() -> VideoQoS {
        let mut qos = VideoQoS::default();
        qos.on_connection_open(1);
        qos.new_display("monitor_0".into());
        qos.set_support_changing_quality("monitor_0", true);
        qos
    }

    fn feed(qos: &mut VideoQoS, id: i32, delay: u32, count: usize, now: &mut Instant) {
        for _ in 0..count {
            *now += Duration::from_secs(1);
            qos.user_network_delay_at(id, delay, *now);
        }
    }

    #[test]
    fn slow_viewer_and_its_quality_changes_do_not_throttle_healthy_viewer() {
        let mut registry = VideoQoSRegistry::default();
        let start = Instant::now();
        for id in [1, 2] {
            let q = registry.peer(id);
            q.new_display("monitor0".into());
            q.set_support_changing_quality("monitor0", true);
            for second in 0..10 {
                q.user_network_delay_at(id, 10, start + Duration::from_secs(second));
            }
        }
        for second in 10..50 {
            let now = start + Duration::from_secs(second);
            registry.peer(1).user_network_delay_at(1, 10, now);
            registry.peer(2).user_network_delay_at(2, 1000, now);
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
        let mut now = q.new_user_instant;
        q.set_profile("latency", Some(true));
        feed(&mut q, 1, 10, 5, &mut now);
        feed(&mut q, 1, 800, 1, &mut now);
        assert_eq!(q.resolution_divisor(), 1);
        feed(&mut q, 1, 800, 7, &mut now);
        assert_eq!(q.resolution_divisor(), 4);
        feed(&mut q, 1, 10, 1, &mut now);
        assert_eq!(q.resolution_divisor(), 4);
        feed(&mut q, 1, 10, 32, &mut now);
        assert_eq!(q.resolution_divisor(), 1);
        q.set_profile("text", Some(true));
        feed(&mut q, 1, 800, 8, &mut now);
        assert_eq!(q.resolution_divisor(), 1);
        q.set_profile("latency", Some(false));
        feed(&mut q, 1, 800, 8, &mut now);
        assert_eq!(q.resolution_divisor(), 1);
    }

    #[test]
    fn startup_limits_burst_and_healthy_link_recovers() {
        let mut qos = active_qos();
        let start = qos.new_user_instant;
        assert_eq!(qos.fps(), 30);
        assert_eq!(qos.ratio(), BR_BALANCED);
        qos.user_network_delay_at(1, 10, start);
        qos.user_network_delay_at(1, 10, start + Duration::from_millis(500));
        assert_eq!(qos.fps(), 30);
        qos.user_network_delay_at(1, 10, start + Duration::from_secs(1));
        qos.user_network_delay_at(1, 10, start + Duration::from_secs(2));
        assert_eq!(qos.fps(), 45);
        qos.user_network_delay_at(1, 10, start + Duration::from_secs(3));
        assert_eq!(qos.fps(), 60);
        for _ in 0..20 { qos.adjust_ratio(true); }
        assert_eq!(qos.ratio(), LAN_DEFAULT_BITRATE_RATIO);
    }

    #[test]
    fn an_isolated_spike_preserves_fps_bitrate_and_resolution() {
        let mut qos = active_qos();
        qos.set_profile("latency", Some(true));
        let mut now = qos.new_user_instant;
        feed(&mut qos, 1, 20, 6, &mut now);
        let ratio = qos.ratio();
        feed(&mut qos, 1, 900, 1, &mut now);
        qos.adjust_ratio(false);
        assert_eq!(qos.fps(), 60);
        assert_eq!(qos.ratio(), ratio);
        assert_eq!(qos.resolution_divisor(), 1);
        feed(&mut qos, 1, 20, 3, &mut now);
        assert_eq!(qos.fps(), 60);
    }

    #[test]
    fn congestion_reduces_bitrate_and_frame_budget_then_recovers_gradually() {
        let mut qos = active_qos();
        let mut now = qos.new_user_instant;
        feed(&mut qos, 1, 20, 6, &mut now);
        let initial_ratio = qos.ratio();
        for _ in 0..5 {
            feed(&mut qos, 1, 800, 1, &mut now);
            qos.adjust_ratio(true);
        }
        assert!(qos.fps() <= 10);
        assert!(qos.ratio() < initial_ratio / 2.0);
        assert!(qos.ratio() >= BR_MIN_HIGH_RESOLUTION);
        feed(&mut qos, 1, 20, 2, &mut now);
        assert!(qos.fps() <= 10); // Do not rebound on the first filtered good sample.
        feed(&mut qos, 1, 20, 5, &mut now);
        assert_eq!(qos.fps(), 60);
        for _ in 0..40 { qos.adjust_ratio(true); }
        assert!(qos.ratio() > initial_ratio);
        assert!(qos.ratio() <= qos.latest_quality().ratio());
    }

    #[test]
    fn stable_vpn_rtt_is_not_mistaken_for_queued_video_from_first_sample() {
        let mut qos = active_qos();
        let start = qos.new_user_instant;
        for second in 0..4 {
            qos.user_network_delay_at(1, 250, start + Duration::from_secs(second));
            assert_eq!(qos.users[&1].delay.avg_delay(), 0);
            assert!(qos.fps() >= INIT_FPS);
        }
        assert_eq!(qos.fps(), 60);
    }

    #[test]
    fn a_normal_pending_probe_on_a_high_rtt_path_does_not_block_recovery() {
        let mut qos = active_qos();
        let start = qos.new_user_instant;
        qos.set_profile("latency", Some(true));
        qos.scale = 2;
        for probe in 0..10 {
            let response = start + Duration::from_millis(1200 + probe * 2001);
            qos.user_network_delay_at(1, 1200, response);
            qos.user_delay_response_elapsed_at(1, 1000, response + Duration::from_millis(1800));
        }
        assert_eq!(qos.fps(), 60);
        assert_eq!(qos.resolution_divisor(), 1);
        assert_eq!(qos.users[&1].delay.network.baseline_rtt(), Some(1200));
        qos.user_delay_response_elapsed_at(1, 4000, start + Duration::from_secs(24));
        assert_eq!(qos.fps(), 30, "real stalls on the same path must still throttle");
    }

    #[test]
    fn enabling_adaptive_resolution_starts_a_fresh_observation_window() {
        let mut qos = active_qos();
        let mut now = qos.new_user_instant;
        feed(&mut qos, 1, 20, 5, &mut now);
        feed(&mut qos, 1, 800, 10, &mut now);
        qos.set_profile("latency", Some(true));
        feed(&mut qos, 1, 800, 1, &mut now);
        assert_eq!(qos.resolution_divisor(), 1);
        feed(&mut qos, 1, 800, 3, &mut now);
        assert_eq!(qos.resolution_divisor(), 2);
    }

    #[test]
    fn missing_response_throttles_immediately_without_poisoning_rtt_baseline() {
        let mut qos = active_qos();
        let start = qos.new_user_instant;
        for second in 0..4 {
            qos.user_network_delay_at(1, 20, start + Duration::from_secs(second));
        }
        assert_eq!(qos.fps(), 60);
        let now = start + Duration::from_millis(3200);
        qos.user_delay_response_elapsed_at(1, 3000, now);
        qos.adjust_ratio(true);
        assert_eq!(qos.fps(), 30);
        assert!(qos.ratio() < BR_BALANCED);
        assert_eq!(qos.users[&1].delay.network.baseline_rtt(), Some(20));
        // Pending send and pending probe feedback in one timer tick cannot halve twice.
        qos.user_delay_response_elapsed_at(1, 4000, now);
        assert_eq!(qos.fps(), 30);
        qos.user_delay_response_elapsed_at(1, 5000, now + Duration::from_secs(1));
        assert_eq!(qos.fps(), 15);
    }

    #[test]
    fn recovery_depends_on_elapsed_time_not_feedback_count() {
        for step_ms in [250, 1000] {
            let mut qos = active_qos();
            let start = qos.new_user_instant;
            let steps = 3000 / step_ms;
            for tick in 0..=steps {
                qos.user_network_delay_at(1, 20, start + Duration::from_millis(tick * step_ms));
            }
            assert_eq!(qos.fps(), 60);
        }
        let mut qos = active_qos();
        let start = qos.new_user_instant;
        for _ in 0..100 { qos.user_network_delay_at(1, 20, start); }
        assert_eq!(qos.fps(), 30, "a burst of replies cannot fast-forward recovery");
        qos.user_network_delay_at(1, 20, start + Duration::from_secs(60));
        assert_eq!(qos.fps(), 30, "silence is not a healthy observation window");
    }

    #[test]
    fn resolution_hysteresis_depends_on_elapsed_time() {
        for step_ms in [250, 1000] {
            let mut qos = active_qos();
            qos.set_profile("latency", Some(true));
            let start = qos.new_user_instant;
            qos.user_network_delay_at(1, 20, start);
            qos.user_network_delay_at(1, 800, start + Duration::from_millis(step_ms));
            let congested_at = start + Duration::from_millis(2 * step_ms);
            for tick in 0..=3000 / step_ms {
                qos.user_network_delay_at(1, 800, congested_at + Duration::from_millis(tick * step_ms));
            }
            assert_eq!(qos.resolution_divisor(), 2);
        }
    }

    #[test]
    fn decoder_user_and_profile_caps_survive_recovery() {
        for (profile, cap) in [("text", 30), ("quality", 45), ("latency", 80)] {
            let mut qos = active_qos();
            qos.set_profile(profile, None);
            qos.user_custom_fps(1, 80);
            let mut now = qos.new_user_instant;
            feed(&mut qos, 1, 20, 15, &mut now);
            assert_eq!(qos.fps(), cap);
            qos.user_auto_adjust_fps(1, 12);
            feed(&mut qos, 1, 20, 15, &mut now);
            assert_eq!(qos.fps(), 12);
        }
        let mut qos = active_qos();
        qos.on_connection_open(2);
        qos.user_custom_fps(1, 20);
        qos.user_auto_adjust_fps(2, 12);
        qos.user_image_quality(1, ImageQuality::Low.value());
        let start = qos.new_user_instant;
        for second in 0..20 {
            let now = start + Duration::from_secs(second);
            qos.user_network_delay_at(1, 10, now);
            qos.user_network_delay_at(2, 10, now);
            qos.adjust_ratio(true);
        }
        assert_eq!(qos.fps(), 12);
        assert!(qos.ratio() <= BR_SPEED);
        let mut now = start + Duration::from_secs(20);
        feed(&mut qos, 2, 1000, 4, &mut now);
        assert!(qos.fps() < 12);
        qos.on_connection_close(2);
        feed(&mut qos, 1, 10, 10, &mut now);
        assert_eq!(qos.fps(), 20);
    }

    #[test]
    fn unsupported_encoder_still_adapts_fps_without_quality_restarts() {
        let mut qos = active_qos();
        qos.set_support_changing_quality("monitor_0", false);
        let mut now = qos.new_user_instant;
        feed(&mut qos, 1, 20, 6, &mut now);
        let ratio = qos.ratio();
        feed(&mut qos, 1, 900, 5, &mut now);
        qos.adjust_ratio(true);
        assert!(!qos.in_vbr_state());
        assert_eq!(qos.ratio(), ratio);
        assert!(qos.fps() < INIT_FPS);
    }
}
