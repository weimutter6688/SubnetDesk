use std::{fmt, net::IpAddr, str::FromStr};

use anyhow::{anyhow, bail, Result};
use sha2::{Digest, Sha256};

pub const DEFAULT_PORT: u16 = 21_118;
pub const PROTOCOL_VERSION: u32 = 2;
pub const CREDENTIAL_PAYLOAD_VERSION: u32 = 1;
pub const NONCE_LEN: usize = 32;
pub const MAX_ENDPOINT_LEN: usize = 512;
pub const MAX_HOST_LEN: usize = 253;
pub const MAX_USERNAME_LEN: usize = 64;
pub const MAX_PASSWORD_LEN: usize = 256;

pub fn device_fingerprint(public_key: &[u8]) -> String {
    Sha256::digest(public_key)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Endpoint {
    host: String,
    port: u16,
    display: String,
}

impl Endpoint {
    pub fn parse(input: &str) -> Result<Self> {
        if input.is_empty() {
            bail!("Endpoint is required");
        }
        if input.len() > MAX_ENDPOINT_LEN {
            bail!("Endpoint is too long");
        }
        if input.chars().any(|c| c.is_control() || c.is_whitespace()) {
            bail!("Endpoint contains whitespace or control characters");
        }

        let (host, port) = if let Some(rest) = input.strip_prefix('[') {
            let close = rest
                .find(']')
                .ok_or_else(|| anyhow!("IPv6 endpoint is missing a closing bracket"))?;
            let host = &rest[..close];
            if host.parse::<std::net::Ipv6Addr>().is_err() {
                bail!("Invalid IPv6 address");
            }
            let suffix = &rest[close + 1..];
            let port = if suffix.is_empty() {
                DEFAULT_PORT
            } else {
                let value = suffix
                    .strip_prefix(':')
                    .ok_or_else(|| anyhow!("Unexpected data after IPv6 address"))?;
                parse_port(value)?
            };
            (host.to_owned(), port)
        } else if input.matches(':').count() > 1 {
            if input.parse::<std::net::Ipv6Addr>().is_err() {
                bail!("IPv6 endpoints with an explicit port must use brackets");
            }
            (input.to_owned(), DEFAULT_PORT)
        } else if let Some((host, port)) = input.rsplit_once(':') {
            if host.is_empty() {
                bail!("Endpoint host is required");
            }
            (host.to_owned(), parse_port(port)?)
        } else {
            (input.to_owned(), DEFAULT_PORT)
        };

        validate_host(&host)?;
        let display = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        Ok(Self {
            host,
            port,
            display,
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn authority(&self) -> &str {
        &self.display
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display)
    }
}

fn parse_port(value: &str) -> Result<u16> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        bail!("Invalid endpoint port");
    }
    let port = value
        .parse::<u16>()
        .map_err(|_| anyhow!("Endpoint port must be between 1 and 65535"))?;
    if port == 0 {
        bail!("Endpoint port must be between 1 and 65535");
    }
    Ok(port)
}

fn validate_host(host: &str) -> Result<()> {
    if host.is_empty() || host.len() > MAX_HOST_LEN {
        bail!("Invalid endpoint host length");
    }
    if IpAddr::from_str(host).is_ok() {
        return Ok(());
    }
    if host.ends_with('.') || host.starts_with('.') {
        bail!("Invalid domain name");
    }
    for label in host.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            bail!("Invalid domain name");
        }
    }
    Ok(())
}

pub fn validate_username(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("Access username is required");
    }
    if value.len() > MAX_USERNAME_LEN {
        bail!("Access username is too long");
    }
    if value.chars().any(char::is_control) {
        bail!("Access username contains control characters");
    }
    Ok(value.to_owned())
}

pub fn validate_password(password: &[u8]) -> Result<()> {
    if password.is_empty() {
        bail!("Access password is required");
    }
    if password.len() > MAX_PASSWORD_LEN {
        bail!("Access password is too long");
    }
    let password = std::str::from_utf8(password)
        .map_err(|_| anyhow!("Access password must be valid UTF-8"))?;
    if password.chars().any(char::is_control) {
        bail!("Access password contains control characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_endpoint_forms() {
        assert_eq!(
            Endpoint::parse("192.168.1.20").unwrap().port(),
            DEFAULT_PORT
        );
        assert_eq!(
            Endpoint::parse("host.lan:22000").unwrap().host(),
            "host.lan"
        );
        assert_eq!(
            Endpoint::parse("[fd00::20]:21118").unwrap().authority(),
            "[fd00::20]:21118"
        );
    }

    #[test]
    fn rejects_ambiguous_or_hostile_endpoints() {
        for input in [
            "",
            " host.lan",
            "host.lan\nforged",
            "host.lan:0",
            "host.lan:65536",
            "[fd00::20:21118",
            "bad_label.lan",
        ] {
            assert!(Endpoint::parse(input).is_err(), "accepted {:?}", input);
        }
    }

    #[test]
    fn validates_access_fields() {
        assert_eq!(validate_username("  operator  ").unwrap(), "operator");
        assert!(validate_username("bad\nname").is_err());
        assert!(validate_password(b"").is_err());
        assert!(validate_password(b"bad\npassword").is_err());
        assert!(validate_password(&vec![b'x'; MAX_PASSWORD_LEN + 1]).is_err());
    }

    #[test]
    fn device_fingerprint_is_stable_sha256_hex() {
        assert_eq!(
            device_fingerprint(b"device-key"),
            "5d19e448729151e104c2f1069e08a199f6a0bad7192e2588e21d924f734c04c6"
        );
    }
}
