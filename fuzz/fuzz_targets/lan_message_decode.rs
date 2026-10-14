#![no_main]
//! Decodes untrusted bytes exactly as the LAN handshake does (`Message::parse_from_bytes`)
//! and applies the same structural checks to hello messages. Must never panic.

use hbb_common::{
    lan::{NONCE_LEN, PROTOCOL_VERSION},
    message_proto::{message, Message},
    protobuf::Message as _,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(message) = Message::parse_from_bytes(data) else {
        return;
    };
    match message.union {
        Some(message::Union::LanClientHello(hello)) => {
            let _ = hello.protocol_version == PROTOCOL_VERSION
                && hello.client_nonce.len() == NONCE_LEN;
        }
        Some(message::Union::LanServerHello(hello)) => {
            let _ = hbb_common::lan::device_fingerprint(&hello.device_public_key);
        }
        Some(message::Union::LoginRequest(request)) => {
            if let Some(login) = request.lan_login.as_ref() {
                let _ = hbb_common::lan::validate_username(&login.access_username);
                let _ = hbb_common::lan::validate_password(&login.access_password);
            }
        }
        _ => {}
    }
    // Re-encoding a decoded message must not panic either.
    let _ = message_roundtrip(data);
});

fn message_roundtrip(data: &[u8]) -> Option<Vec<u8>> {
    Message::parse_from_bytes(data).ok()?.write_to_bytes().ok()
}
