use crate::{bytes_codec::BytesCodec, ResultType};
use anyhow::Context as AnyhowCtx;
use bytes::{BufMut, Bytes, BytesMut};
use futures::{Sink, SinkExt, Stream, StreamExt};
use protobuf::Message;
use sodiumoxide::crypto::{
    box_,
    secretbox::{self, Key, Nonce},
};
use std::{
    io::{self, Error, ErrorKind},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{lookup_host, TcpListener, TcpSocket, ToSocketAddrs},
};
use tokio_util::codec::Framed;

mod connect;

pub trait TcpStreamTrait: AsyncRead + AsyncWrite + Unpin {}
pub struct DynTcpStream(pub Box<dyn TcpStreamTrait + Send + Sync>);

#[derive(Clone, Copy)]
pub enum EndpointRole {
    Client,
    Server,
}

#[derive(Clone)]
pub struct Encrypt {
    key: Key,
    send_sequence: u64,
    receive_sequence: u64,
    role: EndpointRole,
}

pub struct FramedStream(
    pub Framed<DynTcpStream, BytesCodec>,
    pub SocketAddr,
    pub Option<Encrypt>,
    pub u64,
);

pub enum StreamEvent {
    Received(Option<Result<BytesMut, Error>>),
    Flushed(Result<(), Error>),
}

impl Deref for FramedStream {
    type Target = Framed<DynTcpStream, BytesCodec>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for FramedStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Deref for DynTcpStream {
    type Target = Box<dyn TcpStreamTrait + Send + Sync>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for DynTcpStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub(crate) fn new_socket(
    addr: std::net::SocketAddr,
    reuse: bool,
) -> Result<TcpSocket, std::io::Error> {
    let socket = match addr {
        std::net::SocketAddr::V4(..) => TcpSocket::new_v4()?,
        std::net::SocketAddr::V6(..) => TcpSocket::new_v6()?,
    };
    if reuse {
        // windows has no reuse_port, but its reuse_address
        // almost equals to unix's reuse_port + reuse_address,
        // though may introduce nondeterministic behavior
        // illumos has no support for SO_REUSEPORT
        #[cfg(all(unix, not(target_os = "illumos")))]
        socket.set_reuseport(true).ok();
        socket.set_reuseaddr(true).ok();
    }
    socket.bind(addr)?;
    Ok(socket)
}

impl FramedStream {
    pub async fn new<T: ToSocketAddrs + std::fmt::Display>(
        remote_addr: T,
        local_addr: Option<SocketAddr>,
        ms_timeout: u64,
    ) -> ResultType<Self> {
        let display = remote_addr.to_string();
        let resolve = async {
            let addresses = lookup_host(&remote_addr)
                .await
                .map_err(|err| crate::anyhow::anyhow!("DNS resolution failed for {display}: {err}"))?
                .filter(|remote| local_addr.map_or(true, |local| local.is_ipv4() == remote.is_ipv4()))
                .collect();
            Ok(addresses)
        };
        let stream = connect::resolve_and_connect(resolve, ms_timeout, |remote| async move {
            let local = local_addr.unwrap_or_else(|| {
                crate::config::Config::get_any_listen_addr(remote.is_ipv4())
            });
            let stream = new_socket(local, true)
                .context("failed to bind local socket")?
                .connect(remote)
                .await
                .map_err(|err| crate::anyhow::anyhow!("{remote}: {err} ({:?})", err.kind()))?;
            if let Err(err) = stream.set_nodelay(true) {
                crate::log::debug!("Failed to enable TCP_NODELAY for {remote}: {err}");
            }
            Ok(stream)
        })
        .await
        .with_context(|| format!("Failed to connect to {display}"))?;
        let addr = stream.local_addr()?;
        Ok(Self(
            Framed::new(DynTcpStream(Box::new(stream)), BytesCodec::new()),
            addr,
            None,
            0,
        ))
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.1
    }

    pub fn set_send_timeout(&mut self, ms: u64) {
        self.3 = ms;
    }

    pub fn from(stream: impl TcpStreamTrait + Send + Sync + 'static, addr: SocketAddr) -> Self {
        Self(
            Framed::new(DynTcpStream(Box::new(stream)), BytesCodec::new()),
            addr,
            None,
            0,
        )
    }

    pub fn set_raw(&mut self) {
        self.0.codec_mut().set_raw();
        self.2 = None;
    }

    pub fn is_secured(&self) -> bool {
        self.2.is_some()
    }

    #[inline]
    pub async fn send(&mut self, msg: &impl Message) -> ResultType<()> {
        self.send_raw(msg.write_to_bytes()?).await
    }

    /// Queue one frame, then drive its flush alongside reads with next_or_flush.
    /// The caller must not queue another frame until the first has flushed.
    pub async fn queue_message(&mut self, msg: &impl Message) -> ResultType<()> {
        let mut bytes = msg.write_to_bytes()?;
        if let Some(key) = self.2.as_mut() { bytes = key.enc(&bytes)?; }
        self.0.feed(Bytes::from(bytes)).await?;
        Ok(())
    }

    pub async fn next_or_flush(&mut self, flushing: bool) -> StreamEvent {
        let mut event = futures::future::poll_fn(|cx| {
            if flushing {
                if let Poll::Ready(result) = Pin::new(&mut self.0).poll_flush(cx) {
                    return Poll::Ready(StreamEvent::Flushed(result));
                }
            }
            Pin::new(&mut self.0).poll_next(cx).map(StreamEvent::Received)
        }).await;
        if let StreamEvent::Received(Some(Ok(bytes))) = &mut event {
            if let Some(key) = self.2.as_mut() {
                if let Err(error) = key.dec(bytes) { return StreamEvent::Received(Some(Err(error))); }
            }
        }
        event
    }

    #[inline]
    pub async fn send_raw(&mut self, msg: Vec<u8>) -> ResultType<()> {
        let mut msg = msg;
        if let Some(key) = self.2.as_mut() {
            msg = key.enc(&msg)?;
        }
        self.send_bytes(bytes::Bytes::from(msg)).await?;
        Ok(())
    }

    #[inline]
    pub async fn send_bytes(&mut self, bytes: Bytes) -> ResultType<()> {
        if self.3 > 0 {
            super::timeout(self.3, self.0.send(bytes)).await??;
        } else {
            self.0.send(bytes).await?;
        }
        Ok(())
    }

    #[inline]
    pub async fn next(&mut self) -> Option<Result<BytesMut, Error>> {
        let mut res = self.0.next().await;
        if let Some(Ok(bytes)) = res.as_mut() {
            if let Some(key) = self.2.as_mut() {
                if let Err(err) = key.dec(bytes) {
                    return Some(Err(err));
                }
            }
        }
        res
    }

    #[inline]
    pub async fn next_timeout(&mut self, ms: u64) -> Option<Result<BytesMut, Error>> {
        if let Ok(res) = super::timeout(ms, self.next()).await {
            res
        } else {
            None
        }
    }

    pub fn set_key(&mut self, key: Key, role: EndpointRole) {
        self.2 = Some(Encrypt::new(key, role));
    }

    fn get_nonce(seqnum: u64, server_to_client: bool) -> Nonce {
        let mut nonce = Nonce([0u8; secretbox::NONCEBYTES]);
        nonce.0[..std::mem::size_of_val(&seqnum)].copy_from_slice(&seqnum.to_le_bytes());
        nonce.0[8] = u8::from(server_to_client);
        nonce
    }
}

const DEFAULT_BACKLOG: u32 = 128;

pub async fn new_listener<T: ToSocketAddrs>(addr: T, reuse: bool) -> ResultType<TcpListener> {
    if !reuse {
        Ok(TcpListener::bind(addr).await?)
    } else {
        let addr = lookup_host(&addr)
            .await?
            .next()
            .context("could not resolve to any address")?;
        new_socket(addr, true)?
            .listen(DEFAULT_BACKLOG)
            .map_err(anyhow::Error::msg)
    }
}

pub async fn listen_any(port: u16) -> ResultType<TcpListener> {
    if let Ok(mut socket) = TcpSocket::new_v6() {
        #[cfg(unix)]
        {
            // illumos has no support for SO_REUSEPORT
            #[cfg(not(target_os = "illumos"))]
            socket.set_reuseport(true).ok();
            socket.set_reuseaddr(true).ok();
            use std::os::unix::io::{FromRawFd, IntoRawFd};
            let raw_fd = socket.into_raw_fd();
            let sock2 = unsafe { socket2::Socket::from_raw_fd(raw_fd) };
            sock2.set_only_v6(false).ok();
            socket = unsafe { TcpSocket::from_raw_fd(sock2.into_raw_fd()) };
        }
        #[cfg(windows)]
        {
            use std::os::windows::prelude::{FromRawSocket, IntoRawSocket};
            let raw_socket = socket.into_raw_socket();
            let sock2 = unsafe { socket2::Socket::from_raw_socket(raw_socket) };
            sock2.set_only_v6(false).ok();
            socket = unsafe { TcpSocket::from_raw_socket(sock2.into_raw_socket()) };
        }
        if socket
            .bind(SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port))
            .is_ok()
        {
            if let Ok(l) = socket.listen(DEFAULT_BACKLOG) {
                return Ok(l);
            }
        }
    }
    Ok(new_socket(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port),
        true,
    )?
    .listen(DEFAULT_BACKLOG)?)
}

impl Unpin for DynTcpStream {}

impl AsyncRead for DynTcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        AsyncRead::poll_read(Pin::new(&mut self.0), cx, buf)
    }
}

impl AsyncWrite for DynTcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write(Pin::new(&mut self.0), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        AsyncWrite::poll_flush(Pin::new(&mut self.0), cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        AsyncWrite::poll_shutdown(Pin::new(&mut self.0), cx)
    }
}

impl<R: AsyncRead + AsyncWrite + Unpin> TcpStreamTrait for R {}

impl Encrypt {
    pub fn new(key: Key, role: EndpointRole) -> Self {
        Self { key, send_sequence: 0, receive_sequence: 0, role }
    }

    pub fn dec(&mut self, bytes: &mut BytesMut) -> Result<(), Error> {
        self.receive_sequence = self.receive_sequence.checked_add(1)
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "encryption sequence exhausted"))?;
        let nonce = FramedStream::get_nonce(self.receive_sequence, matches!(self.role, EndpointRole::Client));
        match secretbox::open(bytes, &nonce, &self.key) {
            Ok(res) => {
                bytes.clear();
                bytes.put_slice(&res);
                Ok(())
            }
            Err(()) => Err(Error::new(ErrorKind::Other, "decryption error")),
        }
    }

    pub fn enc(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        self.send_sequence = self.send_sequence.checked_add(1)
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "encryption sequence exhausted"))?;
        let nonce = FramedStream::get_nonce(self.send_sequence, matches!(self.role, EndpointRole::Server));
        Ok(secretbox::seal(data, &nonce, &self.key))
    }

    pub fn decode(
        symmetric_data: &[u8],
        their_pk_b: &[u8],
        our_sk_b: &box_::SecretKey,
    ) -> ResultType<Key> {
        if their_pk_b.len() != box_::PUBLICKEYBYTES {
            anyhow::bail!("Handshake failed: pk length {}", their_pk_b.len());
        }
        let nonce = box_::Nonce([0u8; box_::NONCEBYTES]);
        let mut pk_ = [0u8; box_::PUBLICKEYBYTES];
        pk_[..].copy_from_slice(their_pk_b);
        let their_pk_b = box_::PublicKey(pk_);
        let symmetric_key = box_::open(symmetric_data, &nonce, &their_pk_b, &our_sk_b)
            .map_err(|_| anyhow::anyhow!("Handshake failed: box decryption failure"))?;
        if symmetric_key.len() != secretbox::KEYBYTES {
            anyhow::bail!("Handshake failed: invalid secret key length from peer");
        }
        let mut key = [0u8; secretbox::KEYBYTES];
        key[..].copy_from_slice(&symmetric_key);
        Ok(Key(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposite_directions_do_not_reuse_ciphertext_for_the_same_plaintext() {
        let key = Key([7; secretbox::KEYBYTES]);
        let mut client = Encrypt::new(key.clone(), EndpointRole::Client);
        let mut server = Encrypt::new(key, EndpointRole::Server);
        let client_cipher = client.enc(b"known plaintext").unwrap();
        let server_cipher = server.enc(b"known plaintext").unwrap();
        assert_ne!(client_cipher, server_cipher);
        let mut from_client = BytesMut::from(client_cipher.as_slice());
        let mut from_server = BytesMut::from(server_cipher.as_slice());
        server.dec(&mut from_client).unwrap();
        client.dec(&mut from_server).unwrap();
        assert_eq!(&from_client[..], b"known plaintext");
        assert_eq!(&from_server[..], b"known plaintext");
    }

    #[test]
    fn reflection_replay_and_unencrypted_markers_are_rejected() {
        let key = Key([7; secretbox::KEYBYTES]);
        let mut client = Encrypt::new(key.clone(), EndpointRole::Client);
        let cipher = client.enc(b"known plaintext").unwrap();
        assert!(client.dec(&mut BytesMut::from(cipher.as_slice())).is_err());
        let mut server = Encrypt::new(key.clone(), EndpointRole::Server);
        server.dec(&mut BytesMut::from(cipher.as_slice())).unwrap();
        assert!(server.dec(&mut BytesMut::from(cipher.as_slice())).is_err());
        for marker in [&[][..], &[0][..]] {
            let mut server = Encrypt::new(key.clone(), EndpointRole::Server);
            assert!(server.dec(&mut BytesMut::from(marker)).is_err());
        }
    }

    #[test]
    fn counter_exhaustion_never_wraps_to_a_reused_nonce() {
        let mut cipher = Encrypt::new(Key([7; secretbox::KEYBYTES]), EndpointRole::Client);
        cipher.send_sequence = u64::MAX;
        cipher.receive_sequence = u64::MAX;
        assert!(cipher.enc(b"payload").is_err());
        assert!(cipher.dec(&mut BytesMut::from(&[0; 32][..])).is_err());
        assert_eq!(cipher.send_sequence, u64::MAX);
        assert_eq!(cipher.receive_sequence, u64::MAX);
    }

    #[tokio::test]
    async fn hostname_connection_respects_explicit_ipv4_bind_and_preserves_framing() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_address = listener.local_addr().unwrap();
        let mut client = FramedStream::new(
            format!("localhost:{}", server_address.port()),
            Some("127.0.0.1:0".parse().unwrap()),
            2000,
        ).await.unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        let mut server = FramedStream::from(socket, server_address);
        client.send_raw(b"loopback payload".to_vec()).await.unwrap();
        let payload = crate::timeout(1000, server.next()).await.unwrap().unwrap().unwrap();
        assert_eq!(&payload[..], b"loopback payload");
        assert!(client.local_addr().is_ipv4());
    }
}
