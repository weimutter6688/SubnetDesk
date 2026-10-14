#![deny(warnings, clippy::all)]

use std::{
    net::{IpAddr, SocketAddr},
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
    time::Duration,
};

#[cfg(not(target_os = "ios"))]
use hbb_common::tokio_util::{sync::CancellationToken, task::TaskTracker};
use hbb_common::{
    config::{option2bool, Config},
    log,
    tcp::{listen_any, new_listener},
    tokio, ResultType, Stream,
};
use parking_lot::Mutex;

use crate::lan_config::normalize_source_ip;
#[cfg(not(target_os = "ios"))]
use crate::server::{new as new_server, ServerPtr};

/// Bound for listener tasks to exit after their token is cancelled.
#[cfg(not(target_os = "ios"))]
const LISTENER_STOP_TIMEOUT: Duration = Duration::from_secs(5);

static RESTART_GENERATION: AtomicU64 = AtomicU64::new(0);
static ACTIVE_LISTENERS: AtomicUsize = AtomicUsize::new(0);

pub const RUNTIME_STATE_STOPPED: &str = "stopped";
pub const RUNTIME_STATE_STARTING: &str = "starting";
pub const RUNTIME_STATE_LISTENING: &str = "listening";
pub const RUNTIME_STATE_FAILED: &str = "failed";

#[derive(Clone, Debug)]
pub struct LanServerRuntimeStatus {
    pub state: String,
    pub last_error: String,
}

lazy_static::lazy_static! {
    /// Every accepted LAN connection task, across listener restarts.
    #[cfg(not(target_os = "ios"))]
    static ref CONNECTION_TASKS: TaskTracker = TaskTracker::new();
    static ref RUNTIME_STATUS: Mutex<LanServerRuntimeStatus> = Mutex::new(LanServerRuntimeStatus {
        state: RUNTIME_STATE_STARTING.to_owned(),
        last_error: String::new(),
    });
}

fn set_runtime_status(state: &str, last_error: String) {
    let mut status = RUNTIME_STATUS.lock();
    status.state = state.to_owned();
    status.last_error = last_error;
}

pub struct LanServer;

impl LanServer {
    pub fn restart() {
        RESTART_GENERATION.fetch_add(1, Ordering::SeqCst);
        set_runtime_status(RUNTIME_STATE_STARTING, String::new());
        log::info!("LAN server restart requested");
    }

    pub fn is_running() -> bool {
        ACTIVE_LISTENERS.load(Ordering::SeqCst) > 0
    }

    pub fn runtime_status() -> LanServerRuntimeStatus {
        RUNTIME_STATUS.lock().clone()
    }

    pub(crate) fn is_discoverable() -> bool {
        discoverable_state(Self::is_running(), service_ready())
    }

    #[cfg(not(target_os = "ios"))]
    pub async fn start() {
        let server = new_server();
        start_auxiliary_services_once();
        loop {
            let generation = RESTART_GENERATION.load(Ordering::SeqCst);
            let signature = listener_signature();
            if !service_ready() {
                ACTIVE_LISTENERS.store(0, Ordering::SeqCst);
                set_runtime_status(RUNTIME_STATE_STOPPED, String::new());
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            let cancel = CancellationToken::new();
            let listeners = TaskTracker::new();
            let (handles, native_listener_count) =
                match bind_listeners(server.clone(), cancel.clone(), &listeners).await {
                    Ok(result) => result,
                    Err(err) => {
                        log::error!("Failed to start LAN server: {err}");
                        ACTIVE_LISTENERS.store(0, Ordering::SeqCst);
                        set_runtime_status(RUNTIME_STATE_FAILED, err.to_string());
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };
            ACTIVE_LISTENERS.store(native_listener_count, Ordering::SeqCst);
            set_runtime_status(RUNTIME_STATE_LISTENING, String::new());
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if generation != RESTART_GENERATION.load(Ordering::SeqCst)
                    || signature != listener_signature()
                    || !service_ready()
                {
                    break;
                }
            }
            // Stop accepting; already-accepted connections keep running and are
            // tracked separately by CONNECTION_TASKS.
            cancel.cancel();
            listeners.close();
            log::info!(
                "LAN listeners stopping; {} accepted connection task(s) keep running",
                Self::active_connection_tasks()
            );
            ACTIVE_LISTENERS.store(0, Ordering::SeqCst);
            set_runtime_status(
                if service_ready() {
                    RUNTIME_STATE_STARTING
                } else {
                    RUNTIME_STATE_STOPPED
                },
                String::new(),
            );
            if tokio::time::timeout(LISTENER_STOP_TIMEOUT, listeners.wait())
                .await
                .is_err()
            {
                log::warn!("LAN listeners did not stop within {LISTENER_STOP_TIMEOUT:?}");
            }
            for handle in handles {
                if let Err(err) = handle.await {
                    log::debug!("Web listener task ended: {err}");
                }
            }
        }
    }

    /// Accepted LAN connections that are still running (handshake, auth or session).
    #[cfg(not(target_os = "ios"))]
    pub fn active_connection_tasks() -> usize {
        CONNECTION_TASKS.len()
    }
}

pub fn normalize_listen_addresses(value: &str) -> ResultType<String> {
    crate::lan_config::parse_listen_addresses(value)
        .map(|addresses| {
            addresses
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        })
        .map_err(Into::into)
}

pub fn normalize_allowed_networks(value: &str) -> ResultType<String> {
    // The settings UI requires explicit prefixes; bare addresses are rejected here.
    if let Some(network) = value
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .map(str::trim)
        .find(|network| !network.is_empty() && !network.contains('/'))
    {
        hbb_common::anyhow::bail!("Invalid allowed network: {network}");
    }
    value
        .parse::<crate::lan_config::AllowedNetworks>()
        .map(|networks| networks.to_canonical_string())
        .map_err(Into::into)
}

fn service_ready() -> bool {
    Config::lan_credentials_configured()
        && !option2bool("stop-service", &Config::get_option("stop-service"))
}

fn discoverable_state(running: bool, ready: bool) -> bool {
    running && ready
}

fn listen_port() -> u16 {
    crate::lan_config::ListenPort::configured().get()
}

fn listen_addresses() -> Vec<String> {
    Config::get_option("lan-listen-addresses")
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[derive(PartialEq, Eq)]
struct ListenerSignature {
    port: u16,
    addresses: Vec<String>,
    policy: Vec<String>,
    certificate_addresses: Vec<String>,
}

fn listener_signature() -> ListenerSignature {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let web_certificate_addresses = if crate::web_gateway::is_enabled() {
        crate::web_gateway::certificate_address_signature()
    } else {
        Vec::new()
    };
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let web_certificate_addresses = Vec::new();
    ListenerSignature {
        port: listen_port(),
        addresses: listen_addresses(),
        policy: [
            "lan-allowed-networks",
            "web-access-enabled",
            "web-listen-port",
            "web-certificate-path",
            "web-private-key-path",
            "web-listen-addresses",
            "web-allowed-networks",
            "web-allowed-hosts",
            "web-permission-profile",
        ]
        .into_iter()
        .map(Config::get_option)
        .collect(),
        certificate_addresses: web_certificate_addresses,
    }
}

#[cfg(not(target_os = "ios"))]
async fn bind_listeners(
    server: ServerPtr,
    cancel: CancellationToken,
    tracker: &TaskTracker,
) -> ResultType<(Vec<tokio::task::JoinHandle<()>>, usize)> {
    // Validate the whole LAN policy up front so a bad value surfaces as a
    // listener failure in the UI instead of being skipped silently.
    let config = crate::lan_config::LanConfig::load()?;
    if config.allowed_networks.is_empty() {
        log::warn!("LAN allowed networks are empty; every incoming connection will be refused");
    }
    let port = config.port.get();
    let mut listeners = Vec::new();
    if config.listen_addresses.is_empty() {
        listeners.push(listen_any(port).await.map_err(|err| {
            hbb_common::anyhow::anyhow!("Failed to listen on port {port}: {err}")
        })?);
    } else {
        for ip in config.listen_addresses {
            let endpoint = SocketAddr::new(ip, port);
            listeners.push(new_listener(endpoint, true).await.map_err(|err| {
                hbb_common::anyhow::anyhow!("Failed to listen on {endpoint}: {err}")
            })?);
        }
    }

    // The Web gateway still takes a watch receiver; bridge it from the token.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let stop_rx = {
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let cancelled = cancel.clone();
        tracker.spawn(async move {
            cancelled.cancelled().await;
            let _ = stop_tx.send(true);
        });
        stop_rx
    };
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let handles = match crate::web_gateway::bind(server.clone(), stop_rx).await {
        Ok(handles) => handles,
        Err(err) => {
            log::error!("Failed to start optional Web access: {err}");
            crate::web_gateway::record_start_failure(&err.to_string());
            Vec::new()
        }
    };
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let handles = Vec::new();
    let native_listener_count = listeners.len();
    for listener in listeners {
        let server = server.clone();
        let cancel = cancel.clone();
        log::info!("LAN server listening on {}", listener.local_addr()?);
        tracker.spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((stream, addr)) => {
                                let addr = normalize_source_addr(addr);
                                if !source_allowed(addr.ip()) {
                                    log::warn!("Rejected LAN connection from disallowed source {addr}");
                                    continue;
                                }
                                if let Err(err) = stream.set_nodelay(true) {
                                    log::debug!("Failed to enable TCP_NODELAY for {addr}: {err}");
                                }
                                let local_addr = match stream.local_addr() {
                                    Ok(value) => value,
                                    Err(err) => {
                                        log::warn!("Failed to read local address for {addr}: {err}");
                                        continue;
                                    }
                                };
                                let server = server.clone();
                                CONNECTION_TASKS.spawn(async move {
                                    let stream = Stream::from(stream, local_addr);
                                    if let Err(err) = crate::server::create_lan_connection(server, stream, addr, false).await {
                                        match crate::lan_protocol::HandshakeError::find(&err) {
                                            Some(handshake) => log::warn!(
                                                "LAN handshake from {addr} failed (code {}): {err}",
                                                handshake.code()
                                            ),
                                            None => log::warn!("LAN connection from {addr} failed: {err}"),
                                        }
                                    }
                                });
                            }
                            Err(err) => {
                                log::warn!("LAN listener accept failed: {err}");
                                tokio::time::sleep(Duration::from_millis(200)).await;
                            }
                        }
                    }
                }
            }
        });
    }
    Ok((handles, native_listener_count))
}

pub fn source_allowed(ip: IpAddr) -> bool {
    let configured = Config::get_option(crate::lan_config::OPTION_ALLOWED_NETWORKS);
    crate::lan_config::cached_allowed_networks(&configured)
        .map(|networks| networks.contains(ip))
        .unwrap_or(false)
}

/// Strict check against an explicit policy string: an unparsable policy denies all.
pub(crate) fn source_allowed_with(ip: IpAddr, configured: &str) -> bool {
    configured
        .parse::<crate::lan_config::AllowedNetworks>()
        .map(|networks| networks.contains(ip))
        .unwrap_or(false)
}

fn normalize_source_addr(addr: SocketAddr) -> SocketAddr {
    SocketAddr::new(normalize_source_ip(addr.ip()), addr.port())
}

#[cfg(not(target_os = "ios"))]
fn start_auxiliary_services_once() {
    // Supervise the discovery responder: a bind failure (port briefly in use) or
    // any other error restarts it with backoff instead of disabling discovery.
    std::thread::spawn(|| {
        let mut backoff = Duration::from_secs(1);
        loop {
            match crate::lan::start_listening() {
                Ok(()) => backoff = Duration::from_secs(1),
                Err(err) => log::warn!("LAN discovery stopped: {err}; restarting in {backoff:?}"),
            }
            std::thread::sleep(backoff);
            backoff = (backoff * 2).min(Duration::from_secs(60));
        }
    });
    #[cfg(target_os = "linux")]
    if crate::is_server() {
        crate::platform::linux_desktop_manager::start_xdesktop();
    }
    scrap::codec::test_av1();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_has_expected_private_networks() {
        assert!(source_allowed_with(
            "192.168.1.20".parse().unwrap(),
            hbb_common::config::DEFAULT_LAN_ALLOWED_NETWORKS
        ));
        assert!(source_allowed_with(
            "::ffff:192.168.1.20".parse().unwrap(),
            hbb_common::config::DEFAULT_LAN_ALLOWED_NETWORKS
        ));
        assert!(source_allowed_with(
            "::ffff:127.0.0.1".parse().unwrap(),
            hbb_common::config::DEFAULT_LAN_ALLOWED_NETWORKS
        ));
        assert!(source_allowed_with(
            "100.64.10.2".parse().unwrap(),
            hbb_common::config::DEFAULT_LAN_ALLOWED_NETWORKS
        ));
        assert!(source_allowed_with(
            "fd00::20".parse().unwrap(),
            hbb_common::config::DEFAULT_LAN_ALLOWED_NETWORKS
        ));
        assert!(!source_allowed_with(
            "8.8.8.8".parse().unwrap(),
            hbb_common::config::DEFAULT_LAN_ALLOWED_NETWORKS
        ));
        assert!(!source_allowed_with(
            "::ffff:8.8.8.8".parse().unwrap(),
            hbb_common::config::DEFAULT_LAN_ALLOWED_NETWORKS
        ));
        assert!(source_allowed_with(
            "10.23.1.9".parse().unwrap(),
            "10.23.0.0/16"
        ));
        assert!(source_allowed_with(
            "::ffff:10.23.1.9".parse().unwrap(),
            "10.23.0.0/16"
        ));
        assert_eq!(
            normalize_source_addr("[::ffff:192.168.1.20]:21118".parse().unwrap()),
            "192.168.1.20:21118".parse().unwrap()
        );
    }

    #[test]
    fn removed_networks_and_empty_policy_stay_denied() {
        for ip in [
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "100.81.211.45",
            "fd7a:115c:a1e0::1",
            "::1",
        ] {
            assert!(!source_allowed_with(ip.parse().unwrap(), ""));
            assert!(!source_allowed_with(ip.parse().unwrap(), "invalid"));
        }
        assert!(!source_allowed_with(
            "100.81.211.45".parse().unwrap(),
            "192.168.0.0/16"
        ));
        assert!(source_allowed_with(
            "100.81.211.45".parse().unwrap(),
            "192.168.0.0/16\n100.64.0.0/10"
        ));
        // One invalid entry denies everything instead of being silently skipped.
        assert!(!source_allowed_with(
            "192.168.1.1".parse().unwrap(),
            "192.168.0.0/16,bogus"
        ));
        assert!(source_allowed_with(
            "fd7a:115c:a1e0::1".parse().unwrap(),
            hbb_common::config::DEFAULT_LAN_ALLOWED_NETWORKS
        ));
        assert!(!source_allowed_with(
            "100.128.0.1".parse().unwrap(),
            hbb_common::config::DEFAULT_LAN_ALLOWED_NETWORKS
        ));
        assert_eq!(normalize_allowed_networks(" \n ").unwrap(), "");
        assert_eq!(
            normalize_allowed_networks("10.0.0.0/8\r\n100.64.0.0/10").unwrap(),
            "10.0.0.0/8,100.64.0.0/10"
        );
    }

    #[test]
    fn normalizes_listener_configuration() {
        assert_eq!(
            normalize_listen_addresses(" 192.168.1.2, fd00::2 ").unwrap(),
            "192.168.1.2,fd00::2"
        );
        assert_eq!(
            normalize_allowed_networks(" 192.168.0.0/16, fd00::/8 ").unwrap(),
            "192.168.0.0/16,fd00::/8"
        );
        assert!(normalize_listen_addresses("host.lan").is_err());
        assert!(normalize_allowed_networks("192.168.1.1").is_err());
    }

    #[test]
    fn discovery_requires_an_active_ready_listener() {
        assert!(discoverable_state(true, true));
        assert!(!discoverable_state(false, true));
        assert!(!discoverable_state(true, false));
        assert!(!discoverable_state(false, false));
    }
}
