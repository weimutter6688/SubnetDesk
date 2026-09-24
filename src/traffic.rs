//! Cooperative file pacing for sessions sharing the same peer.
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Default)]
struct PeerTraffic {
    video: Option<Instant>,
    input: Option<Instant>,
    next_file: Option<Instant>,
    congested: bool,
}
impl PeerTraffic {
    fn allowance(&mut self, now: Instant) -> Option<usize> {
        if self.video.map_or(true, |t| {
            now.saturating_duration_since(t) > Duration::from_secs(3)
        }) {
            return Some(128 * 1024);
        }
        if self.next_file.map_or(false, |t| now < t) {
            return None;
        }
        self.next_file = Some(now + Duration::from_millis(250));
        let interactive = self.input.map_or(false, |t| {
            now.saturating_duration_since(t) < Duration::from_secs(2)
        });
        Some(if self.congested {
            4096
        } else if interactive {
            8192
        } else {
            16384
        })
    }
}
lazy_static::lazy_static! {
    static ref PEERS: Mutex<HashMap<String, PeerTraffic>> = Default::default();
}
pub fn video(peer: &str, delay: u32) {
    let now = Instant::now();
    let mut peers = PEERS.lock().unwrap();
    peers.retain(|_, p| {
        p.video
            .map_or(false, |t| now.duration_since(t) < Duration::from_secs(10))
    });
    let p = peers.entry(peer.into()).or_default();
    p.video = Some(now);
    p.congested = delay >= 200;
}
pub fn input(peer: &str) {
    if let Some(p) = PEERS.lock().unwrap().get_mut(peer) {
        p.input = Some(Instant::now());
    }
}
pub fn file_allowance(peer: &str) -> Option<usize> {
    PEERS
        .lock()
        .unwrap()
        .get_mut(peer)
        .map_or(Some(128 * 1024), |p| p.allowance(Instant::now()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hbb_common::tokio;
    #[test]
    fn transfers_share_budget_and_recover_after_desktop_closes() {
        let now = Instant::now();
        let mut p = PeerTraffic {
            video: Some(now),
            input: Some(now),
            ..Default::default()
        };
        assert_eq!(p.allowance(now), Some(8192));
        assert_eq!(p.allowance(now), None);
        p.congested = true;
        assert_eq!(p.allowance(now + Duration::from_millis(250)), Some(4096));
        assert_eq!(p.allowance(now + Duration::from_secs(4)), Some(128 * 1024));
        assert_eq!(PeerTraffic::default().allowance(now), Some(128 * 1024));
    }

    #[hbb_common::tokio::test]
    async fn encrypted_input_arrives_while_a_large_outbound_frame_is_blocked() {
        use hbb_common::{
            message_proto::*,
            protobuf::Message as _,
            tcp::{FramedStream, StreamEvent},
        };
        let (a, b) = hbb_common::tokio::io::duplex(64);
        let addr = "127.0.0.1:21118".parse().unwrap();
        let mut server = FramedStream::from(a, addr);
        let mut client = FramedStream::from(b, addr);
        let key = hbb_common::sodiumoxide::crypto::secretbox::Key([7; 32]);
        server.set_key(key.clone());
        client.set_key(key);
        let mut frame = Message::new();
        frame.set_audio_frame(AudioFrame {
            data: vec![3; 65536].into(),
            ..Default::default()
        });
        server.queue_message(&frame).await.unwrap();
        let mut input = Message::new();
        input.set_mouse_event(MouseEvent {
            x: 11,
            y: 13,
            ..Default::default()
        });
        client.send(&input).await.unwrap();
        let event = hbb_common::tokio::time::timeout(
            Duration::from_millis(500),
            server.next_or_flush(true),
        )
        .await
        .unwrap();
        match event {
            StreamEvent::Received(Some(Ok(bytes))) => {
                assert_eq!(Message::parse_from_bytes(&bytes).unwrap(), input)
            }
            _ => panic!("Input should arrive before the blocked outgoing frame has flushed"),
        }
        let (event, received) = hbb_common::tokio::join!(server.next_or_flush(true), client.next());
        assert!(matches!(event, StreamEvent::Flushed(Ok(()))));
        assert_eq!(
            Message::parse_from_bytes(&received.unwrap().unwrap()).unwrap(),
            frame
        );
        client.send(&input).await.unwrap();
        let next = server.next().await.unwrap().unwrap();
        assert_eq!(Message::parse_from_bytes(&next).unwrap(), input);
    }
}
