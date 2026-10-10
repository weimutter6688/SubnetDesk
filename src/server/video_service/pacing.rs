use super::*;
use std::thread::Thread;

#[derive(Default)]
pub(super) struct Pacer {
    started: Option<Instant>,
}

impl Pacer {
    pub fn start(&mut self, now: Instant) {
        self.started = Some(now);
    }

    pub fn remaining(&self, now: Instant, spf: Duration) -> Duration {
        self.started.map_or(Duration::ZERO, |started| {
            spf.saturating_sub(now.saturating_duration_since(started))
        })
    }
}

lazy_static::lazy_static! {
    static ref WAITERS: Mutex<Vec<(usize, Thread)>> = Default::default();
}

pub(super) struct FrameWaiter(usize, Thread);

impl FrameWaiter {
    pub fn new(display: usize) -> Self {
        let thread = std::thread::current();
        WAITERS.lock().unwrap().push((display, thread.clone()));
        Self(display, thread)
    }

    pub fn notify(display: usize) {
        for (_, thread) in WAITERS
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, _)| *id == display)
        {
            thread.unpark();
        }
    }
}

impl Drop for FrameWaiter {
    fn drop(&mut self) {
        WAITERS
            .lock()
            .unwrap()
            .retain(|(display, thread)| *display != self.0 || thread.id() != self.1.id());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_time_is_inside_the_frame_budget_without_catch_up_bursts() {
        let mut pacer = Pacer::default();
        let mut now = Instant::now();
        let beginning = now;
        let spf = Duration::from_secs_f64(1.0 / 60.0);
        for _ in 0..60 {
            pacer.start(now);
            now += Duration::from_millis(8);
            now += pacer.remaining(now, spf);
        }
        assert!(
            now.duration_since(beginning)
                .abs_diff(Duration::from_secs(1))
                < Duration::from_micros(1)
        );
        pacer.start(now);
        now += Duration::from_millis(100);
        assert_eq!(pacer.remaining(now, spf), Duration::ZERO);
        pacer.start(now);
        assert_eq!(pacer.remaining(now, spf), spf);
        assert_eq!(
            pacer.remaining(now + Duration::from_millis(20), Duration::from_millis(50)),
            Duration::from_millis(30)
        );
    }

    #[test]
    fn send_completion_wakeup_is_not_lost_before_waiting() {
        let waiter = FrameWaiter::new(usize::MAX - 3);
        FrameWaiter::notify(waiter.0);
        let start = Instant::now();
        std::thread::park_timeout(Duration::from_secs(2));
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
