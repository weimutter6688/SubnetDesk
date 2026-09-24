use hbb_common::message_proto::VideoFrame;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub const MAX_FRAMES: usize = 6;
const MAX_AGE: Duration = Duration::from_millis(200);

#[derive(Default)]
pub struct VideoQueue {
    frames: VecDeque<(Instant, VideoFrame)>,
    waiting_key: bool,
    notified: bool,
    last_refresh: Option<Instant>,
}

impl VideoQueue {
    pub fn len(&self) -> usize {
        self.frames.len()
    }
    pub fn capacity(&self) -> usize {
        MAX_FRAMES
    }
    pub fn reset(&mut self) {
        self.frames.clear();
        self.waiting_key = true;
        self.last_refresh = Some(Instant::now());
    }
    pub fn wake_consumed(&mut self) {
        self.notified = false;
    }

    pub fn request_due(&mut self, now: Instant) -> bool {
        if self.waiting_key
            && self.last_refresh.map_or(true, |t| {
                now.saturating_duration_since(t) >= Duration::from_secs(1)
            })
        {
            self.last_refresh = Some(now);
            true
        } else {
            false
        }
    }

    fn lose_chain(&mut self, now: Instant) -> bool {
        self.frames.clear();
        self.waiting_key = true;
        self.request_due(now)
    }

    /// Returns (wake decoder, request keyframe). Never retain orphaned deltas.
    pub fn push(&mut self, frame: VideoFrame, key: bool, now: Instant) -> (bool, bool) {
        if key {
            self.frames.clear();
            self.waiting_key = false;
        } else if self.waiting_key {
            return (false, self.request_due(now));
        } else if self.frames.len() >= MAX_FRAMES
            || self
                .frames
                .front()
                .map_or(false, |(t, _)| now.saturating_duration_since(*t) > MAX_AGE)
        {
            return (false, self.lose_chain(now));
        }
        self.frames.push_back((now, frame));
        let wake = !self.notified;
        self.notified = true;
        (wake, false)
    }

    pub fn pop(&mut self, now: Instant) -> (Option<(VideoFrame, u32)>, bool) {
        if self
            .frames
            .front()
            .map_or(false, |(t, _)| now.saturating_duration_since(*t) > MAX_AGE)
        {
            return (None, self.lose_chain(now));
        }
        (
            self.frames
                .pop_front()
                .map(|(t, f)| (f, now.saturating_duration_since(t).as_millis() as u32)),
            false,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overflow_discards_dependency_chain_until_keyframe() {
        let mut q = VideoQueue::default();
        let t = Instant::now();
        assert_eq!(q.push(VideoFrame::new(), true, t), (true, false));
        for _ in 1..MAX_FRAMES {
            assert_eq!(q.push(VideoFrame::new(), false, t), (false, false));
        }
        assert_eq!(q.push(VideoFrame::new(), false, t), (false, true));
        assert_eq!(q.len(), 0);
        assert_eq!(q.push(VideoFrame::new(), false, t), (false, false));
        q.wake_consumed();
        assert_eq!(q.push(VideoFrame::new(), true, t), (true, false));
        assert!(q.pop(t).0.is_some());
    }
    #[test]
    fn stale_decode_is_skipped_and_requests_one_refresh() {
        let mut q = VideoQueue::default();
        let t = Instant::now();
        q.push(VideoFrame::new(), true, t);
        assert!(q.pop(t + Duration::from_millis(201)).1);
        assert!(!q.pop(t + Duration::from_secs(1)).1);
        assert_eq!(q.push(VideoFrame::new(), false, t), (false, false));
    }
    #[test]
    fn repeated_keyframes_keep_one_wakeup_and_one_frame() {
        let mut q = VideoQueue::default();
        let t = Instant::now();
        let mut wakes = 0;
        for _ in 0..1000 {
            wakes += q.push(VideoFrame::new(), true, t).0 as usize;
        }
        assert_eq!((wakes, q.len()), (1, 1));
    }

    #[test]
    fn refreshes_are_rate_limited_but_retry_on_a_static_desktop() {
        let mut q = VideoQueue::default();
        let t = Instant::now();
        q.push(VideoFrame::new(), true, t);
        assert!(q.pop(t + Duration::from_millis(201)).1);
        q.push(VideoFrame::new(), true, t + Duration::from_millis(210));
        assert!(!q.pop(t + Duration::from_millis(500)).1);
        assert!(!q.request_due(t + Duration::from_millis(1000)));
        assert!(q.request_due(t + Duration::from_millis(1201)));
    }
}
