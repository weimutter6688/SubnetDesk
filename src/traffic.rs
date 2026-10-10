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
pub fn video(peer: &str, congested: bool) {
    let now = Instant::now();
    let mut peers = PEERS.lock().unwrap();
    peers.retain(|_, p| {
        p.video
            .map_or(false, |t| now.duration_since(t) < Duration::from_secs(10))
    });
    let p = peers.entry(peer.into()).or_default();
    p.video = Some(now);
    p.congested = congested;
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
    #[cfg(not(target_os = "ios"))]
    #[test]
    fn upload_and_download_pacing_use_the_same_queue_delay_feedback() {
        let id = -103;
        let incoming = "test:incoming-congestion";
        let outgoing = "test:outgoing-congestion";
        let mut upload_delay = crate::network_delay::NetworkDelay::default();
        let mut registry = crate::server::video_service::VIDEO_QOS.lock().unwrap();
        registry.on_connection_open(id);
        for (delay, samples, expected_block) in [(250, 60, 16384), (850, 4, 4096), (250, 4, 16384)] {
            for _ in 0..samples {
                registry.user_network_delay(id, delay);
                upload_delay.record(delay);
            }
            video(incoming, registry.congested(id));
            video(outgoing, upload_delay.congested());
            for key in [incoming, outgoing] {
                // Advance past the previous pacing slot without a wall-clock sleep.
                PEERS.lock().unwrap().get_mut(key).unwrap().next_file = None;
                assert_eq!(file_allowance(key), Some(expected_block));
                assert_eq!(file_allowance(key), None);
            }
        }
        registry.on_connection_close(id);
        let mut peers = PEERS.lock().unwrap();
        peers.remove(incoming);
        peers.remove(outgoing);
    }
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
        server.set_key(key.clone(), hbb_common::tcp::EndpointRole::Server);
        client.set_key(key, hbb_common::tcp::EndpointRole::Client);
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
