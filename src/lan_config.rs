#![deny(warnings, clippy::all)]
// iOS has no LAN listener, so some helpers are unused there.
#![cfg_attr(target_os = "ios", allow(dead_code))]

//! Typed, validated view of the LAN `Config` options. Parsing happens once per
//! distinct raw value; invalid policies fail closed instead of being skipped.

use std::{fmt, net::IpAddr, num::NonZeroU16, str::FromStr};

use cidr_utils::cidr::IpCidr;
use hbb_common::{config::Config, lan::DEFAULT_PORT, log};
use parking_lot::Mutex;

pub const OPTION_LISTEN_PORT: &str = "lan-listen-port";
pub const OPTION_LISTEN_ADDRESSES: &str = "lan-listen-addresses";
pub const OPTION_ALLOWED_NETWORKS: &str = "lan-allowed-networks";
pub const OPTION_DISCOVERY_ENABLED: &str = "lan-discovery-enabled";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LanConfigError {
    InvalidNetwork(String),
    InvalidListenAddress(String),
}

impl fmt::Display for LanConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidNetwork(value) => write!(f, "Invalid allowed network: {value}"),
            Self::InvalidListenAddress(value) => write!(f, "Invalid listen address: {value}"),
        }
    }
}

impl std::error::Error for LanConfigError {}

fn split_list(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Maps IPv4-mapped IPv6 sources to IPv4 so v4 policies apply to dual-stack sockets.
pub fn normalize_source_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(ipv6) => ipv6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(ipv6)),
        IpAddr::V4(_) => ip,
    }
}

/// Source allow-list. Every entry must parse; one bad entry rejects the whole value.
#[derive(Clone, Debug, Default)]
pub struct AllowedNetworks(Vec<IpCidr>);

impl AllowedNetworks {
    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = normalize_source_ip(ip);
        self.0.iter().any(|network| network.contains(ip))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Canonical comma-separated form, as persisted by the settings UI.
    pub fn to_canonical_string(&self) -> String {
        self.0
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }
}

impl FromStr for AllowedNetworks {
    type Err = LanConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        split_list(value)
            .map(|network| {
                IpCidr::from_str(network)
                    .map_err(|_| LanConfigError::InvalidNetwork(network.to_owned()))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListenPort(NonZeroU16);

impl ListenPort {
    pub fn parse_or_default(value: &str) -> Self {
        let default = NonZeroU16::new(DEFAULT_PORT).unwrap_or(NonZeroU16::MIN);
        Self(value.trim().parse::<NonZeroU16>().unwrap_or(default))
    }

    pub fn configured() -> Self {
        Self::parse_or_default(&Config::get_option(OPTION_LISTEN_PORT))
    }

    pub fn get(self) -> u16 {
        self.0.get()
    }
}

pub fn parse_listen_addresses(value: &str) -> Result<Vec<IpAddr>, LanConfigError> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|address| {
            address
                .parse::<IpAddr>()
                .map_err(|_| LanConfigError::InvalidListenAddress(address.to_owned()))
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct LanConfig {
    pub port: ListenPort,
    pub listen_addresses: Vec<IpAddr>,
    pub allowed_networks: AllowedNetworks,
}

impl LanConfig {
    pub fn from_options(get: impl Fn(&str) -> String) -> Result<Self, LanConfigError> {
        Ok(Self {
            port: ListenPort::parse_or_default(&get(OPTION_LISTEN_PORT)),
            listen_addresses: parse_listen_addresses(&get(OPTION_LISTEN_ADDRESSES))?,
            allowed_networks: get(OPTION_ALLOWED_NETWORKS).parse()?,
        })
    }

    pub fn load() -> Result<Self, LanConfigError> {
        Self::from_options(Config::get_option)
    }
}

pub fn discovery_enabled() -> bool {
    Config::get_option(OPTION_DISCOVERY_ENABLED) == "Y"
}

lazy_static::lazy_static! {
    static ref ALLOWED_CACHE: Mutex<Option<(String, Option<AllowedNetworks>)>> = Mutex::new(None);
}

/// Parsed allow-list for `raw`, cached per distinct value. Invalid values are
/// logged once and yield `None` (deny all).
pub fn cached_allowed_networks(raw: &str) -> Option<AllowedNetworks> {
    let mut cache = ALLOWED_CACHE.lock();
    if let Some((cached_raw, parsed)) = cache.as_ref() {
        if cached_raw == raw {
            return parsed.clone();
        }
    }
    let parsed = match raw.parse::<AllowedNetworks>() {
        Ok(networks) => Some(networks),
        Err(err) => {
            log::error!("{err}; denying all LAN sources until the policy is fixed");
            None
        }
    };
    *cache = Some((raw.to_owned(), parsed.clone()));
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_networks_parse_strictly() {
        let networks: AllowedNetworks = "10.0.0.0/8, 192.168.0.0/16\nfd00::/8".parse().unwrap();
        assert!(networks.contains("10.1.2.3".parse().unwrap()));
        assert!(networks.contains("::ffff:192.168.1.1".parse().unwrap()));
        assert!(networks.contains("fd00::1".parse().unwrap()));
        assert!(!networks.contains("8.8.8.8".parse().unwrap()));
        for invalid in ["abc", "10.0.0.0/33", "10.0.0.0/8,bogus", "fd00::/129"] {
            assert!(invalid.parse::<AllowedNetworks>().is_err(), "{invalid}");
        }
        assert!("".parse::<AllowedNetworks>().unwrap().is_empty());
    }

    #[test]
    fn canonical_string_round_trips() {
        let networks: AllowedNetworks = " 10.0.0.0/8 \r\n100.64.0.0/10".parse().unwrap();
        let canonical = networks.to_canonical_string();
        assert_eq!(canonical, "10.0.0.0/8,100.64.0.0/10");
        let reparsed: AllowedNetworks = canonical.parse().unwrap();
        assert_eq!(reparsed.to_canonical_string(), canonical);
    }

    #[test]
    fn arbitrary_strings_never_panic() {
        let mut state = 0x9e37_79b9_u32;
        let alphabet = b"0123456789abcdef:./, \n-x[]";
        for _ in 0..5_000 {
            let mut value = String::new();
            for _ in 0..(state % 40) {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                value.push(alphabet[(state as usize) % alphabet.len()] as char);
            }
            if let Ok(networks) = value.parse::<AllowedNetworks>() {
                let canonical = networks.to_canonical_string();
                assert!(canonical.parse::<AllowedNetworks>().is_ok(), "{value:?}");
            }
            state = state.wrapping_add(1);
        }
    }

    #[test]
    fn listen_port_defaults_on_invalid_values() {
        assert_eq!(ListenPort::parse_or_default("").get(), DEFAULT_PORT);
        assert_eq!(ListenPort::parse_or_default("0").get(), DEFAULT_PORT);
        assert_eq!(ListenPort::parse_or_default("70000").get(), DEFAULT_PORT);
        assert_eq!(ListenPort::parse_or_default(" 21200 ").get(), 21200);
    }

    #[test]
    fn lan_config_rejects_bad_listen_addresses() {
        let config = LanConfig::from_options(|key| match key {
            OPTION_LISTEN_PORT => "21200".to_owned(),
            OPTION_LISTEN_ADDRESSES => "192.168.1.5, ::1".to_owned(),
            OPTION_ALLOWED_NETWORKS => "192.168.0.0/16".to_owned(),
            _ => String::new(),
        })
        .unwrap();
        assert_eq!(config.port.get(), 21200);
        assert_eq!(config.listen_addresses.len(), 2);
        assert!(config
            .allowed_networks
            .contains("192.168.3.4".parse().unwrap()));
        assert!(
            LanConfig::from_options(|key| if key == OPTION_LISTEN_ADDRESSES {
                "not-an-ip".to_owned()
            } else {
                String::new()
            })
            .is_err()
        );
    }
}
