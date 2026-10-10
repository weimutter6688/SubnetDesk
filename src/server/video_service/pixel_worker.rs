use super::{frame_cache::CachedPixels, *};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError},
};

// Native encoders are created, used and destroyed on their owning thread.
// No Encoder or GPU/device pointer crosses a thread boundary.
enum PixelConfig {
    VPX(VpxEncoderConfig),
    AOM(AomEncoderConfig),
    #[cfg(feature = "hwcodec")]
    HWRAM(HwRamEncoderConfig),
}

impl PixelConfig {
    fn from_config(config: &EncoderCfg) -> ResultType<Self> {
        Ok(match config {
            EncoderCfg::VPX(c) => Self::VPX(c.clone()),
            EncoderCfg::AOM(c) => Self::AOM(c.clone()),
            #[cfg(feature = "hwcodec")]
            EncoderCfg::HWRAM(c) => Self::HWRAM(c.clone()),
            #[cfg(feature = "vram")]
            EncoderCfg::VRAM(_) => bail!("Texture encoders must remain on the capture thread"),
        })
    }

    fn into_config(self) -> EncoderCfg {
        match self {
            Self::VPX(c) => EncoderCfg::VPX(c),
            Self::AOM(c) => EncoderCfg::AOM(c),
            #[cfg(feature = "hwcodec")]
            Self::HWRAM(c) => EncoderCfg::HWRAM(c),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct EncoderInfo {
    pub bitrate: u32,
    pub changing_quality: bool,
    pub latency_free: bool,
    #[cfg(target_os = "android")]
    pub hardware: bool,
}

impl EncoderInfo {
    pub fn new(encoder: &Encoder) -> Self {
        Self {
            bitrate: encoder.bitrate(),
            changing_quality: encoder.support_changing_quality(),
            latency_free: encoder.latency_free(),
            #[cfg(target_os = "android")]
            hardware: encoder.is_hardware(),
        }
    }
}

enum Command {
    Configure(PixelConfig, bool, usize, usize, f32),
    Encode(Arc<CachedPixels>, u64, i64, f32),
    #[cfg(test)]
    Block(SyncSender<()>, Receiver<()>),
}

pub(super) enum Reply {
    Configured(EncoderInfo),
    Encoded {
        video: Option<VideoFrame>,
        info: EncoderInfo,
        elapsed_ms: u32,
    },
}

struct State {
    encoder: Encoder,
    width: usize,
    height: usize,
    ratio: f32,
    generation: Option<u64>,
    yuv: Vec<u8>,
    mid: Vec<u8>,
    scaled: Vec<u8>,
    failures: usize,
    first: bool,
}

impl State {
    fn encode(
        &mut self,
        pixels: &CachedPixels,
        generation: u64,
        ms: i64,
        ratio: f32,
    ) -> ResultType<Reply> {
        let started = Instant::now();
        if self.ratio != ratio && self.encoder.support_changing_quality() {
            self.encoder.set_quality(ratio)?;
            self.ratio = ratio;
        }
        if self.generation != Some(generation) {
            scrap::scaled_to_yuv(
                pixels,
                self.width,
                self.height,
                self.encoder.yuvfmt(),
                &mut self.yuv,
                &mut self.mid,
                &mut self.scaled,
            )?;
            self.generation = Some(generation);
        }
        let video = encode_frame(
            EncodeInput::YUV(&self.yuv),
            ms,
            &mut self.encoder,
            &mut self.failures,
            &mut self.first,
        )?;
        Ok(Reply::Encoded {
            video,
            info: EncoderInfo::new(&self.encoder),
            elapsed_ms: started.elapsed().as_millis() as u32,
        })
    }
}

static WORKERS: AtomicUsize = AtomicUsize::new(0);
const MAX_WORKERS: usize = 32;

struct WorkerSlot;
impl WorkerSlot {
    fn acquire() -> ResultType<Self> {
        WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_WORKERS).then_some(n + 1)
            })
            .map_err(|_| {
                anyhow!("All {MAX_WORKERS} pixel encoder workers are in use or shutting down")
            })?;
        Ok(Self)
    }
}
impl Drop for WorkerSlot {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) struct PixelWorker {
    tx: SyncSender<Command>,
    rx: Receiver<ResultType<Reply>>,
    pub busy: bool,
    active: Arc<AtomicBool>,
}

impl Drop for PixelWorker {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}

impl PixelWorker {
    pub fn new(
        name: String,
        config: &EncoderCfg,
        i444: bool,
        width: usize,
        height: usize,
        ratio: f32,
    ) -> ResultType<Self> {
        let slot = WorkerSlot::acquire()?;
        let (tx, commands) = mpsc::sync_channel(1);
        let (results, rx) = mpsc::sync_channel(1);
        let wake = std::thread::current();
        let active = Arc::new(AtomicBool::new(true));
        let worker_active = active.clone();
        let watchdog = Watchdog::new();
        #[cfg(windows)]
        watchdog.start_recovery_monitor(name.clone());
        std::thread::Builder::new().name(name).spawn(move || {
            let _slot = slot;
            let _activity = watchdog.begin();
            let mut state: Option<State> = None;
            let _cleanup = watchdog.cleanup_guard();
            loop {
                watchdog.checkpoint(Phase::Idle);
                let Ok(command) = commands.recv() else {
                    break;
                };
                if !worker_active.load(Ordering::Acquire) {
                    break;
                }
                watchdog.checkpoint(Phase::Preparing);
                let result = match command {
                    Command::Configure(config, i444, width, height, ratio) => {
                        watchdog.checkpoint(Phase::Releasing);
                        drop(state.take());
                        watchdog.checkpoint(Phase::Starting);
                        match Encoder::new(config.into_config(), i444) {
                            Ok(encoder) => {
                                let info = EncoderInfo::new(&encoder);
                                state = Some(State {
                                    encoder,
                                    width,
                                    height,
                                    ratio,
                                    generation: None,
                                    yuv: Vec::new(),
                                    mid: Vec::new(),
                                    scaled: Vec::new(),
                                    failures: 0,
                                    first: true,
                                });
                                Ok(Reply::Configured(info))
                            }
                            Err(error) => {
                                log::error!(
                                    "Pixel encoder setup failed: {error}; falling back to VP9"
                                );
                                Encoder::set_fallback(&EncoderCfg::VPX(VpxEncoderConfig {
                                    width: width as _,
                                    height: height as _,
                                    quality: ratio,
                                    codec: VpxVideoCodecId::VP9,
                                    keyframe_interval: None,
                                }));
                                Err(error)
                            }
                        }
                    }
                    Command::Encode(pixels, generation, ms, ratio) => {
                        watchdog.checkpoint(Phase::Encoding);
                        match state.as_mut() {
                            Some(state) => state.encode(&pixels, generation, ms, ratio),
                            None => Err(anyhow!("Pixel encoder is not initialized")),
                        }
                    }
                    #[cfg(test)]
                    Command::Block(entered, release) => {
                        watchdog.checkpoint(Phase::Encoding);
                        entered.send(()).unwrap();
                        release.recv().unwrap();
                        Ok(Reply::Configured(EncoderInfo::new(
                            &state.as_ref().unwrap().encoder,
                        )))
                    }
                };
                // Only one command can be in flight; the result queue cannot grow.
                if results.send(result).is_err() {
                    break;
                }
                wake.unpark();
            }
        })?;
        let mut worker = Self {
            tx,
            rx,
            busy: false,
            active,
        };
        worker.configure(config, i444, width, height, ratio)?;
        Ok(worker)
    }

    fn submit(&mut self, command: Command) -> ResultType<()> {
        if self.busy {
            bail!("Pixel encoder already has a command in flight");
        }
        self.tx
            .try_send(command)
            .map_err(|_| anyhow!("Pixel encoder command channel unavailable"))?;
        self.busy = true;
        Ok(())
    }

    pub fn configure(
        &mut self,
        config: &EncoderCfg,
        i444: bool,
        width: usize,
        height: usize,
        ratio: f32,
    ) -> ResultType<()> {
        self.submit(Command::Configure(
            PixelConfig::from_config(config)?,
            i444,
            width,
            height,
            ratio,
        ))
    }

    pub fn encode(
        &mut self,
        pixels: Arc<CachedPixels>,
        generation: u64,
        ms: i64,
        ratio: f32,
    ) -> ResultType<()> {
        self.submit(Command::Encode(pixels, generation, ms, ratio))
    }

    pub fn poll(&mut self) -> ResultType<Option<Reply>> {
        match self.rx.try_recv() {
            Ok(result) => {
                self.busy = false;
                result.map(Some)
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => bail!("Pixel encoder worker stopped"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(worker: &mut PixelWorker) -> Reply {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(reply) = worker.poll().unwrap() {
                return reply;
            }
            assert!(Instant::now() < deadline, "worker did not complete");
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }

    #[test]
    fn blocked_encoder_does_not_stop_another_viewer_or_queue_more_frames() {
        struct Pixels(Vec<u8>);
        impl TraitPixelBuffer for Pixels {
            fn data(&self) -> &[u8] {
                &self.0
            }
            fn width(&self) -> usize {
                64
            }
            fn height(&self) -> usize {
                64
            }
            fn stride(&self) -> Vec<usize> {
                vec![256]
            }
            fn pixfmt(&self) -> scrap::Pixfmt {
                scrap::Pixfmt::BGRA
            }
        }
        let config = EncoderCfg::VPX(VpxEncoderConfig {
            width: 64,
            height: 64,
            quality: 0.5,
            codec: VpxVideoCodecId::VP9,
            keyframe_interval: None,
        });
        let mut slow =
            PixelWorker::new("blocked-encoder-test".into(), &config, false, 64, 64, 0.5).unwrap();
        let mut fast =
            PixelWorker::new("healthy-encoder-test".into(), &config, false, 64, 64, 0.5).unwrap();
        assert!(matches!(wait(&mut slow), Reply::Configured(_)));
        assert!(matches!(wait(&mut fast), Reply::Configured(_)));
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        slow.submit(Command::Block(entered_tx, release_rx)).unwrap();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut cache = FrameCache::default();
        let mut pixels = Pixels(vec![80; 64 * 64 * 4]);
        cache.update_pixels(&pixels);
        let original = cache.pixels().unwrap().clone();
        assert!(slow
            .encode(original.clone(), cache.generation, 0, 0.5)
            .is_err());
        fast.encode(original.clone(), cache.generation, 0, 0.5)
            .unwrap();
        pixels.0.fill(160);
        cache.update_pixels(&pixels);
        assert_eq!(
            original.data()[0],
            80,
            "a worker's snapshot must not be overwritten"
        );
        assert_eq!(cache.pixels().unwrap().data()[0], 160);
        let generation = cache.generation;
        cache.clear();
        cache.update_pixels(&pixels);
        assert!(
            cache.generation > generation,
            "switching capture paths must not reuse worker YUV generations"
        );
        assert!(matches!(
            wait(&mut fast),
            Reply::Encoded { video: Some(_), .. }
        ));
        assert!(
            slow.busy,
            "the healthy viewer finished without releasing the blocked viewer"
        );
        release_tx.send(()).unwrap();
        wait(&mut slow);
        slow.encode(cache.pixels().unwrap().clone(), cache.generation, 16, 0.5)
            .unwrap();
        assert!(matches!(
            wait(&mut slow),
            Reply::Encoded { video: Some(_), .. }
        ));
        // Reconfiguration stays on the same bounded worker, then accepts the
        // already-cached static image without waiting for another capture.
        slow.configure(&config, false, 64, 64, 0.5).unwrap();
        assert!(matches!(wait(&mut slow), Reply::Configured(_)));
        slow.encode(cache.pixels().unwrap().clone(), cache.generation, 32, 0.5)
            .unwrap();
        assert!(matches!(
            wait(&mut slow),
            Reply::Encoded { video: Some(_), .. }
        ));
    }

    #[test]
    fn dropping_a_blocked_worker_does_not_wait_for_native_cleanup() {
        let config = EncoderCfg::VPX(VpxEncoderConfig {
            width: 64,
            height: 64,
            quality: 0.5,
            codec: VpxVideoCodecId::VP9,
            keyframe_interval: None,
        });
        let mut worker =
            PixelWorker::new("retiring-encoder-test".into(), &config, false, 64, 64, 0.5).unwrap();
        wait(&mut worker);
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        worker
            .submit(Command::Block(entered_tx, release_rx))
            .unwrap();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let (dropped_tx, dropped_rx) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            drop(worker);
            dropped_tx.send(()).unwrap();
        });
        let dropped = dropped_rx.recv_timeout(Duration::from_secs(2));
        release_tx.send(()).unwrap();
        thread.join().unwrap();
        assert!(
            dropped.is_ok(),
            "capture thread must not join a blocked encoder"
        );
    }
}
