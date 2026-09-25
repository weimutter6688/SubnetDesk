use crate::common::SimpleCallOnReturn;
use hbb_common::{log, tokio::{self, sync::oneshot, task::JoinHandle}};
use std::future::Future;

pub(super) struct VoiceCallTask {
    stop: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl VoiceCallTask {
    pub fn spawn<F, Fut>(forward: F, cleanup: impl Fn() + Send + 'static) -> Self
    where
        F: FnOnce(oneshot::Receiver<()>) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let (stop, receiver) = oneshot::channel();
        // Construct before spawning, so even an unpolled task owns its cleanup.
        let cleanup = SimpleCallOnReturn { b: true, f: Box::new(cleanup) };
        let forward = forward(receiver);
        let handle = tokio::spawn(async move {
            let _cleanup = cleanup;
            forward.await;
        });
        Self { stop: Some(stop), handle: Some(handle) }
    }

    pub async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            if stop.send(()).is_err() {
                log::debug!("Voice forwarding task already stopped");
            }
        }
        if let Some(handle) = self.handle.take() {
            if let Err(err) = handle.await {
                log::warn!("Voice forwarding task failed: {err}");
            }
        }
    }
}

impl Drop for VoiceCallTask {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    #[tokio::test(flavor = "current_thread")]
    async fn stop_waits_for_cleanup_before_session_returns() {
        let cleaned = Arc::new(AtomicUsize::new(0));
        let counter = cleaned.clone();
        let task = VoiceCallTask::spawn(|stop| async move {
            let _ = stop.await;
        }, move || { counter.fetch_add(1, Ordering::SeqCst); });
        tokio::task::yield_now().await;
        task.stop().await;
        assert_eq!(cleaned.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelling_an_unpolled_task_still_cleans_up() {
        let cleaned = Arc::new(AtomicUsize::new(0));
        let counter = cleaned.clone();
        let mut task = VoiceCallTask::spawn(|_| std::future::pending(),
            move || { counter.fetch_add(1, Ordering::SeqCst); });
        let handle = task.handle.take().unwrap();
        handle.abort();
        assert!(handle.await.unwrap_err().is_cancelled());
        assert_eq!(cleaned.load(Ordering::SeqCst), 1);
    }
}
