use hbb_common::{
    anyhow::anyhow,
    futures::{stream::FuturesUnordered, StreamExt},
    tokio::{
        self,
        time::{Duration, Instant},
    },
    ResultType,
};
use std::{collections::VecDeque, future::Future};

// Prefer the requested address, but do not wait for a stale LAN/VPN route to
// time out before trying an alternative. Dropping pending futures closes losers.
pub(super) async fn race_candidates<T, F, Fut>(candidates: Vec<String>, connect: F) -> ResultType<T>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = ResultType<T>>,
{
    let mut candidates: VecDeque<_> = candidates.into();
    let mut pending = FuturesUnordered::new();
    let mut next_launch = Instant::now();
    let mut last_error = anyhow!("no usable LAN endpoint was found");
    loop {
        if pending.is_empty() && candidates.is_empty() {
            return Err(last_error);
        }
        tokio::select! {
            result = pending.next(), if !pending.is_empty() => {
                match result {
                    Some(Ok(value)) => return Ok(value),
                    Some(Err(err)) => last_error = err,
                    None => {}
                }
                next_launch = Instant::now();
            }
            _ = tokio::time::sleep_until(next_launch), if pending.len() < 2 && !candidates.is_empty() => {
                if let Some(candidate) = candidates.pop_front() {
                    pending.push(connect(candidate));
                    next_launch = Instant::now() + Duration::from_millis(250);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct InFlight(Arc<AtomicUsize>);
    impl Drop for InFlight {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn stale_address_does_not_delay_healthy_fallback_and_loser_is_cancelled() {
        let active = Arc::new(AtomicUsize::new(0));
        let start = Instant::now();
        let winner = race_candidates(vec!["stale".into(), "vpn".into()], |address| {
            let active = active.clone();
            async move {
                active.fetch_add(1, Ordering::SeqCst);
                let _guard = InFlight(active);
                if address == "stale" {
                    std::future::pending::<()>().await;
                }
                Ok(address)
            }
        });
        let winner = tokio::time::timeout(Duration::from_secs(2), winner)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(winner, "vpn");
        assert!(start.elapsed() >= Duration::from_millis(250));
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn fast_requested_address_wins_without_starting_fallback() {
        let attempts = AtomicUsize::new(0);
        let winner = race_candidates(vec!["lan".into(), "vpn".into()], |address| {
            attempts.fetch_add(1, Ordering::SeqCst);
            async move { Ok(address) }
        })
        .await
        .unwrap();
        assert_eq!(winner, "lan");
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failures_continue_with_at_most_two_attempts() {
        let active = Arc::new(AtomicUsize::new(0));
        let attempts = AtomicUsize::new(0);
        let result = race_candidates(vec!["a".into(), "b".into(), "c".into()], |address| {
            attempts.fetch_add(1, Ordering::SeqCst);
            let active = active.clone();
            async move {
                assert!(active.fetch_add(1, Ordering::SeqCst) < 2);
                let _guard = InFlight(active);
                tokio::time::sleep(Duration::from_millis(300)).await;
                Err::<(), _>(anyhow!("{address}: refused"))
            }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
}
