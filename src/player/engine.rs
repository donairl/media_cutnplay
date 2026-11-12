use crate::core::error::MediaError;
use crate::player::audio::{AudioBackend, AudioOutput};
use crate::player::hwaccel;
use crate::player::timeline::{DeviceFormat, Timeline};
use ffmpeg_next::format::context::Input;
use ffmpeg_next::format::{sample, Pixel, Sample};
use ffmpeg_next::software::resampling::Context as Resampler;
use ffmpeg_next::software::scaling::{Context as Scaler, Flags};
use ffmpeg_next::util::frame::audio::Audio as AudioFrame;
use ffmpeg_next::util::frame::video::Video as RawVideoFrame;
use ffmpeg_next::ChannelLayout;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Decoded frames kept ready ahead of the clock.
const VIDEO_TARGET: usize = 4;
/// Hard cap on queued frames (only reached on badly interleaved files).
const VIDEO_MAX: usize = 12;
/// Keep at least this much audio buffered, even if the video queue is full.
const AUDIO_LOW_SECS: f64 = 0.25;
const AUDIO_MAX_SECS: f64 = 4.0;
/// A frame is shown once the clock is within this of its PTS.
const PRESENT_SLACK: f64 = 0.004;
/// Tolerance when matching a frame against a seek target.
const SEEK_EPSILON: f64 = 0.001;

/// A decoded picture, tightly packed RGBA.
#[derive(Clone)]
pub struct VideoFrame {
    /// Unique per decoded frame; lets the renderer skip redundant uploads.
    pub id: u64,
    pub pts: f64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for VideoFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoFrame")
            .field("id", &self.id)
            .field("pts", &self.pts)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    /// A frame that should be shown right away (after load / seek) is queued.
    FrameReady,
    /// Playback reached the end of the media.
    Ended,
    /// What is decoding the video ("NVDEC (CUDA)", "VAAPI", "Software").
    Decoder(&'static str),
    Error(String),
}

enum Command {
    Load(PathBuf),
    Close,
    Play,
    Pause,
    Seek(f64),
}

struct QueuedFrame {
    frame: Arc<VideoFrame>,
    /// Show regardless of the clock (first frame after a load / seek).
    immediate: bool,
}

struct Shared {
    timeline: Arc<Timeline>,
    frames: Mutex<VecDeque<QueuedFrame>>,
    ended: AtomicBool,
}

impl Shared {
    fn frames(&self) -> std::sync::MutexGuard<'_, VecDeque<QueuedFrame>> {
        self.frames.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Handle to the playback thread. Cheap to clone; the thread exits when the
/// last handle is dropped.
#[derive(Clone)]
pub struct Player {
    tx: mpsc::Sender<Command>,
    shared: Arc<Shared>,
}

impl Player {
    /// Starts the playback thread. `notify` is called from that thread.
    pub fn spawn(backend: AudioBackend, notify: impl Fn(PlayerEvent) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            timeline: Arc::new(Timeline::new()),
            frames: Mutex::new(VecDeque::new()),
            ended: AtomicBool::new(false),
        });

        let thread_shared = shared.clone();
        std::thread::Builder::new()
            .name("player".into())
            .spawn(move || run(rx, thread_shared, backend, notify))
            .expect("failed to spawn player thread");

        Self { tx, shared }
    }

    /// Opens a file, paused at its first frame.
    pub fn load(&self, path: PathBuf) {
        let _ = self.tx.send(Command::Load(path));
    }

    pub fn close(&self) {
        let _ = self.tx.send(Command::Close);
    }

    pub fn play(&self) {
        let _ = self.tx.send(Command::Play);
    }

    pub fn pause(&self) {
        let _ = self.tx.send(Command::Pause);
    }

    /// Frame-accurate seek. Rapid successive seeks are coalesced.
    pub fn seek(&self, secs: f64) {
        let _ = self.tx.send(Command::Seek(secs));
    }

    /// Current media time in seconds, driven by the audio device.
    pub fn position(&self) -> f64 {
        self.shared.timeline.position()
    }

    pub fn is_playing(&self) -> bool {
        self.shared.timeline.is_playing()
    }

    pub fn has_ended(&self) -> bool {
        self.shared.ended.load(Ordering::Relaxed)
    }

    /// Real audio samples handed to the output so far (diagnostics).
    pub fn audio_samples_played(&self) -> u64 {
        self.shared.timeline.delivered_samples()
    }

    /// Returns the frame that should be on screen now, if it changed.
    /// Call on every display refresh; frames that are already late are skipped.
    pub fn poll_frame(&self) -> Option<Arc<VideoFrame>> {
        let now = self.shared.timeline.position();
        let mut queue = self.shared.frames();
        let mut due = None;
        while let Some(front) = queue.front() {
            if front.immediate || front.frame.pts <= now + PRESENT_SLACK {
                due = queue.pop_front().map(|q| q.frame);
            } else {
                break;
            }
        }
        due
    }
}

fn run(
    rx: mpsc::Receiver<Command>,
    shared: Arc<Shared>,
    backend: AudioBackend,
    notify: impl Fn(PlayerEvent),
) {
    // The output stream is not `Send`; it lives and dies with this thread.
    let _audio = AudioOutput::open(backend, &shared.timeline);
    let device = shared.timeline.device_format();

    let mut media: Option<Media> = None;

    loop {
        let busy = media.as_ref().is_some_and(|m| m.wants_data(&shared));
        let first = if busy {
            match rx.try_recv() {
                Ok(cmd) => Some(cmd),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        } else {
            let wait = if shared.timeline.is_playing() { 5 } else { 100 };
            match rx.recv_timeout(Duration::from_millis(wait)) {
                Ok(cmd) => Some(cmd),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        };

        // Apply everything queued; only the most recent seek is executed.
        let mut seek = None;
        for cmd in first.into_iter().chain(rx.try_iter()) {
            match cmd {
                Command::Load(path) => {
                    seek = None;
                    clear(&shared, 0.0);
                    shared.timeline.set_playing(false);
                    media = match Media::open(&path, device) {
                        Ok(m) => Some(m),
                        Err(err) => {
                            notify(PlayerEvent::Error(err.to_string()));
                            None
                        }
                    };
                    if media.as_ref().is_some_and(|m| m.video.is_none()) {
                        shared.timeline.release();
                    }
                }
                Command::Close => {
                    seek = None;
                    media = None;
                    clear(&shared, 0.0);
                    shared.timeline.set_playing(false);
                }
                Command::Play => {
                    if media.is_some() {
                        shared.timeline.set_playing(true);
                    }
                }
                Command::Pause => shared.timeline.set_playing(false),
                Command::Seek(secs) => seek = Some(secs),
            }
        }

        let Some(m) = media.as_mut() else { continue };

        if let Some(secs) = seek {
            m.seek(secs);
            clear(&shared, secs);
            if m.video.is_none() {
                shared.timeline.release();
            }
        }

        if m.wants_data(&shared) {
            m.pump(&shared, &notify);
        } else if m.eof
            && shared.timeline.is_playing()
            && shared.frames().is_empty()
            && shared.timeline.buffered_secs() == 0.0
        {
            shared.timeline.set_playing(false);
            shared.ended.store(true, Ordering::Relaxed);
            notify(PlayerEvent::Ended);
        }
    }
}

fn clear(shared: &Shared, pts: f64) {
    shared.frames().clear();
    shared.timeline.reset(pts);
    shared.ended.store(false, Ordering::Relaxed);
}

struct VideoTrack {
    index: usize,
    decoder: ffmpeg_next::decoder::Video,
    scaler: Option<Scaler>,
    time_base: f64,
    decoded: RawVideoFrame,
    /// Last frame decoded before the seek target (shown if nothing follows).
    held: RawVideoFrame,
    has_held: bool,
    /// Frames before this time are decode-only pre-roll after a seek.
    skip_until: Option<f64>,
    /// No frame has been emitted since the last load / seek.
    awaiting_first: bool,
    last_pts: f64,
    next_id: u64,
    /// Hardware backend the decoder was opened with, if any.
    hw: Option<hwaccel::HwKind>,
    /// Decoder label last sent to the UI.
    reported: Option<&'static str>,
}

struct AudioTrack {
    index: usize,
    decoder: ffmpeg_next::decoder::Audio,
    resampler: Option<Resampler>,
    time_base: f64,
    device: DeviceFormat,
    decoded: AudioFrame,
    next_pts: f64,
    samples: Vec<f32>,
}

struct Media {
    ictx: Input,
    video: Option<VideoTrack>,
    audio: Option<AudioTrack>,
    eof: bool,
}

impl Media {
    fn open(path: &Path, device: Option<DeviceFormat>) -> Result<Self, MediaError> {
        let ictx = ffmpeg_next::format::input(path)?;

        let video = ictx
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .and_then(|stream| {
                let (decoder, hw) = open_video_decoder(&stream)?;
                Some(VideoTrack {
                    index: stream.index(),
                    decoder,
                    scaler: None,
                    time_base: f64::from(stream.time_base()),
                    decoded: RawVideoFrame::empty(),
                    held: RawVideoFrame::empty(),
                    has_held: false,
                    skip_until: None,
                    awaiting_first: true,
                    last_pts: 0.0,
                    next_id: 0,
                    hw,
                    reported: None,
                })
            });

        // Without an output device there is nobody to consume audio.
        let audio = device.and_then(|device| {
            let stream = ictx.streams().best(ffmpeg_next::media::Type::Audio)?;
            let ctx =
                ffmpeg_next::codec::context::Context::from_parameters(stream.parameters()).ok()?;
            Some(AudioTrack {
                index: stream.index(),
                decoder: ctx.decoder().audio().ok()?,
                resampler: None,
                time_base: f64::from(stream.time_base()),
                device,
                decoded: AudioFrame::empty(),
                next_pts: 0.0,
                samples: Vec::new(),
            })
        });

        if video.is_none() && audio.is_none() {
            return Err(MediaError::NoVideoStream);
        }

        Ok(Self {
            ictx,
            video,
            audio,
            eof: false,
        })
    }

    /// Whether the decoder should produce more right now.
    fn wants_data(&self, shared: &Shared) -> bool {
        if self.eof {
            return false;
        }
        let queued = shared.frames().len();
        let buffered = shared.timeline.buffered_secs();

        let video_needs = self.video.is_some() && queued < VIDEO_TARGET;
        let audio_needs = self.audio.is_some() && buffered < AUDIO_LOW_SECS;

        (video_needs || audio_needs) && queued < VIDEO_MAX && buffered < AUDIO_MAX_SECS
    }

    fn seek(&mut self, secs: f64) {
        let ts = (secs * f64::from(ffmpeg_next::ffi::AV_TIME_BASE)) as i64;
        let _ = self.ictx.seek(ts, ..ts);
        self.eof = false;

        if let Some(v) = &mut self.video {
            v.decoder.flush();
            v.skip_until = Some(secs);
            v.has_held = false;
            v.awaiting_first = true;
        }
        if let Some(a) = &mut self.audio {
            a.decoder.flush();
            a.next_pts = secs;
        }
    }

    /// Demuxes one packet and pushes whatever it decodes to.
    fn pump(&mut self, shared: &Shared, notify: &impl Fn(PlayerEvent)) {
        match self.ictx.packets().next() {
            Some((stream, packet)) => {
                let index = stream.index();
                if let Some(v) = self.video.as_mut().filter(|v| v.index == index) {
                    let _ = v.decoder.send_packet(&packet);
                    v.drain(shared, notify);
                } else if let Some(a) = self.audio.as_mut().filter(|a| a.index == index) {
                    let _ = a.decoder.send_packet(&packet);
                    a.drain(shared);
                }
            }
            None => {
                self.eof = true;
                if let Some(v) = &mut self.video {
                    let _ = v.decoder.send_eof();
                    v.drain(shared, notify);
                    // Seek target was past the last frame: show the last one.
                    if v.awaiting_first && v.has_held {
                        v.has_held = false;
                        std::mem::swap(&mut v.decoded, &mut v.held);
                        v.emit(shared, notify);
                    }
                }
                if let Some(a) = &mut self.audio {
                    let _ = a.decoder.send_eof();
                    a.drain(shared);
                }
                shared.timeline.release();
            }
        }
    }
}

/// Opens the video decoder on the best available backend:
/// NVDEC (CUDA), then VAAPI, then plain software.
fn open_video_decoder(
    stream: &ffmpeg_next::format::stream::Stream,
) -> Option<(ffmpeg_next::decoder::Video, Option<hwaccel::HwKind>)> {
    let new_context = || {
        let mut ctx =
            ffmpeg_next::codec::context::Context::from_parameters(stream.parameters()).ok()?;
        // libavcodec defaults to one thread; let it use every core. Only
        // matters in software, including a mid-negotiation fallback to it.
        ctx.set_threading(ffmpeg_next::codec::threading::Config::kind(
            ffmpeg_next::codec::threading::Type::Frame,
        ));
        Some(ctx)
    };

    for kind in hwaccel::candidates() {
        let Some(mut ctx) = new_context() else { continue };
        let Some(codec) = hwaccel::attach(&mut ctx, kind) else { continue };
        if let Ok(decoder) = ctx.decoder().open_as(codec).and_then(|d| d.video()) {
            return Some((decoder, Some(kind)));
        }
    }

    let decoder = new_context()?.decoder().video().ok()?;
    Some((decoder, None))
}

impl VideoTrack {
    fn drain(&mut self, shared: &Shared, notify: &impl Fn(PlayerEvent)) {
        while self.decoder.receive_frame(&mut self.decoded).is_ok() {
            let pts = self
                .decoded
                .timestamp()
                .or(self.decoded.pts())
                .map_or(self.last_pts, |t| t as f64 * self.time_base);
            self.last_pts = pts;

            if let Some(target) = self.skip_until {
                if pts < target - SEEK_EPSILON {
                    std::mem::swap(&mut self.decoded, &mut self.held);
                    self.has_held = true;
                    continue;
                }
                self.skip_until = None;
            }
            self.emit(shared, notify);
        }
    }

    /// Converts `self.decoded` to RGBA and queues it for display.
    fn emit(&mut self, shared: &Shared, notify: &impl Fn(PlayerEvent)) {
        let on_gpu = hwaccel::is_hw_frame(&self.decoded);
        let Some(frame) = self.convert() else { return };

        // The backend is only certain once a frame comes out: the decoder may
        // have been opened for hardware and still settled on software.
        let label = match self.hw {
            Some(kind) if on_gpu => kind.label(),
            _ => "Software",
        };
        if self.reported != Some(label) {
            self.reported = Some(label);
            notify(PlayerEvent::Decoder(label));
        }

        let immediate = self.awaiting_first;
        self.awaiting_first = false;

        shared.frames().push_back(QueuedFrame {
            frame: Arc::new(frame),
            immediate,
        });
        if immediate {
            shared.timeline.release();
            notify(PlayerEvent::FrameReady);
        }
    }

    fn convert(&mut self) -> Option<VideoFrame> {
        // Hardware surfaces come back to system memory before conversion.
        let downloaded;
        let src = if hwaccel::is_hw_frame(&self.decoded) {
            downloaded = hwaccel::download(&self.decoded)?;
            &downloaded
        } else {
            &self.decoded
        };
        let (format, width, height) = (src.format(), src.width(), src.height());
        if width == 0 || height == 0 {
            return None;
        }

        // (Re)build the scaler if the stream changed resolution / format.
        let stale = self.scaler.as_ref().is_none_or(|s| {
            let input = s.input();
            input.format != format || input.width != width || input.height != height
        });
        if stale {
            self.scaler =
                Scaler::get(format, width, height, Pixel::RGBA, width, height, Flags::BILINEAR)
                    .ok();
        }

        let mut rgb = RawVideoFrame::empty();
        self.scaler.as_mut()?.run(src, &mut rgb).ok()?;

        let stride = rgb.stride(0);
        let row = width as usize * 4;
        let data = rgb.data(0);
        let mut rgba = Vec::with_capacity(row * height as usize);
        for y in 0..height as usize {
            rgba.extend_from_slice(data.get(y * stride..y * stride + row)?);
        }

        self.next_id += 1;
        Some(VideoFrame {
            id: self.next_id,
            pts: self.last_pts,
            width,
            height,
            rgba,
        })
    }
}

impl AudioTrack {
    fn drain(&mut self, shared: &Shared) {
        while self.decoder.receive_frame(&mut self.decoded).is_ok() {
            let pts = self
                .decoded
                .timestamp()
                .or(self.decoded.pts())
                .map_or(self.next_pts, |t| t as f64 * self.time_base);
            let in_rate = self.decoded.rate().max(1);
            self.next_pts = pts + self.decoded.samples() as f64 / f64::from(in_rate);

            if self.decoded.channel_layout().is_empty() {
                let channels = i32::from(self.decoded.channels());
                self.decoded.set_channel_layout(ChannelLayout::default(channels));
            }
            let (format, layout) = (self.decoded.format(), self.decoded.channel_layout());

            let stale = self.resampler.as_ref().is_none_or(|r| {
                let input = r.input();
                input.format != format || input.channel_layout != layout || input.rate != in_rate
            });
            if stale {
                self.resampler = Resampler::get(
                    format,
                    layout,
                    in_rate,
                    Sample::F32(sample::Type::Packed),
                    ChannelLayout::default(self.device.channels as i32),
                    self.device.rate,
                )
                .ok();
            }
            let Some(resampler) = self.resampler.as_mut() else { continue };

            let mut out = AudioFrame::empty();
            if resampler.run(&self.decoded, &mut out).is_err() {
                continue;
            }

            let bytes = out.samples() * self.device.channels * 4;
            let Some(data) = out.data(0).get(..bytes) else { continue };
            self.samples.clear();
            self.samples.extend(
                data.chunks_exact(4)
                    .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])),
            );
            shared.timeline.push_audio(pts, &self.samples);
        }
    }
}
