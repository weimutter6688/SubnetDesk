use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const MAX_POLL_GAP: Duration = Duration::from_secs(5);
const STALL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Idle,
    Starting,
    Preparing,
    Capturing,
    Encoding,
    Releasing,
    Panicked,
}

#[derive(Clone, Copy)]
struct Progress {
    revision: u64,
    phase: Phase,
}

#[derive(Clone)]
pub(super) struct Watchdog(Arc<Mutex<Progress>>);

impl Watchdog {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Progress {
            revision: 0,
            phase: Phase::Idle,
        })))
    }

    pub fn checkpoint(&self, phase: Phase) {
        let mut progress = self.0.lock().unwrap();
        progress.revision = progress.revision.wrapping_add(1);
        progress.phase = phase;
    }

    pub fn begin(&self) -> Activity {
        self.checkpoint(Phase::Starting);
        Activity(self.clone())
    }

    pub fn cleanup_guard(&self) -> Cleanup {
        Cleanup(self.clone())
    }

    // Native capture/codec calls cannot be cancelled by a Rust timeout. Only
    // recover a Windows --server that has a running service to respawn it.
    #[cfg(windows)]
    pub fn start_recovery_monitor(&self, name: String) {
        if !crate::is_server()
            || !crate::platform::is_root()
            || !crate::platform::is_installed()
            || !crate::platform::is_self_service_running()
        {
            return;
        }
        let progress = self.0.clone();
        if let Err(err) = std::thread::Builder::new()
            .name(format!("{name}-watchdog"))
            .spawn(move || {
                let mut detector = StallDetector::new(Instant::now());
                loop {
                    std::thread::sleep(POLL_INTERVAL);
                    // Do not acquire service, codec or capture locks here: the
                    // worker may be blocked while holding one of those locks.
                    let state = *progress.lock().unwrap();
                    if state.phase == Phase::Idle && Arc::strong_count(&progress) == 1 {
                        break;
                    }
                    if let Some(phase) = detector.poll(Instant::now(), state) {
                        if crate::platform::is_self_service_running() {
                            hbb_common::log::error!(
                                "Video service {name} stopped making progress in {phase:?}; restarting supervised server"
                            );
                            if progress.lock().unwrap().revision != state.revision {
                                continue;
                            }
                            // ExitProcess can itself deadlock in a driver's DLL
                            // teardown. Terminate only this supervised child;
                            // --service observes its exit and starts --server.
                            let terminated = unsafe {
                                winapi::um::processthreadsapi::TerminateProcess(
                                    winapi::um::processthreadsapi::GetCurrentProcess(),
                                    1,
                                )
                            };
                            if terminated == 0 {
                                hbb_common::log::error!(
                                    "Failed to terminate stalled video server: {}",
                                    std::io::Error::last_os_error()
                                );
                            }
                            break;
                        }
                        hbb_common::log::error!(
                            "Video service {name} stalled in {phase:?}, but its supervisor is not running"
                        );
                        break;
                    }
                }
            })
        {
            hbb_common::log::error!("Failed to start video recovery monitor: {err}");
        }
    }
}

// This guard belongs to the caller of run_inner, so it becomes Idle only after
// all capture/encoder destructors have returned, including error paths.
pub(super) struct Activity(Watchdog);

impl Drop for Activity {
    fn drop(&mut self) {
        self.0.checkpoint(if std::thread::panicking() {
            Phase::Panicked
        } else {
            Phase::Idle
        });
    }
}

// Declare after native resources so this runs before their destructors.
pub(super) struct Cleanup(Watchdog);

impl Drop for Cleanup {
    fn drop(&mut self) {
        self.0.checkpoint(Phase::Releasing);
    }
}

struct StallDetector {
    revision: Option<u64>,
    last_progress: Instant,
    last_poll: Instant,
}

impl StallDetector {
    fn new(now: Instant) -> Self {
        Self {
            revision: None,
            last_progress: now,
            last_poll: now,
        }
    }

    fn poll(&mut self, now: Instant, progress: Progress) -> Option<Phase> {
        // Sleeping/suspended computers can advance Instant without either
        // thread running. Give the worker a full grace period after resume.
        let scheduling_gap = now.saturating_duration_since(self.last_poll) > MAX_POLL_GAP;
        self.last_poll = now;
        if self.revision != Some(progress.revision)
            || progress.phase == Phase::Idle
            || scheduling_gap
        {
            self.revision = Some(progress.revision);
            self.last_progress = now;
        }
        if progress.phase == Phase::Panicked
            || (progress.phase != Phase::Idle
                && now.saturating_duration_since(self.last_progress) >= STALL_TIMEOUT)
        {
            Some(progress.phase)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poll(detector: &mut StallDetector, watchdog: &Watchdog, now: Instant) -> Option<Phase> {
        detector.poll(now, *watchdog.0.lock().unwrap())
    }

    #[test]
    fn idle_reconnect_and_static_desktop_do_not_trigger_recovery() {
        let watchdog = Watchdog::new();
        let start = Instant::now();
        let mut detector = StallDetector::new(start);
        {
            let _activity = watchdog.begin();
            watchdog.checkpoint(Phase::Capturing);
            assert!(poll(&mut detector, &watchdog, start).is_none());
        }
        let reconnect = start + Duration::from_secs(12 * 3600);
        for second in 1..120 {
            assert!(poll(
                &mut detector,
                &watchdog,
                start + Duration::from_secs(second)
            )
            .is_none());
        }
        assert!(poll(&mut detector, &watchdog, reconnect).is_none());
        let _activity = watchdog.begin();
        for second in 0..120 {
            // A healthy static desktop still polls capture, even with no frames.
            watchdog.checkpoint(Phase::Capturing);
            assert!(poll(
                &mut detector,
                &watchdog,
                reconnect + Duration::from_secs(second)
            )
            .is_none());
        }
    }

    #[test]
    fn blocked_native_cleanup_is_detected_until_it_finishes() {
        struct NativeResource {
            entered: std::sync::mpsc::Sender<()>,
            release: std::sync::mpsc::Receiver<()>,
        }
        impl Drop for NativeResource {
            fn drop(&mut self) {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            }
        }
        let watchdog = Watchdog::new();
        let worker_watchdog = watchdog.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _activity = worker_watchdog.begin();
            let _resource = NativeResource {
                entered: entered_tx,
                release: release_rx,
            };
            let _cleanup = worker_watchdog.cleanup_guard();
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let start = Instant::now();
        let mut detector = StallDetector::new(start);
        let mut detected = None;
        for second in 0..=30 {
            detected = poll(
                &mut detector,
                &watchdog,
                start + Duration::from_secs(second),
            );
        }
        release_tx.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(detected, Some(Phase::Releasing));
        assert!(poll(&mut detector, &watchdog, start + Duration::from_secs(31)).is_none());
    }

    #[test]
    fn resume_and_new_progress_reset_the_stall_deadline() {
        let watchdog = Watchdog::new();
        let _activity = watchdog.begin();
        watchdog.checkpoint(Phase::Encoding);
        let start = Instant::now();
        let mut detector = StallDetector::new(start);
        for second in 0..20 {
            assert!(poll(
                &mut detector,
                &watchdog,
                start + Duration::from_secs(second)
            )
            .is_none());
        }
        let resume = start + Duration::from_secs(3600);
        for second in 0..30 {
            assert!(poll(
                &mut detector,
                &watchdog,
                resume + Duration::from_secs(second)
            )
            .is_none());
        }
        watchdog.checkpoint(Phase::Preparing);
        for second in 30..60 {
            assert!(poll(
                &mut detector,
                &watchdog,
                resume + Duration::from_secs(second)
            )
            .is_none());
        }
        assert_eq!(
            poll(&mut detector, &watchdog, resume + Duration::from_secs(60)),
            Some(Phase::Preparing)
        );
    }

    #[test]
    fn worker_panic_is_not_reported_as_healthy_idle() {
        let watchdog = Watchdog::new();
        let result = std::panic::catch_unwind(|| {
            let _activity = watchdog.begin();
            let _cleanup = watchdog.cleanup_guard();
            panic!("simulated video worker failure");
        });
        assert!(result.is_err());
        let now = Instant::now();
        assert_eq!(
            poll(&mut StallDetector::new(now), &watchdog, now),
            Some(Phase::Panicked)
        );
    }
}
