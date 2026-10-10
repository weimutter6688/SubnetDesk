use crate::{tcp::FramedStream, ResultType, Stream};
use std::net::SocketAddr;
use tokio::net::ToSocketAddrs;

/// Connect directly to a validated LAN/VPN endpoint.
pub async fn connect_tcp_local<T>(
    target: T,
    local: Option<SocketAddr>,
    ms_timeout: u64,
) -> ResultType<Stream>
where
    T: ToSocketAddrs + std::fmt::Display,
{
    Ok(Stream::from_framed(
        FramedStream::new(target, local, ms_timeout).await?,
    ))
}
