//! Deterministic workload simulation, not an end-to-end desktop benchmark.
//! Models a fixed 1080p changing scene and ordered TCP retransmission stalls.
use super::*;
use serde_json::json;

fn percentile(values: &mut [f64], percentile: f64) -> f64 {
    values.sort_by(f64::total_cmp);
    values[((values.len() - 1) as f64 * percentile) as usize]
}

#[test]
#[ignore = "writes a repeatable weak-link performance report when explicitly requested"]
fn weak_network_report() {
    let mut reports = Vec::new();
    for (name, mbps, rtt, jitter, stall_every, decode_fps) in [
        ("lan", 100.0, 4, 1, 0, 120),
        ("vpn_4mbps", 4.0, 80, 30, 0, 60),
        ("weak_1mbps", 1.0, 160, 80, 19, 30),
        ("slow_decoder", 10.0, 40, 10, 0, 12),
    ] {
        let mut qos = VideoQoS::default();
        qos.on_connection_open(1);
        qos.new_display("simulation".into());
        qos.set_support_changing_quality("simulation", true);
        qos.new_user_instant = Instant::now() - Duration::from_secs(2);
        let mut rng = 17u64;
        let mut network_queue = 0.0f64;
        let mut decode_queue = 0.0f64;
        let mut frame_credit = 0.0;
        let mut frame_age = Vec::new();
        let mut input_rtt = Vec::new();
        let mut first_frame_ms = None;
        let mut stalls = 0;
        let mut encoded_frames = 0;
        // 60 seconds, 10 ms ticks; a 4x keyframe at startup.
        for tick in 0..6000u32 {
            let kbps = qos.ratio() as f64 * 6000.0;
            qos.store_bitrate(kbps as u32);
            let frame_bytes = kbps * 1000.0 / 8.0 / 30.0;
            frame_credit += qos.fps() as f64 / 100.0;
            if frame_credit >= 1.0 {
                frame_credit -= 1.0;
                encoded_frames += 1;
                network_queue += frame_bytes * if encoded_frames == 1 { 4.0 } else { 1.0 };
                let age = network_queue * 8.0 / (mbps * 1000.0) + rtt as f64 / 2.0 + decode_queue;
                first_frame_ms.get_or_insert(tick as f64 * 10.0 + age);
                frame_age.push(age);
                decode_queue += 1000.0 / decode_fps as f64;
            }
            // Preserve TCP ordering: packet loss is represented as an RTO stall,
            // never by deleting bytes from the application stream.
            let stopped = stall_every > 0 && tick / 100 % stall_every == 0 && tick % 100 < 20;
            if !stopped {
                network_queue = (network_queue - mbps * 1250.0).max(0.0);
            }
            decode_queue = (decode_queue - 10.0).max(0.0);
            if tick % 100 == 99 {
                rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
                let noise = (rng >> 32) as u32 % (jitter * 2 + 1);
                let delay = rtt + noise + (network_queue * 8.0 / (mbps * 1000.0)) as u32;
                qos.user_network_delay(1, delay);
                qos.user_auto_adjust_fps(1, decode_fps);
                qos.adjust_ratio(true);
                input_rtt.push(delay as f64);
                if delay > 500 {
                    stalls += 1;
                }
            }
        }
        reports.push(json!({
            "scenario": name, "bandwidth_mbps": mbps, "base_rtt_ms": rtt,
            "first_frame_ms": first_frame_ms,
            "input_rtt_p95_ms": percentile(&mut input_rtt, 0.95),
            "frame_age_p95_ms": percentile(&mut frame_age, 0.95),
            "seconds_above_500ms": stalls, "encoded_frames": encoded_frames,
            "final_fps_budget": qos.fps(), "final_bitrate_kbps": qos.bitrate(),
        }));
    }
    let report = json!({"kind":"deterministic_workload_simulation", "duration_seconds":60,
        "notes":"Synthetic 1080p workload; not measured desktop latency or packet-loss throughput", "scenarios":reports});
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    if let Ok(path) = std::env::var("SUBNETDESK_WEAK_REPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}
