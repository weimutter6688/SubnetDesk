use std::sync::{Arc, RwLock};

use crate::client::*;
use hbb_common::{
    allow_err, bail,
    config::READ_TIMEOUT,
    futures::{SinkExt, StreamExt},
    log,
    message_proto::*,
    protobuf::Message as _,
    rendezvous_proto::ConnType,
    tcp, timeout,
    tokio::{self, net::TcpStream, sync::mpsc},
    tokio_util::codec::{BytesCodec, Framed},
    ResultType, Stream,
};

fn run_rdp(port: u16) {
    std::process::Command::new("cmdkey")
        .arg("/delete:localhost")
        .output()
        .ok();
    let username = std::env::var("rdp_username").unwrap_or_default();
    let password = std::env::var("rdp_password").unwrap_or_default();
    if !username.is_empty() || !password.is_empty() {
        let mut args = vec!["/generic:localhost".to_owned()];
        if !username.is_empty() {
            args.push(format!("/user:{}", username));
        }
        if !password.is_empty() {
            args.push(format!("/pass:{}", password));
        }
        std::process::Command::new("cmdkey")
            .args(&args)
            .output()
            .ok();
    }
    std::process::Command::new("mstsc")
        .arg(format!("/v:localhost:{}", port))
        .spawn()
        .ok();
}

pub async fn listen(
    id: String,
    port: i32,
    interface: impl Interface,
    ui_receiver: mpsc::UnboundedReceiver<Data>,
    lc: Arc<RwLock<LoginConfigHandler>>,
    remote_host: String,
    remote_port: i32,
) -> ResultType<()> {
    let listener = tcp::new_listener(format!("127.0.0.1:{}", port), true).await?;
    let addr = listener.local_addr()?;
    log::info!("listening on port {:?}", addr);
    let is_rdp = port == 0;
    if is_rdp {
        run_rdp(addr.port());
    }
    let mut ui_receiver = ui_receiver;
    loop {
        tokio::select! {
            Ok((forward, addr)) = listener.accept() => {
                log::info!("new connection from {:?}", addr);
                lc.write().unwrap().port_forward = (remote_host.clone(), remote_port);
                let id = id.clone();
                let mut forward = Framed::new(forward, BytesCodec::new());
                let mut close_port_forward = false;
                match connect_and_login(&id, &mut ui_receiver, interface.clone(), &mut forward, is_rdp, &mut close_port_forward).await {
                    Ok(Some(stream)) => {
                        let interface = interface.clone();
                        tokio::spawn(async move {
                            if let Err(err) = run_forward(forward, stream).await {
                                interface.msgbox("error", "Error", &err.to_string(), "");
                            }
                            log::info!("connection from {:?} closed", addr);
                       });
                    }
                    _ if close_port_forward => {
                        break;
                    }
                    Err(err) => {
                        interface.on_establish_connection_error(err.to_string());
                    }
                    _ => {}
                }
            }
            d = ui_receiver.recv() => {
                match d {
                    Some(Data::Close) => {
                        break;
                    }
                    Some(Data::NewRDP) => {
                        println!("receive run_rdp from ui_receiver");
                        run_rdp(addr.port());
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

async fn connect_and_login(
    id: &str,
    ui_receiver: &mut mpsc::UnboundedReceiver<Data>,
    interface: impl Interface,
    forward: &mut Framed<TcpStream, BytesCodec>,
    is_rdp: bool,
    close_port_forward: &mut bool,
) -> ResultType<Option<Stream>> {
    let conn_type = if is_rdp {
        ConnType::RDP
    } else {
        ConnType::PORT_FORWARD
    };
    let (mut stream, device_public_key, connected_endpoint) =
        Client::start(id, conn_type, interface.clone()).await?;
    interface.update_direct(Some(true));
    if device_public_key.is_empty()
        || !crate::client::confirm_lan_device(
            &interface,
            ui_receiver,
            &connected_endpoint,
            &device_public_key,
        )
        .await
    {
        *close_port_forward = true;
        return Ok(None);
    }
    interface.get_lch().write().unwrap().lan_fingerprint =
        crate::lan_protocol::fingerprint(&device_public_key);
    interface.send_initial_lan_login(&mut stream).await;
    let mut buffer = Vec::new();
    let mut received = false;

    loop {
        tokio::select! {
            res = timeout(READ_TIMEOUT, stream.next()) => match res {
                Err(_) => {
                    bail!("Timeout");
                }
                Ok(Some(Ok(bytes))) => {
                    if !received {
                        received = true;
                        interface.update_received(true);
                    }
                    let msg_in = Message::parse_from_bytes(&bytes)?;
                    match msg_in.union {
                        Some(message::Union::LoginResponse(lr)) => {
                            interface.set_auth_retry_after(lr.retry_after_seconds);
                            match lr.union {
                            Some(login_response::Union::Error(err)) => {
                                if !interface.handle_login_error(&err).await {
                                    return Ok(None);
                                }
                            }
                            Some(login_response::Union::PeerInfo(pi)) => {
                                interface.handle_peer_info(pi);
                                break;
                            }
                            _ => {}
                            }
                        }
                        Some(message::Union::TestDelay(t)) => {
                            interface.handle_test_delay(t, &mut stream).await;
                        }
                        _ => {}
                    }
                }
                Ok(Some(Err(err))) => {
                    bail!("Connection closed: {}", err);
                }
                _ => {
                    bail!("Reset by the peer");
                }
            },
            d = ui_receiver.recv() => {
                match d {
                    Some(Data::Login((os_username, os_password, password, remember))) => {
                        interface.handle_login_from_ui(os_username, os_password, password, remember, &mut stream).await;
                    }
                    Some(Data::SubmitLanCredentials) => {
                        interface.send_initial_lan_login(&mut stream).await;
                    }
                    Some(Data::Message(msg)) => {
                        allow_err!(stream.send(&msg).await);
                    }
                    _ => {}
                }
            },
            res = forward.next() => {
                if let Some(Ok(bytes)) = res {
                    buffer.extend(bytes);
                } else {
                    return Ok(None);
                }
            },
        }
    }
    if !buffer.is_empty() {
        stream.send_raw(buffer).await?;
    }
    Ok(Some(stream))
}

async fn run_forward(forward: Framed<TcpStream, BytesCodec>, stream: Stream) -> ResultType<()> {
    log::info!("new port forwarding connection started");
    let mut forward = forward;
    let mut stream = stream;
    loop {
        tokio::select! {
            res = forward.next() => {
                if let Some(Ok(bytes)) = res {
                    stream.send_raw(bytes.to_vec()).await?;
                } else {
                    break;
                }
            },
            res = stream.next() => {
                if let Some(Ok(bytes)) = res {
                    forward.send(bytes).await?;
                } else {
                    break;
                }
            },
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hbb_common::{
        sodiumoxide::crypto::secretbox,
        tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener},
    };

    #[tokio::test]
    async fn forwarding_preserves_encryption_and_binary_data_in_both_directions() {
        let _ = hbb_common::sodiumoxide::init();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut application = TcpStream::connect(address).await.unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        let (client_socket, server_socket) = tokio::io::duplex(256 * 1024);
        let mut client = Stream::from_framed(tcp::FramedStream::from(client_socket, address));
        let mut server = Stream::from_framed(tcp::FramedStream::from(server_socket, address));
        let key = secretbox::Key([11; secretbox::KEYBYTES]);
        client.set_key(key.clone(), tcp::EndpointRole::Client);
        server.set_key(key, tcp::EndpointRole::Server);
        // The forwarding transition must continue the authenticated sequence.
        server.send_raw(b"login response".to_vec()).await.unwrap();
        assert_eq!(&client.next().await.unwrap().unwrap()[..], b"login response");
        let task = tokio::spawn(run_forward(Framed::new(socket, BytesCodec::new()), client));
        timeout(2000, async {
            let outbound: Vec<_> = (0..96 * 1024).map(|i| (i % 256) as u8).collect();
            application.write_all(&outbound).await.unwrap();
            let mut received = Vec::new();
            while received.len() < outbound.len() {
                received.extend(server.next().await.unwrap().unwrap());
            }
            assert_eq!(received, outbound);
            let inbound = vec![0, 255, 0, 128, 1, 13, 10];
            server.send_raw(inbound.clone()).await.unwrap();
            let mut received = vec![0; inbound.len()];
            application.read_exact(&mut received).await.unwrap();
            assert_eq!(received, inbound);
            assert!(server.is_secured());
            application.shutdown().await.unwrap();
            task.await.unwrap().unwrap();
        }).await.unwrap();
    }
}
