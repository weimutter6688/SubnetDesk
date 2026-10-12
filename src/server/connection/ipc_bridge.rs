#![deny(warnings, clippy::all)]

use super::{connection_manager_launch_args, IpcReceiver, IpcSender, CLICK_TIME};
use crate::ipc::{self, Data};
#[cfg(target_os = "linux")]
use crate::platform::linux_desktop_manager;
#[cfg(target_os = "linux")]
use hbb_common::timeout;
use hbb_common::{bail, bytes::Bytes, log, sleep, tokio, ResultType};
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;

// Resolve and start CM, then bridge its bidirectional IPC on this runtime.
// Cancelling a saturated connection must also cancel startup and any stalled IPC.
pub(super) async fn start_ipc(
    rx_to_cm: IpcReceiver,
    tx_from_cm: IpcSender,
    rx_desktop_ready: tokio::sync::mpsc::Receiver<()>,
    tx_stream_ready: tokio::sync::mpsc::Sender<()>,
) -> ResultType<()> {
    let failure = tx_from_cm.failure();
    tokio::select! {
        biased;
        _ = failure.wait() => bail!("Connection manager queue exceeded its budget"),
        result = start_ipc_inner(rx_to_cm, tx_from_cm, rx_desktop_ready, tx_stream_ready) => result,
    }
}

async fn start_ipc_inner(
    mut rx_to_cm: IpcReceiver,
    tx_from_cm: IpcSender,
    mut _rx_desktop_ready: mpsc::Receiver<()>,
    tx_stream_ready: mpsc::Sender<()>,
) -> ResultType<()> {
    use hbb_common::anyhow::anyhow;

    loop {
        if !crate::platform::is_prelogin() {
            break;
        }
        sleep(1.).await;
    }
    #[cfg(target_os = "linux")]
    let headless_cm = crate::is_server()
        && crate::platform::is_headless_allowed()
        && linux_desktop_manager::is_headless();
    #[cfg(not(target_os = "linux"))]
    let headless_cm = false;
    let mut stream = None;
    if !headless_cm {
        if let Ok(s) = crate::ipc::connect(1000, "_cm").await {
            stream = Some(s);
        }
    }
    if stream.is_none() {
        let args = connection_manager_launch_args();
        #[allow(unused_mut)]
        #[cfg(target_os = "linux")]
        let mut user = None;

        // Cm run as user, wait until desktop session is ready.
        #[cfg(target_os = "linux")]
        if headless_cm {
            let mut username = linux_desktop_manager::get_username();
            loop {
                if !username.is_empty() {
                    break;
                }
                // `_rx_desktop_ready` is used as a wake-up signal from desktop/session state changes
                // (for example wait_desktop_cm_ready paths). It is not itself a proof of CM readiness.
                if matches!(timeout(1_000, _rx_desktop_ready.recv()).await, Ok(None)) {
                    bail!("Desktop readiness channel closed before the session was ready");
                }
                username = linux_desktop_manager::get_username();
            }
            let uid = {
                let username_for_cmd = username.clone();
                let mut uid_cmd = hbb_common::tokio::process::Command::new("id");
                // TODO:
                // Keep current behavior for now to minimize change risk.
                // If usernames starting with '-' are observed in the field, prefer:
                // `id -u -- <username>` to avoid option-parsing ambiguity.
                // Already verified that `id -u -- <username>` works as expected on macOS and Ubuntu 24.04.
                uid_cmd.arg("-u").arg(&username_for_cmd).kill_on_drop(true);
                let output = timeout(10_000, uid_cmd.output())
                    .await
                    .map_err(|_| anyhow!("Timed out querying uid for {}", username))?
                    .map_err(|e| anyhow!("Failed to run `id -u {}`: {}", username, e))?;
                if !output.status.success() {
                    bail!("Failed to query uid for {}", username);
                }
                let output = String::from_utf8_lossy(&output.stdout);
                let output = output.trim();
                if output.parse::<u32>().is_err() {
                    bail!("Invalid uid {}", output);
                }
                output.to_string()
            };
            user = Some((uid, username));
        }
        #[cfg(target_os = "linux")]
        let cm_uid: Option<u32> = match &user {
            Some((uid, _)) => Some(
                uid.parse::<u32>()
                    .map_err(|_| anyhow!("Invalid uid {}", uid))?,
            ),
            None => None,
        };
        #[cfg(target_os = "linux")]
        if let Some(uid) = cm_uid {
            if let Ok(s) = crate::ipc::connect_for_uid(1000, uid, "_cm").await {
                stream = Some(s);
            }
        }
        if stream.is_none() {
            let run_done;
            if crate::platform::is_root() {
                let mut res = Ok(None);
                for _ in 0..10 {
                    #[cfg(not(any(target_os = "linux")))]
                    {
                        log::debug!("Start cm");
                        res = crate::platform::run_as_user(args.clone());
                    }
                    #[cfg(target_os = "linux")]
                    {
                        log::debug!("Start cm");
                        res = crate::platform::run_as_user(
                            args.clone(),
                            user.clone(),
                            None::<(&str, &str)>,
                        );
                    }
                    if res.is_ok() {
                        break;
                    }
                    log::error!("Failed to run cm: {res:?}");
                    sleep(1.).await;
                }
                if let Some(task) = res? {
                    crate::server::CHILD_PROCESS.lock().unwrap().push(task);
                }
                run_done = true;
            } else {
                run_done = false;
            }
            if !run_done {
                log::debug!("Start cm");
                crate::server::CHILD_PROCESS
                    .lock()
                    .unwrap()
                    .push(crate::run_me(args)?);
            }
            for _ in 0..20 {
                sleep(0.3).await;
                #[cfg(target_os = "linux")]
                {
                    if let Some(uid) = cm_uid {
                        if let Ok(s) = crate::ipc::connect_for_uid(1000, uid, "_cm").await {
                            stream = Some(s);
                            break;
                        }
                        continue;
                    }
                }
                if let Ok(s) = crate::ipc::connect(1000, "_cm").await {
                    stream = Some(s);
                    break;
                }
            }
        }
    }
    if stream.is_none() {
        bail!("Failed to connect to connection manager");
    }

    let _res = tx_stream_ready.send(()).await;
    let mut stream = stream.ok_or(anyhow!("none stream"))?;
    loop {
        tokio::select! {
            res = stream.next() => {
                match res {
                    Err(err) => {
                        return Err(err);
                    }
                    Ok(Some(data)) => {
                        match data {
                            ipc::Data::ClickTime(_)=> {
                                let ct = CLICK_TIME.load(Ordering::SeqCst);
                                let data = ipc::Data::ClickTime(ct);
                                stream.send(&data).await?;
                            }
                            // FileBlockFromCM: data is always sent separately via send_raw.
                            // The data field has #[serde(skip)], so it's empty after deserialization.
                            // Read the raw data bytes following this message.
                            //
                            // Note: Empty data (for empty files) is correctly handled. BytesCodec with
                            // raw=false adds a length prefix, so next_raw() returns empty BytesMut for
                            // zero-length frames. This mirrors the WriteBlock pattern below.
                            ipc::Data::FileBlockFromCM { id, file_num, data: _, compressed, conn_id } => {
                                let raw_data = stream.next_raw().await?;
                                tx_from_cm.send(ipc::Data::FileBlockFromCM {
                                    id,
                                    file_num,
                                    data: raw_data.into(),
                                    compressed,
                                    conn_id,
                                })?;
                            }
                            _ => {
                                tx_from_cm.send(data)?;
                            }
                        }
                    }
                    _ => {}
                }
            }
            res = rx_to_cm.recv() => {
                match res {
                    Some(data) => {
                        if let Data::FS(ipc::FS::WriteBlock{id,
                            file_num,
                            data,
                            compressed}) = data {
                                stream.send(&Data::FS(ipc::FS::WriteBlock{id, file_num, data: Bytes::new(), compressed})).await?;
                                stream.send_raw(data).await?;
                        } else {
                            stream.send(&data).await?;
                        }
                    }
                    None => {
                        bail!("expected");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::queue::{self, QueueFailure};
    use super::*;

    #[tokio::test]
    async fn overflow_cancels_ipc_bootstrap_before_starting_a_connection_manager() {
        let failure = QueueFailure::default();
        let (sender, _receiver) = queue::channel("test", 1, 1, &failure, Vec::<u8>::len);
        assert!(sender.send(vec![0; 2]).is_err());
        let (_tx_to_cm, rx_to_cm) = queue::ipc_channel(&failure);
        let (tx_from_cm, _rx_from_cm) = queue::ipc_channel(&failure);
        let (_desktop, rx_desktop) = mpsc::channel(1);
        let (ready, mut rx_ready) = mpsc::channel(1);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            start_ipc(rx_to_cm, tx_from_cm, rx_desktop, ready),
        )
        .await
        .unwrap();
        assert!(result.unwrap_err().to_string().contains("budget"));
        assert!(rx_ready.recv().await.is_none());
    }
}
