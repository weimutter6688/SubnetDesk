#![deny(warnings, clippy::all)]

use super::{ConnectionTmpl, Data};
use hbb_common::{
    anyhow::{anyhow, bail},
    tokio::{
        self,
        io::{AsyncRead, AsyncWrite},
        sync::mpsc,
    },
    ResultType,
};
use std::{
    sync::mpsc as blocking,
    time::{Duration, Instant},
};

const QUEUE_CAPACITY: usize = 32;
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(1);

struct Request {
    data: Data,
    response: bool,
    deadline: Instant,
    reply: blocking::Sender<ResultType<Option<Data>>>,
}

/// A synchronous input device talks to an IPC task on the existing runtime.
/// Call `send` and `request` from an input thread or `spawn_blocking`.
pub(crate) struct InputIpcClient {
    sender: mpsc::Sender<Request>,
    worker: tokio::task::JoinHandle<()>,
}

impl InputIpcClient {
    pub(crate) fn new<T>(mut connection: ConnectionTmpl<T>) -> Self
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (sender, mut receiver) = mpsc::channel::<Request>(QUEUE_CAPACITY);
        let worker = tokio::spawn(async move {
            while let Some(request) = receiver.recv().await {
                let remaining = request.deadline.saturating_duration_since(Instant::now());
                let result = tokio::time::timeout(remaining, async {
                    if remaining.is_zero() {
                        bail!("Input IPC request expired before sending");
                    }
                    connection.send(&request.data).await?;
                    if request.response {
                        let data = connection
                            .next()
                            .await?
                            .ok_or_else(|| anyhow!("Input IPC returned an invalid response"))?;
                        Ok(Some(data))
                    } else {
                        Ok(None)
                    }
                })
                .await
                .unwrap_or_else(|_| Err(anyhow!("Input IPC request timed out")));
                let failed = result.is_err();
                // The caller may already have timed out. Close this transport on any
                // failure so a late reply cannot be consumed by the next request.
                let abandoned = request.reply.send(result).is_err();
                if failed || abandoned {
                    break;
                }
            }
        });
        Self { sender, worker }
    }

    pub(crate) fn send(&self, data: Data) -> ResultType<()> {
        self.request(data, false).map(|_| ())
    }

    pub(crate) fn request(&self, data: Data, response: bool) -> ResultType<Option<Data>> {
        let (reply, receiver) = blocking::channel();
        self.sender
            .try_send(Request {
                data,
                response,
                deadline: Instant::now() + REQUEST_TIMEOUT,
                reply,
            })
            .map_err(|_| anyhow!("Input IPC request queue is full or closed"))?;
        receiver
            .recv_timeout(REQUEST_TIMEOUT)
            .map_err(|err| anyhow!("Input IPC response unavailable: {err}"))?
    }
}

impl Drop for InputIpcClient {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{DataKeyboard, DataKeyboardResponse, DataMouse};
    use enigo::{Key, MouseButton};

    #[tokio::test]
    async fn input_transport_preserves_errors_and_drops_inside_the_runtime() {
        for data in [
            Data::Keyboard(DataKeyboard::KeyDown(Key::Control)),
            Data::Mouse(DataMouse::Down(MouseButton::Left)),
        ] {
            let (local, remote) = tokio::io::duplex(64);
            drop(remote);
            let client = InputIpcClient::new(ConnectionTmpl::new(local));
            let client = tokio::task::spawn_blocking(move || {
                assert!(client.send(data).is_err());
                client
            })
            .await
            .unwrap();
            // Dropping an input device from async setup must never drop a runtime.
            drop(client);
        }
    }

    #[tokio::test]
    async fn key_state_response_is_ordered_after_input() {
        let (local, remote) = tokio::io::duplex(1024);
        let client = InputIpcClient::new(ConnectionTmpl::new(local));
        let peer = tokio::spawn(async move {
            let mut peer = ConnectionTmpl::new(remote);
            assert!(matches!(
                peer.next().await.unwrap(),
                Some(Data::Keyboard(DataKeyboard::KeyDown(Key::Shift)))
            ));
            assert!(matches!(
                peer.next().await.unwrap(),
                Some(Data::Keyboard(DataKeyboard::GetKeyState(Key::Shift)))
            ));
            peer.send(&Data::KeyboardResponse(DataKeyboardResponse::GetKeyState(
                true,
            )))
            .await
            .unwrap();
        });
        tokio::task::spawn_blocking(move || {
            client
                .send(Data::Keyboard(DataKeyboard::KeyDown(Key::Shift)))
                .unwrap();
            assert!(matches!(
                client
                    .request(Data::Keyboard(DataKeyboard::GetKeyState(Key::Shift)), true)
                    .unwrap(),
                Some(Data::KeyboardResponse(DataKeyboardResponse::GetKeyState(
                    true
                )))
            ));
        })
        .await
        .unwrap();
        peer.await.unwrap();
    }

    #[tokio::test]
    async fn unresponsive_service_times_out_and_cannot_reuse_a_late_reply() {
        let (local, _remote) = tokio::io::duplex(1024);
        let client = InputIpcClient::new(ConnectionTmpl::new(local));
        tokio::task::spawn_blocking(move || {
            let start = Instant::now();
            assert!(client
                .request(Data::Keyboard(DataKeyboard::GetKeyState(Key::Shift)), true)
                .is_err());
            assert!(start.elapsed() < Duration::from_secs(3));
            assert!(client
                .send(Data::Keyboard(DataKeyboard::KeyDown(Key::Shift)))
                .is_err());
        })
        .await
        .unwrap();
    }
}
