#![deny(warnings, clippy::all)]
// iOS has no LAN listener, so some helpers are unused there.
#![cfg_attr(target_os = "ios", allow(dead_code))]

//! Shared admission control for incoming connections: a global slot plus a
//! per-source slot, released together when the [`Admission`] ticket is dropped.
//! Used for unauthenticated LAN connections and for Web gateway sessions.

use std::{collections::HashMap, net::IpAddr, sync::Arc};

use hbb_common::tokio::sync::{OwnedSemaphorePermit, Semaphore};
use parking_lot::Mutex;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AdmitError {
    /// The global pool is exhausted.
    Global,
    /// This source already holds its maximum number of slots.
    PerSource,
}

impl std::fmt::Display for AdmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Global => f.write_str("too many pending connections"),
            Self::PerSource => f.write_str("too many pending connections from this source"),
        }
    }
}

impl std::error::Error for AdmitError {}

#[derive(Clone)]
pub struct AdmissionControl {
    global: Arc<Semaphore>,
    per_source: Arc<Mutex<HashMap<IpAddr, usize>>>,
    per_source_max: usize,
}

/// One admitted connection. Dropping it returns both slots.
pub struct Admission {
    control: AdmissionControl,
    source: IpAddr,
    _global: OwnedSemaphorePermit,
}

impl AdmissionControl {
    pub fn new(global_max: usize, per_source_max: usize) -> Self {
        Self {
            global: Arc::new(Semaphore::new(global_max)),
            per_source: Arc::new(Mutex::new(HashMap::new())),
            per_source_max,
        }
    }

    pub fn try_admit(&self, source: IpAddr) -> Result<Admission, AdmitError> {
        {
            let mut sources = self.per_source.lock();
            let count = sources.entry(source).or_insert(0);
            if *count >= self.per_source_max {
                if *count == 0 {
                    sources.remove(&source);
                }
                return Err(AdmitError::PerSource);
            }
            *count += 1;
        }
        match self.global.clone().try_acquire_owned() {
            Ok(global) => Ok(Admission {
                control: self.clone(),
                source,
                _global: global,
            }),
            Err(_) => {
                self.release(source);
                Err(AdmitError::Global)
            }
        }
    }

    fn release(&self, source: IpAddr) {
        let mut sources = self.per_source.lock();
        if let Some(count) = sources.get_mut(&source) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                sources.remove(&source);
            }
        }
    }

    #[cfg(test)]
    fn tracked_sources(&self) -> usize {
        self.per_source.lock().len()
    }
}

impl Admission {
    pub fn source(&self) -> IpAddr {
        self.source
    }
}

impl Drop for Admission {
    fn drop(&mut self) {
        self.control.release(self.source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(value: &str) -> IpAddr {
        value.parse().unwrap()
    }

    #[test]
    fn per_source_limit_is_enforced_and_released() {
        let control = AdmissionControl::new(10, 2);
        let first = control.try_admit(ip("192.0.2.1")).unwrap();
        let _second = control.try_admit(ip("192.0.2.1")).unwrap();
        assert_eq!(
            control.try_admit(ip("192.0.2.1")).err(),
            Some(AdmitError::PerSource)
        );
        assert!(control.try_admit(ip("192.0.2.2")).is_ok());
        drop(first);
        assert!(control.try_admit(ip("192.0.2.1")).is_ok());
    }

    #[test]
    fn global_limit_does_not_leak_per_source_slots() {
        let control = AdmissionControl::new(1, 4);
        let held = control.try_admit(ip("192.0.2.1")).unwrap();
        assert_eq!(
            control.try_admit(ip("192.0.2.9")).err(),
            Some(AdmitError::Global)
        );
        assert_eq!(control.tracked_sources(), 1);
        drop(held);
        assert_eq!(control.tracked_sources(), 0);
        assert!(control.try_admit(ip("192.0.2.9")).is_ok());
    }

    #[test]
    fn random_admit_and_release_sequences_keep_counts_consistent() {
        // Deterministic pseudo-random sequence (no extra test dependency).
        let control = AdmissionControl::new(5, 2);
        let sources = [ip("10.0.0.1"), ip("10.0.0.2"), ip("10.0.0.3")];
        let mut held: Vec<Admission> = Vec::new();
        let mut state = 0x2545_f491_u32;
        for _ in 0..2_000 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            if state % 3 == 0 && !held.is_empty() {
                let index = (state as usize / 3) % held.len();
                held.swap_remove(index);
            } else if let Ok(admission) =
                control.try_admit(sources[(state as usize / 7) % sources.len()])
            {
                held.push(admission);
            }
            assert!(held.len() <= 5);
            for source in sources {
                assert!(held.iter().filter(|a| a.source() == source).count() <= 2);
            }
        }
        held.clear();
        assert_eq!(control.tracked_sources(), 0);
    }
}
