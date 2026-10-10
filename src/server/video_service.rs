// 24FPS (actually 23.976FPS) is what video professionals ages ago determined to be the
// slowest playback rate that still looks smooth enough to feel real.
// Our eyes can see a slight difference and even though 30FPS actually shows
// more information and is more realistic.
// 60FPS is commonly used in game, teamviewer 12 support this for video editing user.

// how to capture with mouse cursor:
// https://docs.microsoft.com/zh-cn/windows/win32/direct3ddxgi/desktop-dup-api?redirectedfrom=MSDN

// RECORD: The following Project has implemented audio capture, hardware codec and mouse cursor drawn.
// https://github.com/PHZ76/DesktopSharing

// dxgi memory leak issue
// https://stackoverflow.com/questions/47801238/memory-leak-in-creating-direct2d-device
// but per my test, it is more related to AcquireNextFrame,
// https://forums.developer.nvidia.com/t/dxgi-outputduplication-memory-leak-when-using-nv-but-not-amd-drivers/108582

// to-do:
// https://slhck.info/video/2017/03/01/rate-control.html

use super::{display_service::check_display_changed, service::ServiceTmpl, video_qos::VideoQoSRegistry, *};
#[cfg(target_os = "linux")]
use crate::common::SimpleCallOnReturn;
#[cfg(target_os = "linux")]
use crate::platform::linux::is_x11;
use crate::privacy_mode::{get_privacy_mode_conn_id, INVALID_PRIVACY_MODE_CONN_ID};
#[cfg(windows)]
use crate::{
    platform::windows::is_process_consent_running,
    privacy_mode::{is_current_privacy_mode_impl, PRIVACY_MODE_IMPL_WIN_MAG},
    ui_interface::is_installed,
};
use hbb_common::{anyhow::anyhow, config};
#[cfg(feature = "hwcodec")]
use scrap::hwcodec::{HwRamEncoder, HwRamEncoderConfig};
#[cfg(feature = "vram")]
use scrap::vram::{VRamEncoder, VRamEncoderConfig};
#[cfg(not(windows))]
use scrap::Capturer;
use scrap::{
    aom::AomEncoderConfig,
    codec::{Encoder, EncoderCfg},
    record::{Recorder, RecorderContext},
    vpxcodec::{VpxEncoderConfig, VpxVideoCodecId},
    CodecFormat, Display, EncodeInput, TraitCapturer, TraitPixelBuffer,
};
#[cfg(windows)]
use std::sync::Once;
use std::{
    collections::HashSet,
    io::ErrorKind::WouldBlock,
    ops::{Deref, DerefMut},
    time::{self, Duration, Instant},
};

pub const OPTION_REFRESH: &'static str = "refresh";

mod frame_cache;
use frame_cache::{CachedFrame, FrameCache};

lazy_static::lazy_static! {
    static ref FRAME_PENDING: Mutex<HashMap<(i32, usize), Instant>> = Default::default();

    pub static ref VIDEO_QOS: Arc<Mutex<VideoQoSRegistry>> = Default::default();
    pub static ref IS_UAC_RUNNING: Arc<Mutex<bool>> = Default::default();
    pub static ref IS_FOREGROUND_WINDOW_ELEVATED: Arc<Mutex<bool>> = Default::default();
    static ref SCREENSHOTS: Mutex<HashMap<(VideoSource, usize), Screenshot>> = Default::default();
}

struct Screenshot {
    sid: String,
    tx: Sender,
    restore_vram: bool,
}

#[inline]
pub fn notify_video_frame_fetched(display_idx: usize, conn_id: i32, frame_tm: Option<Instant>) {
    let sent = {
        let mut pending = FRAME_PENDING.lock().unwrap();
        let key = (conn_id, display_idx);
        if pending.get(&key).map_or(false, |t| frame_tm.map_or(true, |ack| ack >= *t)) {
            pending.remove(&key)
        } else { None }
    };
    if let Some(sent) = sent {
        VIDEO_QOS.lock().unwrap().peer(conn_id).send_ms = sent.elapsed().as_millis() as u32;
    }
}

pub fn notify_video_frame_fetched_by_conn_id(conn_id: i32, frame_tm: Option<Instant>) {
    let displays: Vec<_> = FRAME_PENDING.lock().unwrap().keys()
        .filter_map(|&(id, display)| (id == conn_id).then_some(display)).collect();
    for display in displays { notify_video_frame_fetched(display, conn_id, frame_tm); }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum VideoSource {
    Monitor,
    Camera,
}

impl VideoSource {
    pub fn service_name_prefix(&self) -> &'static str {
        match self {
            VideoSource::Monitor => "monitor",
            VideoSource::Camera => "camera",
        }
    }

    pub fn is_monitor(&self) -> bool {
        matches!(self, VideoSource::Monitor)
    }

    pub fn is_camera(&self) -> bool {
        matches!(self, VideoSource::Camera)
    }
}

#[derive(Clone)]
pub struct VideoService {
    sp: GenericService,
    idx: usize,
    source: VideoSource,
}

impl Deref for VideoService {
    type Target = ServiceTmpl<ConnInner>;

    fn deref(&self) -> &Self::Target {
        &self.sp
    }
}

impl DerefMut for VideoService {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sp
    }
}

pub fn get_service_name(source: VideoSource, idx: usize) -> String {
    format!("{}{}", source.service_name_prefix(), idx)
}

pub fn new(source: VideoSource, idx: usize) -> GenericService {
    let vs = VideoService {
        sp: GenericService::new(get_service_name(source, idx), true),
        idx,
        source,
    };
    GenericService::run(&vs, run);
    vs.sp
}

// Capturer object is expensive, avoiding to create it frequently.
fn create_capturer(
    privacy_mode_id: i32,
    display: Display,
    _current: usize,
    _portable_service_running: bool,
) -> ResultType<Box<dyn TraitCapturer>> {
    #[cfg(not(windows))]
    let c: Option<Box<dyn TraitCapturer>> = None;
    #[cfg(windows)]
    let mut c: Option<Box<dyn TraitCapturer>> = None;
    if privacy_mode_id > 0 {
        #[cfg(windows)]
        {
            // Windows Mode 1 can cover every local monitor with overlay windows,
            // but the legacy magnifier capture backend is still single-monitor
            // constrained. Keep display-switch gating aligned with that backend
            // limit, not just the overlay coverage.
            if let Some(c1) = crate::privacy_mode::win_mag::create_capturer(
                privacy_mode_id,
                display.origin(),
                display.width(),
                display.height(),
            )? {
                c = Some(Box::new(c1));
            }
        }
    }

    match c {
        Some(c1) => return Ok(c1),
        None => {
            #[cfg(windows)]
            {
                log::debug!("Create capturer dxgi|gdi");
                return crate::portable_service::client::create_capturer(
                    _current,
                    display,
                    _portable_service_running,
                );
            }
            #[cfg(not(windows))]
            {
                log::debug!("Create capturer from scrap");
                return Ok(Box::new(
                    Capturer::new(display).with_context(|| "Failed to create capturer")?,
                ));
            }
        }
    };
}

// This function works on privacy mode. Windows only for now.
pub fn test_create_capturer(
    privacy_mode_id: i32,
    display_idx: usize,
    timeout_millis: u64,
) -> String {
    let test_begin = Instant::now();
    loop {
        let err = match Display::all() {
            Ok(mut displays) => {
                if displays.len() <= display_idx {
                    anyhow!(
                        "Failed to get display {}, the displays' count is {}",
                        display_idx,
                        displays.len()
                    )
                } else {
                    let display = displays.remove(display_idx);
                    match create_capturer(privacy_mode_id, display, display_idx, false) {
                        Ok(_) => return "".to_owned(),
                        Err(e) => e,
                    }
                }
            }
            Err(e) => e.into(),
        };
        if test_begin.elapsed().as_millis() >= timeout_millis as _ {
            return err.to_string();
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

// Note: This function is extremely expensive, do not call it frequently.
#[cfg(windows)]
fn check_uac_switch(privacy_mode_id: i32, capturer_privacy_mode_id: i32) -> ResultType<()> {
    if capturer_privacy_mode_id != INVALID_PRIVACY_MODE_CONN_ID
        && is_current_privacy_mode_impl(PRIVACY_MODE_IMPL_WIN_MAG)
    {
        if !is_installed() {
            if privacy_mode_id != capturer_privacy_mode_id {
                if !is_process_consent_running()? {
                    bail!("consent.exe is not running");
                }
            }
            if is_process_consent_running()? {
                bail!("consent.exe is running");
            }
        }
    }
    Ok(())
}

pub(super) struct CapturerInfo {
    pub origin: (i32, i32),
    pub width: usize,
    pub height: usize,
    pub ndisplay: usize,
    pub current: usize,
    pub privacy_mode_id: i32,
    pub _capturer_privacy_mode_id: i32,
    pub capturer: Box<dyn TraitCapturer>,
}

impl Deref for CapturerInfo {
    type Target = Box<dyn TraitCapturer>;

    fn deref(&self) -> &Self::Target {
        &self.capturer
    }
}

impl DerefMut for CapturerInfo {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.capturer
    }
}

fn get_capturer_monitor(
    current: usize,
    portable_service_running: bool,
) -> ResultType<CapturerInfo> {
    #[cfg(target_os = "linux")]
    {
        if !is_x11() {
            return super::wayland::get_capturer_for_display(current);
        }
    }

    let mut displays = Display::all()?;
    let ndisplay = displays.len();
    if ndisplay <= current {
        bail!(
            "Failed to get display {}, displays len: {}",
            current,
            ndisplay
        );
    }

    let display = displays.remove(current);

    #[cfg(target_os = "linux")]
    if let Display::X11(inner) = &display {
        if let Err(err) = inner.get_shm_status() {
            log::warn!(
                "MIT-SHM extension not working properly on select X11 server: {:?}",
                err
            );
        }
    }

    let (origin, width, height) = (display.origin(), display.width(), display.height());
    let name = display.name();
    log::debug!(
        "#displays={}, current={}, origin: {:?}, width={}, height={}, cpus={}/{}, name:{}",
        ndisplay,
        current,
        &origin,
        width,
        height,
        num_cpus::get_physical(),
        num_cpus::get(),
        &name,
    );

    let privacy_mode_id = get_privacy_mode_conn_id().unwrap_or(INVALID_PRIVACY_MODE_CONN_ID);
    #[cfg(not(windows))]
    let capturer_privacy_mode_id = privacy_mode_id;
    #[cfg(windows)]
    let mut capturer_privacy_mode_id = privacy_mode_id;
    #[cfg(windows)]
    {
        if capturer_privacy_mode_id != INVALID_PRIVACY_MODE_CONN_ID
            && is_current_privacy_mode_impl(PRIVACY_MODE_IMPL_WIN_MAG)
        {
            if !is_installed() {
                if is_process_consent_running()? {
                    capturer_privacy_mode_id = INVALID_PRIVACY_MODE_CONN_ID;
                }
            }
        }
    }
    log::debug!(
        "Try create capturer with capturer privacy mode id {}",
        capturer_privacy_mode_id,
    );

    if privacy_mode_id != INVALID_PRIVACY_MODE_CONN_ID {
        if privacy_mode_id != capturer_privacy_mode_id {
            log::info!("In privacy mode, but show UAC prompt window for now");
        } else {
            log::info!("In privacy mode, the peer side cannot watch the screen");
        }
    }
    let capturer = create_capturer(
        capturer_privacy_mode_id,
        display,
        current,
        portable_service_running,
    )?;
    Ok(CapturerInfo {
        origin,
        width,
        height,
        ndisplay,
        current,
        privacy_mode_id,
        _capturer_privacy_mode_id: capturer_privacy_mode_id,
        capturer,
    })
}

fn get_capturer_camera(current: usize) -> ResultType<CapturerInfo> {
    let cameras = camera::Cameras::get_sync_cameras();
    let ncamera = cameras.len();
    if ncamera <= current {
        bail!("Failed to get camera {}, cameras len: {}", current, ncamera,);
    }
    let Some(camera) = cameras.get(current) else {
        bail!(
            "Camera of index {} doesn't exist or platform not supported",
            current
        );
    };
    let capturer = camera::Cameras::get_capturer(current)?;
    let (width, height) = (camera.width as usize, camera.height as usize);
    let origin = (camera.x as i32, camera.y as i32);
    let name = &camera.name;
    let privacy_mode_id = get_privacy_mode_conn_id().unwrap_or(INVALID_PRIVACY_MODE_CONN_ID);
    let _capturer_privacy_mode_id = privacy_mode_id;
    log::debug!(
        "#cameras={}, current={}, origin: {:?}, width={}, height={}, cpus={}/{}, name:{}",
        ncamera,
        current,
        &origin,
        width,
        height,
        num_cpus::get_physical(),
        num_cpus::get(),
        name,
    );
    return Ok(CapturerInfo {
        origin,
        width,
        height,
        ndisplay: ncamera,
        current,
        privacy_mode_id,
        _capturer_privacy_mode_id: privacy_mode_id,
        capturer,
    });
}
fn get_capturer(
    source: VideoSource,
    current: usize,
    portable_service_running: bool,
) -> ResultType<CapturerInfo> {
    match source {
        VideoSource::Monitor => get_capturer_monitor(current, portable_service_running),
        VideoSource::Camera => get_capturer_camera(current),
    }
}

struct ViewerEncoder {
    encoder: Encoder,
    config: EncoderCfg,
    codec: CodecFormat,
    use_i444: bool,
    width: usize,
    height: usize,
    ratio: f32,
    record: bool,
    spf: Duration,
    pending: Option<Instant>,
    last_sent: Option<Instant>,
    second: Instant,
    count: usize,
    failures: usize,
    first_frame: bool,
    repeat: usize,
    frame_generation: u64,
    yuv: Vec<u8>,
    mid: Vec<u8>,
    scaled: Vec<u8>,
}

impl ViewerEncoder {
    fn input_texture(&self) -> bool {
        #[cfg(feature = "vram")]
        { self.encoder.input_texture() }
        #[cfg(not(feature = "vram"))]
        { false }
    }

    fn ready(&self) -> bool {
        self.pending.is_none() && self.last_sent.map_or(true, |t| t.elapsed() >= self.spf)
    }
    fn sent(&mut self) {
        let now = Instant::now();
        self.pending = Some(now);
        self.last_sent = Some(now);
        self.count += 1;
    }
}

fn send_cached_frames(
    display: usize, sp: &GenericService, viewers: &mut HashMap<i32, ViewerEncoder>,
    cache: &FrameCache, ms: i64, recorder: &Arc<Mutex<Option<Recorder>>>,
    record_owner: Option<i32>, capture_ms: u32,
) -> ResultType<()> {
    let Some(frame) = &cache.frame else { return Ok(()); };
    for (&id, viewer) in viewers.iter_mut() {
        let new_frame = viewer.frame_generation != cache.generation;
        if !viewer.ready() || !(new_frame || viewer.first_frame
            || (!viewer.encoder.latency_free() && viewer.repeat < 10)) { continue; }
        let encode_begin = Instant::now();
        let input = match frame {
            CachedFrame::Pixels(pixels) => {
                if new_frame || viewer.yuv.is_empty() {
                    scrap::scaled_to_yuv(pixels, viewer.width, viewer.height,
                        viewer.encoder.yuvfmt(), &mut viewer.yuv, &mut viewer.mid,
                        &mut viewer.scaled)?;
                }
                EncodeInput::YUV(&viewer.yuv)
            }
            #[cfg(windows)]
            CachedFrame::Texture(texture, rotation) => EncodeInput::Texture((texture.as_ptr(), *rotation)),
        };
        viewer.repeat = if new_frame { 0 } else { viewer.repeat + 1 };
        viewer.frame_generation = cache.generation;
        let sent = handle_one_frame(display, sp, id, input, ms,
            &mut viewer.encoder, if record_owner == Some(id) { recorder.clone() } else { Default::default() },
            &mut viewer.failures, &mut viewer.first_frame, viewer.width, viewer.height)?;
        let mut registry = VIDEO_QOS.lock().unwrap();
        let qos = registry.peer(id);
        qos.capture_ms = capture_ms;
        qos.encode_ms = encode_begin.elapsed().as_millis() as u32;
        qos.stream_size = (viewer.width as u32, viewer.height as u32);
        drop(registry);
        if !sent.is_empty() { viewer.sent(); }
    }
    Ok(())
}

fn drain_frame_acks(display: usize, viewers: &mut HashMap<i32, ViewerEncoder>) {
    let pending = FRAME_PENDING.lock().unwrap();
    for (&id, viewer) in viewers.iter_mut() {
        if !pending.contains_key(&(id, display)) {
            viewer.pending = None;
        }
    }
}

fn stream_dimensions(width: usize, height: usize, mut divisor: u32) -> (usize, usize) {
    while divisor > 1 && (width / (divisor as usize) < 640 || height / (divisor as usize) < 360) {
        divisor /= 2;
    }
    if divisor <= 1 { (width, height) }
    else { (width / divisor as usize / 2 * 2, height / divisor as usize / 2 * 2) }
}

fn prepare_viewers(
    viewers: &mut HashMap<i32, ViewerEncoder>, sp: &GenericService, c: &CapturerInfo,
    source: VideoSource, portable: bool, record_incoming: bool, texture_mode: &mut bool,
) -> ResultType<()> {
    let ids = sp.subscriber_ids();
    viewers.retain(|id, _| ids.contains(id));
    let name = sp.name();
    let settings: Vec<_> = {
        let mut registry = VIDEO_QOS.lock().unwrap();
        ids.iter().map(|&id| {
            let qos = registry.peer(id);
            if !viewers.contains_key(&id) { qos.new_display(name.clone()); }
            let divisor = if record_incoming { 1 } else { qos.resolution_divisor() };
            (id, stream_dimensions(c.width, c.height, divisor), qos.ratio(), qos.spf(), qos.record())
        }).collect()
    };
    let allow_texture = settings.iter().all(|s| s.1 == (c.width, c.height));
    for (id, (width, height), ratio, spf, record) in settings {
        // Do not append frames or reset an encoder while its previous frame is in flight.
        if viewers.get(&id).map_or(false, |v| v.pending.is_some()) {
            if let Some(v) = viewers.get_mut(&id) {
                if v.second.elapsed() >= Duration::from_secs(1) {
                    VIDEO_QOS.lock().unwrap().peer(id).update_display_data(&name, v.count);
                    v.count = 0;
                    v.second = Instant::now();
                }
            }
            continue;
        }
        // A capture restart must not append another frame to a blocked socket.
        if FRAME_PENDING.lock().unwrap().contains_key(&(id, c.current)) { continue; }
        let refresh = format!("refresh-{id}");
        let reset = viewers.get(&id).map_or(true, |v| {
            v.width != width || v.height != height || v.codec != Encoder::negotiated_codec()
                || v.use_i444 != Encoder::use_i444(&v.config) || v.record != record || sp.is_option_true(&refresh)
                || (v.input_texture() && !allow_texture)
                || (!v.encoder.support_changing_quality() && v.ratio != ratio)
        });
        if reset {
            let cfg = get_encoder_config(c, name.clone(), ratio, record || record_incoming,
                portable, source, width, height, allow_texture);
            let use_i444 = Encoder::use_i444(&cfg);
            Encoder::set_fallback(&cfg);
            let encoder = match Encoder::new(cfg.clone(), use_i444) {
                Ok(e) => e,
                Err(e) => {
                    log::error!("Viewer {id} encoder setup failed: {e}; falling back to VP9");
                    Encoder::set_fallback(&EncoderCfg::VPX(VpxEncoderConfig {
                        width: width as _, height: height as _, quality: ratio,
                        codec: VpxVideoCodecId::VP9, keyframe_interval: None,
                    }));
                    bail!("SWITCH");
                }
            };
            #[cfg(target_os = "android")]
            if source.is_monitor() {
                if let Err(e) = check_change_scale(encoder.is_hardware()) {
                    try_broadcast_display_changed(sp, c.current, c, true)?;
                    bail!(e);
                }
            }
            viewers.insert(id, ViewerEncoder {
                encoder, config: cfg, codec: Encoder::negotiated_codec(), use_i444, width, height, ratio, record,
                spf, pending: None, last_sent: None, second: Instant::now(), count: 0, failures: 0,
                first_frame: true, repeat: 0, frame_generation: 0, yuv: Vec::new(), mid: Vec::new(), scaled: Vec::new(),
            });
            sp.set_option_bool(&refresh, false);
        }
        if let Some(v) = viewers.get_mut(&id) {
            v.spf = spf;
            if v.ratio != ratio && v.encoder.support_changing_quality() {
                v.encoder.set_quality(ratio)?;
                v.ratio = ratio;
            }
            let mut registry = VIDEO_QOS.lock().unwrap();
            let qos = registry.peer(id);
            qos.store_bitrate(v.encoder.bitrate());
            qos.set_support_changing_quality(&name, v.encoder.support_changing_quality());
            if v.second.elapsed() >= Duration::from_secs(1) {
                qos.update_display_data(&name, v.count);
                v.count = 0;
                v.second = Instant::now();
            }
        }
    }
    *texture_mode = !viewers.is_empty() && viewers.values().all(ViewerEncoder::input_texture);
    Ok(())
}

fn run(vs: VideoService) -> ResultType<()> {
    let mut _raii = Raii::new(vs.sp.name());
    // Wayland only support one video capturer for now. It is ok to call ensure_inited() here.
    //
    // ensure_inited() is needed because clear() may be called.
    // to-do: wayland ensure_inited should pass current display index.
    // But for now, we do not support multi-screen capture on wayland.
    #[cfg(target_os = "linux")]
    super::wayland::ensure_inited()?;
    #[cfg(target_os = "linux")]
    let _wayland_call_on_ret = {
        // Increment active display count when starting
        let _display_count = super::wayland::increment_active_display_count();

        SimpleCallOnReturn {
            b: true,
            f: Box::new(|| {
                // Decrement active display count and only clear if this was the last display
                let remaining_count = super::wayland::decrement_active_display_count();
                if remaining_count == 0 {
                    super::wayland::clear();
                }
            }),
        }
    };

    #[cfg(windows)]
    let last_portable_service_running = crate::portable_service::client::running();
    #[cfg(not(windows))]
    let last_portable_service_running = false;

    let display_idx = vs.idx;
    let sp = vs.sp;
    let mut c = get_capturer(vs.source, display_idx, last_portable_service_running)?;
    #[cfg(windows)]
    if !scrap::codec::enable_directx_capture() && !c.is_gdi() {
        log::info!("disable dxgi with option, fall back to gdi");
        c.set_gdi();
    }
    let record_incoming = config::option2bool(
        "allow-auto-record-incoming", &Config::get_option("allow-auto-record-incoming"));
    let mut recorder: Arc<Mutex<Option<Recorder>>> = Default::default();
    let mut record_owner = None;
    let mut viewers: HashMap<i32, ViewerEncoder> = HashMap::new();
    let mut latest_frame = FrameCache::default();
    let mut capture_ms = 0;
    let mut texture_mode = false;
    sp.set_option_bool(OPTION_REFRESH, false);

    let start = time::Instant::now();
    let mut last_check_displays = time::Instant::now();
    #[cfg(windows)]
    let mut try_gdi = 1;
    #[cfg(windows)]
    log::info!("gdi: {}", c.is_gdi());
    #[cfg(windows)]
    start_uac_elevation_check();

    #[cfg(target_os = "linux")]
    let mut would_block_count = 0u32;
    let mut last_frame = Instant::now();

    while sp.ok() {
        #[cfg(windows)]
        check_uac_switch(c.privacy_mode_id, c._capturer_privacy_mode_id)?;
        drain_frame_acks(display_idx, &mut viewers);
        prepare_viewers(&mut viewers, &sp, &c, vs.source, last_portable_service_running,
            record_incoming, &mut texture_mode)?;
        let next_owner = viewers.keys().copied().min();
        if record_incoming && record_owner != next_owner {
            recorder = get_recorder(true, display_idx, vs.source == VideoSource::Camera);
            record_owner = next_owner;
        }
        #[cfg(feature = "vram")]
        c.set_output_texture(texture_mode);
        let spf = viewers.values().map(|v| v.spf).min().unwrap_or(Duration::from_millis(33));
        if sp.is_option_true(OPTION_REFRESH) {
            if vs.source.is_monitor() {
                let _ = try_broadcast_display_changed(&sp, display_idx, &c, true);
            }
            log::info!("switch to refresh");
            bail!("SWITCH");
        }
        #[cfg(windows)]
        if last_portable_service_running != crate::portable_service::client::running() {
            log::info!("switch due to portable service running changed");
            bail!("SWITCH");
        }
        #[cfg(all(windows, feature = "vram"))]
        if c.is_gdi() && texture_mode {
            VRamEncoder::set_fallback_gdi(sp.name(), true);
            bail!("SWITCH");
        }
        if vs.source.is_monitor() {
            check_privacy_mode_changed(&sp, display_idx, &c)?;
        }
        #[cfg(windows)]
        {
            if crate::platform::windows::desktop_changed()
                && !crate::portable_service::client::running()
            {
                bail!("Desktop changed");
            }
        }
        let now = time::Instant::now();
        if vs.source.is_monitor() && last_check_displays.elapsed().as_millis() > 1000 {
            last_check_displays = now;
            // This check may be redundant, but it is better to be safe.
            // The previous check in `sp.is_option_true(OPTION_REFRESH)` block may be enough.
            try_broadcast_display_changed(&sp, display_idx, &c, false)?;
        }

        if !viewers.values().any(ViewerEncoder::ready) {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }

        let time = now - start;
        let ms = (time.as_secs() * 1000 + time.subsec_millis() as u64) as i64;
        let capture_begin = Instant::now();
        let res = match c.frame(spf) {
            Ok(frame) => {
                capture_ms = capture_begin.elapsed().as_millis() as u32;
                last_frame = Instant::now();
                if frame.valid() {
                    let screenshot_key = (vs.source, display_idx);
                    let screenshot = SCREENSHOTS.lock().unwrap().remove(&screenshot_key);
                    if let Some(mut screenshot) = screenshot {
                        let restore_vram = screenshot.restore_vram;
                        let (msg, w, h, data) = match &frame {
                            scrap::Frame::PixelBuffer(f) => match get_rgba_from_pixelbuf(f) {
                                Ok(rgba) => ("".to_owned(), f.width(), f.height(), rgba),
                                Err(e) => {
                                    let serr = e.to_string();
                                    log::error!(
                                        "Failed to convert the pix format into rgba, {}",
                                        &serr
                                    );
                                    (format!("Convert pixfmt: {}", serr), 0, 0, vec![])
                                }
                            },
                            scrap::Frame::Texture(_) => {
                                if restore_vram {
                                    // Already set one time, just ignore to break infinite loop.
                                    // Though it's unreachable, this branch is kept to avoid infinite loop.
                                    (
                                        "Please change codec and try again.".to_owned(),
                                        0,
                                        0,
                                        vec![],
                                    )
                                } else {
                                    #[cfg(all(windows, feature = "vram"))]
                                    VRamEncoder::set_not_use(sp.name(), true);
                                    screenshot.restore_vram = true;
                                    SCREENSHOTS
                                        .lock()
                                        .unwrap()
                                        .insert(screenshot_key, screenshot);
                                    _raii.try_vram = false;
                                    bail!("SWITCH");
                                }
                            }
                        };
                        std::thread::spawn(move || {
                            handle_screenshot(screenshot, msg, w, h, data);
                        });
                        if restore_vram {
                            bail!("SWITCH");
                        }
                    }

                    latest_frame.update(&frame)?;
                    send_cached_frames(display_idx, &sp, &mut viewers, &latest_frame, ms,
                        &recorder, record_owner, capture_ms)?;
                }
                #[cfg(windows)]
                {
                    #[cfg(feature = "vram")]
                    if try_gdi == 1 && !c.is_gdi() {
                        VRamEncoder::set_fallback_gdi(sp.name(), false);
                    }
                    try_gdi = 0;
                }
                Ok(())
            }
            Err(err) => Err(err),
        };

        match res {
            Err(ref e) if e.kind() == WouldBlock => {
                if last_frame.elapsed() > Duration::from_secs(1)
                    && latest_frame.frame.is_none()
                    && viewers.values().any(|v| v.first_frame) {
                    // Ask the capture backend for its initial image on a static desktop.
                    // This happens only on a new stream with no cached pixels.
                    bail!("SWITCH");
                }
                #[cfg(windows)]
                if try_gdi > 0 && !c.is_gdi() {
                    if try_gdi > 3 {
                        c.set_gdi();
                        try_gdi = 0;
                        log::info!("No image, fall back to gdi");
                    }
                    try_gdi += 1;
                }
                #[cfg(target_os = "linux")]
                {
                    would_block_count += 1;
                    if !is_x11() {
                        if would_block_count >= 100 {
                            // to-do: Unknown reason for WouldBlock 100 times (seconds = 100 * 1 / fps)
                            // https://github.com/rustdesk/rustdesk/blob/63e6b2f8ab51743e77a151e2b7ff18816f5fa2fb/libs/scrap/src/common/wayland.rs#L81
                            //
                            // Do not reset the capturer for now, as it will cause the prompt to show every few minutes.
                            // https://github.com/rustdesk/rustdesk/issues/4276
                            //
                            // super::wayland::clear();
                            // bail!("Wayland capturer none 100 times, try restart capture");
                        }
                    }
                }
                send_cached_frames(display_idx, &sp, &mut viewers, &latest_frame, ms,
                    &recorder, record_owner, capture_ms)?;
            }
            Err(err) => {
                // This check may be redundant, but it is better to be safe.
                // The previous check in `sp.is_option_true(OPTION_REFRESH)` block may be enough.
                if vs.source.is_monitor() {
                    try_broadcast_display_changed(&sp, display_idx, &c, true)?;
                }

                #[cfg(windows)]
                if !c.is_gdi() {
                    c.set_gdi();
                    log::info!("dxgi error, fall back to gdi: {:?}", err);
                    continue;
                }
                return Err(err.into());
            }
            _ => {
                #[cfg(target_os = "linux")]
                {
                    would_block_count = 0;
                }
            }
        }

        let elapsed = now.elapsed();
        // may need to enable frame(timeout)
        log::trace!("{:?} {:?}", time::Instant::now(), elapsed);
        if elapsed < spf {
            std::thread::sleep(spf - elapsed);
        }
    }

    Ok(())
}

struct Raii {
    name: String,
    try_vram: bool,
}

impl Raii {
    fn new(name: String) -> Self {
        log::info!("new video service: {}", name);
        Raii {
            name,
            try_vram: true,
        }
    }
}

impl Drop for Raii {
    fn drop(&mut self) {
        log::info!("stop video service: {}", self.name);
        #[cfg(feature = "vram")]
        if self.try_vram {
            VRamEncoder::set_not_use(self.name.clone(), false);
        }
        #[cfg(feature = "vram")]
        Encoder::update(scrap::codec::EncodingUpdate::Check);
        VIDEO_QOS.lock().unwrap().remove_display(&self.name);
    }
}

fn get_encoder_config(
    c: &CapturerInfo,
    _name: String,
    quality: f32,
    record: bool,
    _portable_service: bool,
    _source: VideoSource,
    width: usize,
    height: usize,
    allow_texture: bool,
) -> EncoderCfg {
    #[cfg(all(windows, feature = "vram"))]
    if _portable_service || c.is_gdi() || _source == VideoSource::Camera {
        log::info!("gdi:{}, portable:{}", c.is_gdi(), _portable_service);
        VRamEncoder::set_not_use(_name, true);
    }
    #[cfg(feature = "vram")]
    Encoder::update(scrap::codec::EncodingUpdate::Check);
    // https://www.wowza.com/community/t/the-correct-keyframe-interval-in-obs-studio/95162
    let keyframe_interval = if record { Some(240) } else { None };
    let negotiated_codec = Encoder::negotiated_codec();
    match negotiated_codec {
        CodecFormat::H264 | CodecFormat::H265 => {
            #[cfg(feature = "vram")]
            if let Some(feature) = if allow_texture { VRamEncoder::try_get(&c.device(), negotiated_codec) } else { None } {
                return EncoderCfg::VRAM(VRamEncoderConfig {
                    device: c.device(),
                    width: width,
                    height: height,
                    quality,
                    feature,
                    keyframe_interval,
                });
            }
            #[cfg(feature = "hwcodec")]
            if let Some(hw) = HwRamEncoder::try_get(negotiated_codec) {
                return EncoderCfg::HWRAM(HwRamEncoderConfig {
                    name: hw.name,
                    mc_name: hw.mc_name,
                    width: width,
                    height: height,
                    quality,
                    keyframe_interval,
                });
            }
            EncoderCfg::VPX(VpxEncoderConfig {
                width: width as _,
                height: height as _,
                quality,
                codec: VpxVideoCodecId::VP9,
                keyframe_interval,
            })
        }
        format @ (CodecFormat::VP8 | CodecFormat::VP9) => EncoderCfg::VPX(VpxEncoderConfig {
            width: width as _,
            height: height as _,
            quality,
            codec: if format == CodecFormat::VP8 {
                VpxVideoCodecId::VP8
            } else {
                VpxVideoCodecId::VP9
            },
            keyframe_interval,
        }),
        CodecFormat::AV1 => EncoderCfg::AOM(AomEncoderConfig {
            width: width as _,
            height: height as _,
            quality,
            keyframe_interval,
        }),
        _ => EncoderCfg::VPX(VpxEncoderConfig {
            width: width as _,
            height: height as _,
            quality,
            codec: VpxVideoCodecId::VP9,
            keyframe_interval,
        }),
    }
}

fn get_recorder(
    record_incoming: bool,
    display_idx: usize,
    camera: bool,
) -> Arc<Mutex<Option<Recorder>>> {
    #[cfg(windows)]
    let root = crate::platform::is_root();
    #[cfg(not(windows))]
    let root = false;
    let recorder = if record_incoming {
        Recorder::new(RecorderContext {
            server: true,
            id: hbb_common::whoami::hostname(),
            dir: crate::ui_interface::video_save_directory(root),
            display_idx,
            camera,
            tx: None,
        })
        .map_or(Default::default(), |r| Arc::new(Mutex::new(Some(r))))
    } else {
        Default::default()
    };

    recorder
}

#[cfg(target_os = "android")]
fn check_change_scale(hardware: bool) -> ResultType<()> {
    use hbb_common::config::keys::OPTION_ENABLE_ANDROID_SOFTWARE_ENCODING_HALF_SCALE as SCALE_SOFT;

    // isStart flag is set at the end of startCapture() in Android, wait it to be set.
    let n = 60; // 3s
    for i in 0..n {
        if scrap::is_start() == Some(true) {
            log::info!("start flag is set");
            break;
        }
        log::info!("wait for start, {i}");
        std::thread::sleep(Duration::from_millis(50));
        if i == n - 1 {
            log::error!("wait for start timeout");
        }
    }
    let screen_size = scrap::screen_size();
    let scale_soft = hbb_common::config::option2bool(SCALE_SOFT, &Config::get_option(SCALE_SOFT));
    let half_scale = !hardware && scale_soft;
    log::info!("hardware: {hardware}, scale_soft: {scale_soft}, screen_size: {screen_size:?}",);
    scrap::android::call_main_service_set_by_name(
        "half_scale",
        Some(half_scale.to_string().as_str()),
        None,
    )
    .ok();
    let old_scale = screen_size.2;
    let new_scale = scrap::screen_size().2;
    log::info!("old_scale: {old_scale}, new_scale: {new_scale}");
    if old_scale != new_scale {
        log::info!("switch due to scale changed, {old_scale} -> {new_scale}");
        // switch is not a must, but it is better to do so.
        bail!("SWITCH");
    }
    Ok(())
}

fn check_privacy_mode_changed(
    sp: &GenericService,
    display_idx: usize,
    ci: &CapturerInfo,
) -> ResultType<()> {
    let privacy_mode_id_2 = get_privacy_mode_conn_id().unwrap_or(INVALID_PRIVACY_MODE_CONN_ID);
    if ci.privacy_mode_id != privacy_mode_id_2 {
        if privacy_mode_id_2 != INVALID_PRIVACY_MODE_CONN_ID {
            let msg_out = crate::common::make_privacy_mode_msg(
                back_notification::PrivacyModeState::PrvOnByOther,
                "".to_owned(),
            );
            sp.send_to_others(msg_out, privacy_mode_id_2);
        }
        log::info!("switch due to privacy mode changed");
        try_broadcast_display_changed(&sp, display_idx, ci, true).ok();
        bail!("SWITCH");
    }
    Ok(())
}

#[inline]
fn handle_one_frame(
    display: usize,
    sp: &GenericService,
    id: i32,
    frame: EncodeInput,
    ms: i64,
    encoder: &mut Encoder,
    recorder: Arc<Mutex<Option<Recorder>>>,
    encode_fail_counter: &mut usize,
    first_frame: &mut bool,
    width: usize,
    height: usize,
) -> ResultType<HashSet<i32>> {
    let mut send_conn_ids: HashSet<i32> = Default::default();
    let first = *first_frame;
    *first_frame = false;
    match encoder.encode_to_message(frame, ms) {
        Ok(mut vf) => {
            *encode_fail_counter = 0;
            vf.display = display as _;
            vf.stream_width = width as _;
            vf.stream_height = height as _;
            let mut msg = Message::new();
            msg.set_video_frame(vf);
            recorder
                .lock()
                .unwrap()
                .as_mut()
                .map(|r| r.write_message(&msg, width, height));
            FRAME_PENDING.lock().unwrap().insert((id, display), Instant::now());
            if sp.send_to(msg, id) {
                send_conn_ids.insert(id);
            } else {
                FRAME_PENDING.lock().unwrap().remove(&(id, display));
            }
        }
        Err(e) => {
            *encode_fail_counter += 1;
            // Encoding errors are not frequent except on Android
            if !cfg!(target_os = "android") {
                log::error!("encode fail: {e:?}, times: {}", *encode_fail_counter,);
            }
            let max_fail_times = if cfg!(target_os = "android") && encoder.is_hardware() {
                9
            } else {
                3
            };
            let repeat = !encoder.latency_free();
            // repeat encoders can reach max_fail_times on the first frame
            if (first && !repeat) || *encode_fail_counter >= max_fail_times {
                *encode_fail_counter = 0;
                if encoder.is_hardware() {
                    encoder.disable();
                    log::error!("switch due to encoding fails, first frame: {first}, error: {e:?}");
                    bail!("SWITCH");
                }
            }
            match e.to_string().as_str() {
                scrap::codec::ENCODE_NEED_SWITCH => {
                    encoder.disable();
                    log::error!("switch due to encoder need switch");
                    bail!("SWITCH");
                }
                _ => {}
            }
        }
    }
    Ok(send_conn_ids)
}

#[inline]
pub fn refresh() {
    #[cfg(target_os = "android")]
    Display::refresh_size();
}

#[cfg(windows)]
fn start_uac_elevation_check() {
    static START: Once = Once::new();
    START.call_once(|| {
        if !crate::platform::is_installed() && !crate::platform::is_root() {
            std::thread::spawn(|| loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if let Ok(uac) = is_process_consent_running() {
                    *IS_UAC_RUNNING.lock().unwrap() = uac;
                }
                if !crate::platform::is_elevated(None).unwrap_or(false) {
                    if let Ok(elevated) = crate::platform::is_foreground_window_elevated() {
                        *IS_FOREGROUND_WINDOW_ELEVATED.lock().unwrap() = elevated;
                    }
                }
            });
        }
    });
}

#[inline]
fn try_broadcast_display_changed(
    sp: &GenericService,
    display_idx: usize,
    cap: &CapturerInfo,
    refresh: bool,
) -> ResultType<()> {
    if refresh {
        // Get display information immediately.
        crate::display_service::check_displays_changed().ok();
    }
    if let Some(display) = check_display_changed(
        cap.ndisplay,
        cap.current,
        (cap.origin.0, cap.origin.1, cap.width, cap.height),
    ) {
        log::info!("Display {} changed", display);
        if let Some(msg_out) =
            make_display_changed_msg(display_idx, Some(display), VideoSource::Monitor)
        {
            let msg_out = Arc::new(msg_out);
            sp.send_shared(msg_out.clone());
            // switch display may occur before the first video frame, add snapshot to send to new subscribers
            sp.snapshot(move |sps| {
                sps.send_shared(msg_out.clone());
                Ok(())
            })?;
            bail!("SWITCH");
        }
    }
    Ok(())
}

pub fn make_display_changed_msg(
    display_idx: usize,
    opt_display: Option<DisplayInfo>,
    source: VideoSource,
) -> Option<Message> {
    let display = match opt_display {
        Some(d) => d,
        None => match source {
            VideoSource::Monitor => display_service::get_display_info(display_idx)?,
            VideoSource::Camera => camera::Cameras::get_sync_cameras()
                .get(display_idx)?
                .clone(),
        },
    };
    let mut misc = Misc::new();
    misc.set_switch_display(SwitchDisplay {
        display: display_idx as _,
        x: display.x,
        y: display.y,
        width: display.width,
        height: display.height,
        cursor_embedded: match source {
            VideoSource::Monitor => display_service::capture_cursor_embedded(),
            VideoSource::Camera => false,
        },
        #[cfg(not(target_os = "android"))]
        resolutions: Some(SupportedResolutions {
            resolutions: match source {
                VideoSource::Monitor => {
                    if display.name.is_empty() {
                        vec![]
                    } else {
                        crate::platform::resolutions(&display.name)
                    }
                }
                VideoSource::Camera => camera::Cameras::get_camera_resolution(display_idx)
                    .ok()
                    .into_iter()
                    .collect(),
            },
            ..SupportedResolutions::default()
        })
        .into(),
        original_resolution: display.original_resolution,
        ..Default::default()
    });
    let mut msg_out = Message::new();
    msg_out.set_misc(misc);
    Some(msg_out)
}

pub fn set_take_screenshot(source: VideoSource, display_idx: usize, sid: String, tx: Sender) {
    SCREENSHOTS.lock().unwrap().insert(
        (source, display_idx),
        Screenshot {
            sid,
            tx,
            restore_vram: false,
        },
    );
}

// We need to this function, because the `stride` may be larger than `width * 4`.
fn get_rgba_from_pixelbuf<'a>(pixbuf: &scrap::PixelBuffer<'a>) -> ResultType<Vec<u8>> {
    let w = pixbuf.width();
    let h = pixbuf.height();
    let stride = pixbuf.stride();
    let Some(s) = stride.get(0) else {
        bail!("Invalid pixel buf stride.")
    };

    if *s == w * 4 {
        let mut rgba = vec![];
        scrap::convert(pixbuf, scrap::Pixfmt::RGBA, &mut rgba)?;
        Ok(rgba)
    } else {
        let bgra = pixbuf.data();
        let mut bit_flipped = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let i = s * y + 4 * x;
                bit_flipped.extend_from_slice(&[bgra[i + 2], bgra[i + 1], bgra[i], bgra[i + 3]]);
            }
        }
        Ok(bit_flipped)
    }
}

fn handle_screenshot(screenshot: Screenshot, msg: String, w: usize, h: usize, data: Vec<u8>) {
    let mut response = ScreenshotResponse::new();
    response.sid = screenshot.sid;
    if msg.is_empty() {
        if data.is_empty() {
            response.msg = "Failed to take screenshot, please try again later.".to_owned();
        } else {
            fn encode_png(width: usize, height: usize, rgba: Vec<u8>) -> ResultType<Vec<u8>> {
                let mut png = Vec::new();
                let mut encoder =
                    repng::Options::smallest(width as _, height as _).build(&mut png)?;
                encoder.write(&rgba)?;
                encoder.finish()?;
                Ok(png)
            }
            match encode_png(w as _, h as _, data) {
                Ok(png) => {
                    response.data = png.into();
                }
                Err(e) => {
                    response.msg = format!("Error encoding png: {}", e);
                }
            }
        }
    } else {
        response.msg = msg;
    }
    let mut msg_out = Message::new();
    msg_out.set_screenshot_response(response);
    if let Err(e) = screenshot
        .tx
        .send((hbb_common::tokio::time::Instant::now(), Arc::new(msg_out)))
    {
        log::error!("Failed to send screenshot, {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_viewer_catches_up_without_another_capture() {
        struct Pixels(Vec<u8>);
        impl TraitPixelBuffer for Pixels {
            fn data(&self) -> &[u8] { &self.0 }
            fn width(&self) -> usize { 64 }
            fn height(&self) -> usize { 64 }
            fn stride(&self) -> Vec<usize> { vec![256] }
            fn pixfmt(&self) -> scrap::Pixfmt { scrap::Pixfmt::BGRA }
        }
        fn viewer() -> ViewerEncoder {
            let config = EncoderCfg::VPX(VpxEncoderConfig {
                width: 64, height: 64, quality: 0.5,
                codec: VpxVideoCodecId::VP9, keyframe_interval: None,
            });
            ViewerEncoder {
                encoder: Encoder::new(config.clone(), false).unwrap(), config,
                codec: CodecFormat::VP9, use_i444: false, width: 64, height: 64,
                ratio: 0.5, record: false, spf: Duration::ZERO,
                pending: None, last_sent: None, second: Instant::now(), count: 0,
                failures: 0, first_frame: true, repeat: 0, frame_generation: 0,
                yuv: vec![], mid: vec![], scaled: vec![],
            }
        }
        let display = usize::MAX - 1;
        let sp = GenericService::new("cache-test".into(), false);
        let (fast_tx, mut fast_rx) = hbb_common::tokio::sync::mpsc::unbounded_channel();
        let (slow_tx, mut slow_rx) = hbb_common::tokio::sync::mpsc::unbounded_channel();
        sp.on_subscribe(ConnInner::new(-101, None, Some(fast_tx)));
        sp.on_subscribe(ConnInner::new(-102, None, Some(slow_tx)));
        let mut viewers = HashMap::from([(-101, viewer()), (-102, viewer())]);
        let recorder = Default::default();
        let mut cache = FrameCache::default();
        let mut pixels = Pixels(vec![0; 64 * 64 * 4]);
        cache.update_pixels(&pixels);
        send_cached_frames(display, &sp, &mut viewers, &cache, 0, &recorder, None, 0).unwrap();
        fast_rx.try_recv().unwrap();
        slow_rx.try_recv().unwrap();
        // Only the fast viewer acknowledges the initial image. Subsequent
        // captures replace the shared cache while the slow viewer stays blocked.
        for value in [80, 160] {
            notify_video_frame_fetched(display, -101, None);
            drain_frame_acks(display, &mut viewers);
            pixels.0.fill(value);
            cache.update_pixels(&pixels);
            send_cached_frames(display, &sp, &mut viewers, &cache, value as _, &recorder, None, 0).unwrap();
            fast_rx.try_recv().unwrap();
            assert!(slow_rx.try_recv().is_err());
        }
        pixels.0.fill(0); // The cache must own its pixels.
        let CachedFrame::Pixels(saved) = cache.frame.as_ref().unwrap() else { panic!("pixels"); };
        assert_eq!(saved.data()[0], 160);
        // The desktop is now idle: acknowledge without providing another frame.
        notify_video_frame_fetched(display, -101, None);
        notify_video_frame_fetched(display, -102, None);
        drain_frame_acks(display, &mut viewers);
        send_cached_frames(display, &sp, &mut viewers, &cache, 200, &recorder, None, 0).unwrap();
        assert!(fast_rx.try_recv().is_err());
        let (_, msg) = slow_rx.try_recv().unwrap();
        assert!(matches!(&msg.union, Some(message::Union::VideoFrame(_))));
        assert_eq!((msg.video_frame().stream_width, msg.video_frame().stream_height), (64, 64));
        assert_eq!(viewers[&-102].frame_generation, cache.generation);
        assert_eq!(viewers[&-102].count, 2);
        // An encoder rebuilt for refresh also gets the cached image immediately.
        notify_video_frame_fetched(display, -102, None);
        viewers.insert(-102, viewer());
        send_cached_frames(display, &sp, &mut viewers, &cache, 250, &recorder, None, 0).unwrap();
        slow_rx.try_recv().unwrap();
        for id in [-101, -102] {
            notify_video_frame_fetched(display, id, None);
            VIDEO_QOS.lock().unwrap().on_connection_close(id);
        }
    }

    #[test]
    fn frame_notifications_are_bounded_and_scoped_to_viewer_and_display() {
        let display = usize::MAX;
        let before = Instant::now();
        let started = before + Duration::from_millis(1);
        FRAME_PENDING.lock().unwrap().insert((-1, display), started);
        FRAME_PENDING.lock().unwrap().insert((-2, display), started);
        notify_video_frame_fetched(display, -1, Some(before));
        assert!(FRAME_PENDING.lock().unwrap().contains_key(&(-1, display)));
        for _ in 0..100 { notify_video_frame_fetched(display, -1, None); }
        assert!(!FRAME_PENDING.lock().unwrap().contains_key(&(-1, display)));
        assert!(FRAME_PENDING.lock().unwrap().remove(&(-2, display)).is_some());
        VIDEO_QOS.lock().unwrap().on_connection_close(-1);
    }

    #[test]
    fn adaptive_size_preserves_native_space_and_minimum_readability() {
        assert_eq!(stream_dimensions(1920, 1080, 1), (1920, 1080));
        assert_eq!(stream_dimensions(1920, 1080, 4), (960, 540));
        assert_eq!(stream_dimensions(3840, 2160, 4), (960, 540));
        assert_eq!(stream_dimensions(800, 600, 4), (800, 600));
    }

    #[test]
    fn scaled_vp9_stream_decodes_at_transmitted_size_with_padded_capture_stride() {
        use scrap::{GoogleImage, Pixfmt, vpxcodec::{VpxDecoder, VpxDecoderConfig}};
        struct Pixels(Vec<u8>);
        impl TraitPixelBuffer for Pixels {
            fn data(&self) -> &[u8] { &self.0 }
            fn width(&self) -> usize { 1280 }
            fn height(&self) -> usize { 720 }
            fn stride(&self) -> Vec<usize> { vec![1280 * 4 + 64] }
            fn pixfmt(&self) -> Pixfmt { Pixfmt::BGRA }
        }
        let mut pixels = Pixels(vec![0; (1280 * 4 + 64) * 720]);
        for y in 0..720 { for x in 0..1280 {
            let i = y * (1280 * 4 + 64) + x * 4;
            pixels.0[i..i+4].copy_from_slice(&[(x % 256) as u8, (y % 256) as u8, 80, 255]);
        }}
        let mut encoder = Encoder::new(EncoderCfg::VPX(VpxEncoderConfig {
            width: 640, height: 360, quality: 0.5, codec: VpxVideoCodecId::VP9,
            keyframe_interval: None,
        }), false).unwrap();
        let (mut yuv, mut mid, mut scaled) = (vec![], vec![], vec![]);
        scrap::scaled_to_yuv(&pixels, 640, 360, encoder.yuvfmt(), &mut yuv, &mut mid, &mut scaled).unwrap();
        let frame = encoder.encode_to_message(EncodeInput::YUV(&yuv), 0).unwrap();
        let video_frame::Union::Vp9s(frames) = frame.union.unwrap() else { panic!("Expected VP9"); };
        assert!(frames.frames[0].key);
        let mut decoder = VpxDecoder::new(VpxDecoderConfig { codec: VpxVideoCodecId::VP9 }).unwrap();
        let decoded = decoder.decode(&frames.frames[0].data).unwrap().next().unwrap();
        assert_eq!((decoded.width(), decoded.height()), (640, 360));
        pixels.0.truncate(10);
        assert!(scrap::scaled_to_yuv(&pixels, 640, 360, encoder.yuvfmt(), &mut yuv, &mut mid, &mut scaled).is_err());
    }

    #[test]
    fn adaptive_stream_resize_keeps_full_desktop_content() {
        use scrap::Pixfmt;
        struct Pixels(Vec<u8>);
        impl TraitPixelBuffer for Pixels {
            fn data(&self) -> &[u8] { &self.0 }
            fn width(&self) -> usize { 1280 }
            fn height(&self) -> usize { 720 }
            fn stride(&self) -> Vec<usize> { vec![1280 * 4] }
            fn pixfmt(&self) -> Pixfmt { Pixfmt::BGRA }
        }
        let mut pixels = Pixels(vec![255; 1280 * 720 * 4]);
        // White desktop with a black upper-left quadrant exposes an accidental
        // crop/padded small picture when switching down and back up.
        for y in 0..360 { for x in 0..640 {
            pixels.0[(y * 1280 + x) * 4..(y * 1280 + x) * 4 + 3].fill(0);
        }}
        let mut handler = crate::client::VideoHandler::new(CodecFormat::VP9, 0);
        let mut pixelbuffer = true;
        let mut chroma = None;
        let (mut yuv, mut mid, mut scaled) = (vec![], vec![], vec![]);
        for (width, height) in [(1280, 720), (640, 360), (1280, 720), (640, 360)] {
            let mut encoder = Encoder::new(EncoderCfg::VPX(VpxEncoderConfig {
                width: width as _, height: height as _, quality: 0.5,
                codec: VpxVideoCodecId::VP9, keyframe_interval: None,
            }), false).unwrap();
            scrap::scaled_to_yuv(&pixels, width, height, encoder.yuvfmt(),
                &mut yuv, &mut mid, &mut scaled).unwrap();
            let mut frame = encoder.encode_to_message(EncodeInput::YUV(&yuv), 0).unwrap();
            frame.stream_width = width as _;
            frame.stream_height = height as _;
            assert!(handler.handle_frame(frame, &mut pixelbuffer, &mut chroma).unwrap());
            assert_eq!((handler.rgb.w, handler.rgb.h), (width, height));
            let at = |x, y| handler.rgb.raw[(y * width + x) * 4];
            assert!(at(width / 4, height / 4) < 20);
            assert!(at(width * 3 / 4, height / 4) > 235);
            assert!(at(width / 4, height * 3 / 4) > 235);
            assert!(at(width * 3 / 4, height * 3 / 4) > 235);
        }
    }
}
