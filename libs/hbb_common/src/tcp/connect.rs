use crate::{
    anyhow::anyhow,
    futures::{stream::FuturesUnordered, StreamExt},
    ResultType,
};
use std::{
    collections::{HashSet, VecDeque},
    future::Future,
    net::SocketAddr,
    time::Duration,
};
use tokio::time::Instant;

const ADDRESS_DELAY: Duration = Duration::from_millis(80);

// Preserve resolver preference within each family, but give the other family
// a chance before exhausting a list of stale IPv6 or IPv4 routes.
fn ordered_addresses(addresses: Vec<SocketAddr>) -> VecDeque<SocketAddr> {
    let first_ipv4 = addresses.first().map_or(true, SocketAddr::is_ipv4);
    let mut seen = HashSet::new();
    let (mut preferred, mut alternate): (VecDeque<_>, VecDeque<_>) = addresses
        .into_iter()
        .filter(|address| seen.insert(*address))
        .partition(|address| address.is_ipv4() == first_ipv4);
    let mut ordered = VecDeque::new();
    while !preferred.is_empty() || !alternate.is_empty() {
        if let Some(address) = preferred.pop_front() {
            ordered.push_back(address);
        }
        if let Some(address) = alternate.pop_front() {
            ordered.push_back(address);
        }
    }
    ordered
}

pub(super) async fn resolve_and_connect<T, R, F, Fut>(
    resolve: R,
    ms_timeout: u64,
    connect: F,
) -> ResultType<T>
where
    R: Future<Output = ResultType<Vec<SocketAddr>>>,
    F: Fn(SocketAddr) -> Fut,
    Fut: Future<Output = ResultType<T>>,
{
    // One budget covers DNS and every address. Dropping this future closes
    // pending sockets, including the losing attempt after a successful race.
    crate::timeout(ms_timeout, async {
        let mut addresses = ordered_addresses(resolve.await?);
        if addresses.is_empty() {
            return Err(anyhow!("DNS resolution returned no compatible address"));
        }
        let mut pending = FuturesUnordered::new();
        let mut next_launch = Instant::now();
        let mut last_error = anyhow!("route unavailable");
        loop {
            if pending.is_empty() && addresses.is_empty() {
                return Err(last_error);
            }
            tokio::select! {
                result = pending.next(), if !pending.is_empty() => {
                    match result {
                        Some(Ok(stream)) => return Ok(stream),
                        Some(Err(err)) => last_error = err,
                        None => {}
                    }
                    next_launch = Instant::now();
                }
                _ = tokio::time::sleep_until(next_launch), if pending.len() < 2 && !addresses.is_empty() => {
                    if let Some(address) = addresses.pop_front() {
                        pending.push(connect(address));
                        next_launch = Instant::now() + ADDRESS_DELAY;
                    }
                }
            }
        }
    })
    .await
    .map_err(|_| anyhow!("connection timed out after {ms_timeout} ms (including DNS)"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct Active(Arc<AtomicUsize>);
    impl Drop for Active {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn resolver_order_is_interleaved_and_deduplicated() {
        let addresses: Vec<SocketAddr> = [
            "[::1]:1",
            "[::1]:2",
            "127.0.0.1:1",
            "[::1]:1",
            "127.0.0.1:2",
        ]
        .iter()
        .map(|address| address.parse().unwrap())
        .collect();
        let ordered: Vec<_> = ordered_addresses(addresses)
            .into_iter()
            .map(|a| a.to_string())
            .collect();
        assert_eq!(
            ordered,
            ["[::1]:1", "127.0.0.1:1", "[::1]:2", "127.0.0.1:2"]
        );
    }

    #[tokio::test]
    async fn stalled_dns_expires_before_any_connection_is_launched() {
        let attempts = AtomicUsize::new(0);
        let result = resolve_and_connect(std::future::pending(), 20, |_| {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Ok(()) }
        })
        .await;
        assert!(result.unwrap_err().to_string().contains("including DNS"));
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn unreachable_first_family_does_not_delay_the_other_and_loser_is_dropped() {
        let active = Arc::new(AtomicUsize::new(0));
        let addresses = vec![
            "[::1]:1".parse().unwrap(),
            "[::1]:2".parse().unwrap(),
            "127.0.0.1:1".parse().unwrap(),
        ];
        let result = resolve_and_connect(async { Ok(addresses) }, 2000, |address| {
            let active = active.clone();
            async move {
                assert!(active.fetch_add(1, Ordering::SeqCst) < 2);
                let _guard = Active(active);
                if address.is_ipv6() {
                    std::future::pending::<()>().await;
                }
                Ok(address)
            }
        })
        .await
        .unwrap();
        assert!(result.is_ipv4());
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn all_addresses_share_one_timeout_and_at_most_two_connections() {
        let active = Arc::new(AtomicUsize::new(0));
        let attempts = AtomicUsize::new(0);
        let addresses = vec![
            "127.0.0.1:1".parse().unwrap(),
            "127.0.0.1:2".parse().unwrap(),
            "127.0.0.1:3".parse().unwrap(),
        ];
        let result = resolve_and_connect(async { Ok(addresses) }, 200, |_| {
            attempts.fetch_add(1, Ordering::SeqCst);
            let active = active.clone();
            async move {
                assert!(active.fetch_add(1, Ordering::SeqCst) < 2);
                let _guard = Active(active);
                std::future::pending::<ResultType<()>>().await
            }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn immediate_failures_try_the_remaining_addresses() {
        let addresses = vec![
            "127.0.0.1:1".parse().unwrap(),
            "127.0.0.1:2".parse().unwrap(),
        ];
        let result = resolve_and_connect(async { Ok(addresses) }, 1000, |address| async move {
            if address.port() == 1 {
                return Err(anyhow!("refused"));
            }
            Ok(address)
        })
        .await
        .unwrap();
        assert_eq!(result.port(), 2);
        let result = resolve_and_connect(async { Ok(vec![]) }, 1000, |_| async { Ok(()) }).await;
        assert!(result.is_err());
    }
}
