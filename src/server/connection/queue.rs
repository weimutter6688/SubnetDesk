#![deny(warnings, clippy::all)]

use hbb_common::{
    anyhow::{anyhow, bail},
    tokio::sync::{mpsc, Notify, OwnedSemaphorePermit, Semaphore},
    ResultType,
};
use hbb_common::{message_proto::Message, protobuf::Message as _, tokio::time::Instant};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc as blocking, Arc,
};

type Outgoing = (Instant, Arc<Message>);
const MESSAGE_QUEUE_CAPACITY: usize = 128;
const MESSAGE_QUEUE_BYTES: usize = 32 * 1024 * 1024;
#[cfg(not(target_os = "android"))]
const IPC_QUEUE_CAPACITY: usize = 128;
#[cfg(not(target_os = "android"))]
const IPC_QUEUE_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn message_channel(
    failure: &QueueFailure,
) -> (super::Sender, BoundedReceiver<Outgoing>) {
    channel(
        "outgoing",
        MESSAGE_QUEUE_CAPACITY,
        MESSAGE_QUEUE_BYTES,
        failure,
        |value| value.1.compute_size() as usize + std::mem::size_of::<Message>(),
    )
}

#[cfg(not(target_os = "android"))]
pub(crate) fn ipc_channel(
    failure: &QueueFailure,
) -> (
    BoundedSender<crate::ipc::Data>,
    BoundedReceiver<crate::ipc::Data>,
) {
    channel(
        "connection manager",
        IPC_QUEUE_CAPACITY,
        IPC_QUEUE_BYTES,
        failure,
        ipc_weight,
    )
}

#[cfg(not(target_os = "android"))]
fn ipc_weight(data: &crate::ipc::Data) -> usize {
    use crate::ipc::{Data, FS};
    // File bytes are deliberately skipped by serde and sent in a separate frame.
    // Count them explicitly; count other payloads without allocating a JSON copy.
    match data {
        Data::FileBlockFromCM { data, .. } | Data::FS(FS::WriteBlock { data, .. }) => {
            data.len().saturating_add(512)
        }
        _ => {
            struct Counter(usize);
            impl std::io::Write for Counter {
                fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                    self.0 = self.0.saturating_add(bytes.len());
                    Ok(bytes.len())
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    Ok(())
                }
            }
            let mut counter = Counter(std::mem::size_of::<Data>());
            if let Err(err) = serde_json::to_writer(&mut counter, data) {
                hbb_common::log::warn!("Failed to measure IPC queue payload: {err}");
                return usize::MAX;
            }
            counter.0
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct QueueFailure(Arc<FailureState>);

#[derive(Default)]
struct FailureState {
    failed: AtomicBool,
    wake: Notify,
}

impl QueueFailure {
    fn fail(&self) {
        self.0.failed.store(true, Ordering::Release);
        self.0.wake.notify_waiters();
    }

    pub(crate) async fn wait(&self) {
        let wake = self.0.wake.notified();
        hbb_common::tokio::pin!(wake);
        // Register before checking the latch, so failure cannot race the wait.
        wake.as_mut().enable();
        if !self.0.failed.load(Ordering::Acquire) {
            wake.await;
        }
    }
}

struct Charged<T> {
    value: T,
    _permit: OwnedSemaphorePermit,
}

enum Transport<T> {
    Async(mpsc::Sender<Charged<T>>),
    Blocking(blocking::SyncSender<Charged<T>>),
}

/// Never blocks a producer or silently drops an ordered message on overflow.
/// The shared failure signal tells the connection to close and release its queues.
pub struct BoundedSender<T> {
    transport: Transport<T>,
    bytes: Arc<Semaphore>,
    failure: QueueFailure,
    weight: fn(&T) -> usize,
    name: &'static str,
}

impl<T> Clone for BoundedSender<T> {
    fn clone(&self) -> Self {
        Self {
            transport: match &self.transport {
                Transport::Async(sender) => Transport::Async(sender.clone()),
                Transport::Blocking(sender) => Transport::Blocking(sender.clone()),
            },
            bytes: self.bytes.clone(),
            failure: self.failure.clone(),
            weight: self.weight,
            name: self.name,
        }
    }
}

impl<T> BoundedSender<T> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) fn failure(&self) -> QueueFailure {
        self.failure.clone()
    }

    pub fn send(&self, value: T) -> ResultType<()> {
        if self.failure.0.failed.load(Ordering::Acquire) {
            bail!("Connection queue already exceeded its budget");
        }
        let permit = u32::try_from((self.weight)(&value).max(1))
            .ok()
            .and_then(|size| self.bytes.clone().try_acquire_many_owned(size).ok());
        let Some(permit) = permit else {
            self.failure.fail();
            bail!("{} queue exceeded its byte budget", self.name);
        };
        let item = Charged {
            value,
            _permit: permit,
        };
        let full = match &self.transport {
            Transport::Async(sender) => match sender.try_send(item) {
                Ok(()) => return Ok(()),
                Err(mpsc::error::TrySendError::Full(_)) => true,
                Err(mpsc::error::TrySendError::Closed(_)) => false,
            },
            Transport::Blocking(sender) => match sender.try_send(item) {
                Ok(()) => return Ok(()),
                Err(blocking::TrySendError::Full(_)) => true,
                Err(blocking::TrySendError::Disconnected(_)) => false,
            },
        };
        if full {
            self.failure.fail();
        }
        Err(anyhow!(
            "{} queue is {}",
            self.name,
            if full { "full" } else { "closed" }
        ))
    }
}

pub(crate) struct BoundedReceiver<T>(mpsc::Receiver<Charged<T>>);

impl<T> BoundedReceiver<T> {
    pub(crate) async fn recv(&mut self) -> Option<T> {
        self.0.recv().await.map(|item| item.value)
    }

    #[cfg(test)]
    pub(crate) fn try_recv(&mut self) -> Result<T, mpsc::error::TryRecvError> {
        self.0.try_recv().map(|item| item.value)
    }
}

pub(crate) struct BlockingReceiver<T>(blocking::Receiver<Charged<T>>, QueueFailure);

impl<T> BlockingReceiver<T> {
    pub(crate) fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<T, blocking::RecvTimeoutError> {
        if self.1 .0.failed.load(Ordering::Acquire) {
            return Err(blocking::RecvTimeoutError::Disconnected);
        }
        self.0.recv_timeout(timeout).map(|item| item.value)
    }
}

pub(crate) fn channel<T>(
    name: &'static str,
    count: usize,
    bytes: usize,
    failure: &QueueFailure,
    weight: fn(&T) -> usize,
) -> (BoundedSender<T>, BoundedReceiver<T>) {
    let (sender, receiver) = mpsc::channel(count);
    (
        BoundedSender {
            transport: Transport::Async(sender),
            bytes: Arc::new(Semaphore::new(bytes)),
            failure: failure.clone(),
            weight,
            name,
        },
        BoundedReceiver(receiver),
    )
}

pub(crate) fn blocking_channel<T>(
    name: &'static str,
    count: usize,
    bytes: usize,
    failure: &QueueFailure,
    weight: fn(&T) -> usize,
) -> (BoundedSender<T>, BlockingReceiver<T>) {
    let (sender, receiver) = blocking::sync_channel(count);
    (
        BoundedSender {
            transport: Transport::Blocking(sender),
            bytes: Arc::new(Semaphore::new(bytes)),
            failure: failure.clone(),
            weight,
            name,
        },
        BlockingReceiver(receiver, failure.clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hbb_common::tokio;

    fn weight(value: &Vec<u8>) -> usize {
        value.capacity()
    }

    #[tokio::test]
    async fn slow_consumer_has_a_byte_limit_and_overflow_wakes_the_connection() {
        let failure = QueueFailure::default();
        let (sender, mut receiver) = channel("test", 4, 8, &failure, weight);
        sender.send(vec![1; 5]).unwrap();
        assert!(sender.send(vec![2; 4]).is_err());
        // The notification must survive overflow before the waiter is polled.
        hbb_common::tokio::time::timeout(std::time::Duration::from_secs(1), failure.wait())
            .await
            .unwrap();
        assert_eq!(receiver.recv().await.unwrap(), vec![1; 5]);
        assert!(
            sender.send(vec![3]).is_err(),
            "ordered streams cannot resume after a lost message"
        );
    }

    #[test]
    fn draining_releases_the_byte_budget_and_preserves_order() {
        let (sender, mut receiver) = channel("test", 2, 8, &QueueFailure::default(), weight);
        sender.send(vec![1; 5]).unwrap();
        assert_eq!(receiver.try_recv().unwrap(), vec![1; 5]);
        sender.send(vec![2; 5]).unwrap();
        sender.send(vec![3; 3]).unwrap();
        assert_eq!(receiver.try_recv().unwrap(), vec![2; 5]);
        assert_eq!(receiver.try_recv().unwrap(), vec![3; 3]);
    }

    #[test]
    fn count_limit_also_applies_to_small_messages() {
        let failure = QueueFailure::default();
        let (sender, _receiver) = channel("test", 1, 100, &failure, weight);
        sender.send(Vec::new()).unwrap();
        assert!(sender.send(Vec::new()).is_err());
        assert!(failure.0.failed.load(Ordering::Acquire));
    }

    #[test]
    fn a_closed_consumer_is_distinct_from_an_overflow() {
        let failure = QueueFailure::default();
        let (sender, receiver) = channel("test", 1, 100, &failure, weight);
        drop(receiver);
        assert!(sender.send(vec![1]).is_err());
        assert!(!failure.0.failed.load(Ordering::Acquire));
    }

    #[test]
    fn blocking_input_queue_never_stalls_a_producer() {
        let failure = QueueFailure::default();
        let (sender, receiver) = blocking_channel("input", 1, 8, &failure, weight);
        sender.send(vec![1; 8]).unwrap();
        assert_eq!(
            receiver.recv_timeout(std::time::Duration::ZERO).unwrap(),
            vec![1; 8]
        );
        sender.send(vec![2; 8]).unwrap();
        assert!(sender.send(vec![3]).is_err());
        assert!(failure.0.failed.load(Ordering::Acquire));
        assert_eq!(
            receiver.recv_timeout(std::time::Duration::ZERO),
            Err(blocking::RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn ipc_raw_file_bytes_are_charged_even_when_serde_skips_them() {
        use crate::ipc::{Data, FS};
        let data = Data::FS(FS::WriteBlock {
            id: 1,
            file_num: 0,
            data: vec![0; 4096].into(),
            compressed: false,
        });
        let failure = QueueFailure::default();
        let (sender, _receiver) = channel("ipc", 4, 4096, &failure, ipc_weight);
        assert!(sender.send(data).is_err());
        assert!(failure.0.failed.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn overflow_wakes_both_the_connection_and_its_ipc_worker() {
        let failure = QueueFailure::default();
        let connection = failure.clone();
        let ipc = failure.clone();
        let connection = tokio::spawn(async move { connection.wait().await });
        let ipc = tokio::spawn(async move { ipc.wait().await });
        tokio::task::yield_now().await;
        failure.fail();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            connection.await.unwrap();
            ipc.await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_stalled_connection_does_not_exhaust_another_connections_budget() {
        let stalled = QueueFailure::default();
        let healthy = QueueFailure::default();
        let (slow, _receiver) = channel("slow", 1, 8, &stalled, weight);
        let (fast, mut receiver) = channel("fast", 1, 8, &healthy, weight);
        slow.send(vec![1; 8]).unwrap();
        assert!(slow.send(vec![2]).is_err());
        for _ in 0..10 {
            fast.send(vec![3; 8]).unwrap();
            assert_eq!(receiver.recv().await.unwrap(), vec![3; 8]);
        }
        assert!(!healthy.0.failed.load(Ordering::Acquire));
    }
}
