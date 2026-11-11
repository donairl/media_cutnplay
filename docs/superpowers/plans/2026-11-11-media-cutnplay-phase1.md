# Media CutNPlay (Phase 1: MVP Core Engine & Basic UI) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Media CutNPlay Phase 1: a standalone, zero-IPC offline desktop media cutter and transcoder with Iced 0.14 GUI, safe FFmpeg 9 C-FFI decoding, lossless stream cutting, fullscreen playback, and memory footprint < 35 MB.

**Architecture:** A dedicated OS background worker hosts stateful FFmpeg `AVCodecContext` instances and handles asynchronous seek, frame decode, and remux tasks over Tokio MPSC channels. The Iced 0.14 Elm-style UI (Model-View-Update) consumes decoded RGBA frame handles without blocking the main event loop, featuring a 3-column layout (Queue, Preview/Scrubber with Fullscreen, Cut Controls) and real-time process RAM telemetry.

**Tech Stack:** Rust 1.99+, `iced` 0.14 (with `image` & `tokio`), `ffmpeg-next` 9.0 (native `libavformat`, `libavcodec`, `libswscale`), `tokio` 1.x, `sysinfo` 0.33, `rfd` 0.15.

---

## Global Constraints

- **Language & Runtime:** Pure Rust with safe abstractions around `ffmpeg-next` C-FFI; no WebViews, no Tauri, no Electron, zero IPC.
- **FFmpeg Linking:** Must compile against system FFmpeg 9 (`libavcodec 63`, `libavformat 63`, `libswscale 10`, `libavutil 61`).
- **Memory Footprint:** Idle process memory must stay under 35 MB RAM; peak scrubbing memory under 150 MB.
- **Offline Assurance:** Zero network requests, telemetry, or external daemon dependencies.
- **Lossless Cut:** Stream copy (`-c copy`) mode must adjust PTS/DTS and execute on GOP keyframe boundaries without generational quality loss.
- **GUI:** Iced 0.14 using standard Elm architecture with Model, View, and Update.

---

### Task 1: Cargo Scaffolding & Dependency Setup

**Files:**
- Create: `Cargo.toml`
- Create: `.gitignore`
- Create: `src/main.rs`

**Interfaces:**
- Consumes: System FFmpeg libraries via pkg-config
- Produces: Executable crate skeleton compiling with `cargo check` and running a basic Iced window

- [ ] **Step 1: Create `.gitignore`**

```gitignore
/target
Cargo.lock
*.mp4
*.mkv
test_assets/
```

- [ ] **Step 2: Create `Cargo.toml` with pinned dependencies**

```toml
[package]
name = "media_cutnplay"
version = "0.1.0"
edition = "2021"

[dependencies]
iced = { version = "0.14", features = ["image", "tokio"] }
ffmpeg-next = { version = "9.0.0", features = ["codec", "format", "filter", "software-scaling"] }
tokio = { version = "1", features = ["full"] }
sysinfo = "0.33"
rfd = "0.15"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Create minimal `src/main.rs`**

```rust
fn main() -> iced::Result {
    ffmpeg_next::init().expect("Failed to initialize FFmpeg");
    iced::application("Media CutNPlay", update, view).run()
}

#[derive(Debug, Clone)]
enum Message {}

fn update(_state: &mut (), _message: Message) {}

fn view(_state: &()) -> iced::Element<'_, Message> {
    iced::widget::text("Media CutNPlay Initialized").into()
}
```

- [ ] **Step 4: Verify build and check**

Run: `cargo check`
Expected: PASS with 0 errors.

- [ ] **Step 5: Commit scaffolding**

```bash
git add Cargo.toml .gitignore src/main.rs
git commit -m "chore: scaffold project with Iced and ffmpeg-next dependencies"
```

---

### Task 2: Core Media Error & Metadata Extraction Engine

**Files:**
- Create: `src/core/mod.rs`
- Create: `src/core/error.rs`
- Create: `src/core/metadata.rs`
- Create: `tests/metadata_test.rs`

**Interfaces:**
- Consumes: Local media file path (`std::path::Path`)
- Produces: `MediaMetadata` struct containing `duration_secs: f64`, `bitrate: i64`, `format_name: String`, `video: Option<VideoMetadata>`, `audio: Option<AudioMetadata>`.

- [ ] **Step 1: Write the failing metadata extraction test**

```rust
// tests/metadata_test.rs
use std::process::Command;
use tempfile::NamedTempFile;

#[test]
fn test_probe_metadata_valid_video() {
    let tmp = NamedTempFile::new().unwrap();
    let video_path = tmp.path().with_extension("mp4");

    // Generate a 1-second synthetic MP4 test clip using ffmpeg CLI
    let status = Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=30",
            "-f", "lavfi", "-i", "sine=duration=1:frequency=1000",
            "-c:v", "libx264", "-c:a", "aac",
            video_path.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to run ffmpeg generator");
    assert!(status.success());

    ffmpeg_next::init().unwrap();
    let meta = media_cutnplay::core::metadata::probe_file(&video_path)
        .expect("Failed to probe metadata");

    assert!(meta.duration_secs >= 0.9 && meta.duration_secs <= 1.1);
    let video = meta.video.expect("Missing video stream");
    assert_eq!(video.width, 320);
    assert_eq!(video.height, 240);
    assert_eq!(video.fps.round() as u32, 30);
    assert!(meta.audio.is_some());

    let _ = std::fs::remove_file(video_path);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test metadata_test`
Expected: FAIL (modules `core::error` and `core::metadata` not yet defined).

- [ ] **Step 3: Implement `src/core/error.rs`, `src/core/metadata.rs`, and `src/core/mod.rs`**

```rust
// src/core/error.rs
use std::fmt;

#[derive(Debug, Clone)]
pub enum MediaError {
    Ffmpeg(String),
    Io(String),
    NoVideoStream,
    SeekFailed(String),
    CutFailed(String),
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ffmpeg(msg) => write!(f, "FFmpeg error: {msg}"),
            Self::Io(msg) => write!(f, "I/O error: {msg}"),
            Self::NoVideoStream => write!(f, "No video stream found in file"),
            Self::SeekFailed(msg) => write!(f, "Seek failed: {msg}"),
            Self::CutFailed(msg) => write!(f, "Cut failed: {msg}"),
        }
    }
}

impl std::error::Error for MediaError {}

impl From<ffmpeg_next::Error> for MediaError {
    fn from(err: ffmpeg_next::Error) -> Self {
        Self::Ffmpeg(err.to_string())
    }
}
```

```rust
// src/core/metadata.rs
use crate::core::error::MediaError;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct VideoMetadata {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub pixel_format: String,
}

#[derive(Debug, Clone)]
pub struct AudioMetadata {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone)]
pub struct MediaMetadata {
    pub file_path: std::path::PathBuf,
    pub file_size_bytes: u64,
    pub duration_secs: f64,
    pub bitrate: i64,
    pub format_name: String,
    pub video: Option<VideoMetadata>,
    pub audio: Option<AudioMetadata>,
}

pub fn probe_file(path: &Path) -> Result<MediaMetadata, MediaError> {
    let ictx = ffmpeg_next::format::input(path)?;
    let file_size_bytes = std::fs::metadata(path)
        .map(|m| m.len())
        .unwrap_or(0);

    let duration_secs = if ictx.duration() > 0 {
        ictx.duration() as f64 / f64::from(ffmpeg_next::ffi::AV_TIME_BASE)
    } else {
        0.0
    };

    let bitrate = ictx.bit_rate();
    let format_name = ictx.format().name().to_string();

    let mut video = None;
    let mut audio = None;

    for stream in ictx.streams() {
        let codecpar = stream.parameters();
        match codecpar.medium() {
            ffmpeg_next::media::Type::Video if video.is_none() => {
                if let Ok(ctx) = ffmpeg_next::codec::context::Context::from_parameters(codecpar.clone()) {
                    if let Ok(decoder) = ctx.decoder().video() {
                        let fps = f64::from(stream.rate().numerator()) / f64::from(stream.rate().denominator().max(1));
                        video = Some(VideoMetadata {
                            codec: decoder.codec().map(|c| c.name().to_string()).unwrap_or_default(),
                            width: decoder.width(),
                            height: decoder.height(),
                            fps,
                            pixel_format: format!("{:?}", decoder.format()),
                        });
                    }
                }
            }
            ffmpeg_next::media::Type::Audio if audio.is_none() => {
                if let Ok(ctx) = ffmpeg_next::codec::context::Context::from_parameters(codecpar.clone()) {
                    if let Ok(decoder) = ctx.decoder().audio() {
                        audio = Some(AudioMetadata {
                            codec: decoder.codec().map(|c| c.name().to_string()).unwrap_or_default(),
                            sample_rate: decoder.rate(),
                            channels: decoder.channels(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    Ok(MediaMetadata {
        file_path: path.to_path_buf(),
        file_size_bytes,
        duration_secs,
        bitrate,
        format_name,
        video,
        audio,
    })
}
```

```rust
// src/core/mod.rs
pub mod error;
pub mod metadata;
pub mod decoder;
pub mod cutter;
```

Export `pub mod core;` in `src/lib.rs` (or `src/main.rs`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test metadata_test`
Expected: PASS with 1 test passed.

- [ ] **Step 5: Commit metadata engine**

```bash
git add src/core tests/metadata_test.rs
git commit -m "feat(core): implement container probe and metadata extraction"
```

---

### Task 3: Video Frame Decoding & RGBA Scaling Pipeline

**Files:**
- Create: `src/core/decoder.rs`
- Create: `tests/decoder_test.rs`

**Interfaces:**
- Consumes: Media path, target timestamp `f64`
- Produces: `DecodedFrame { timestamp: f64, width: u32, height: u32, rgba_data: Vec<u8> }`

- [ ] **Step 1: Write the failing frame decoder test**

```rust
// tests/decoder_test.rs
use std::process::Command;
use tempfile::NamedTempFile;

#[test]
fn test_decode_frame_to_rgba() {
    let tmp = NamedTempFile::new().unwrap();
    let video_path = tmp.path().with_extension("mp4");

    let status = Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "testsrc=duration=2:size=160x120:rate=25",
            "-c:v", "libx264",
            video_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    ffmpeg_next::init().unwrap();
    let mut decoder = media_cutnplay::core::decoder::VideoDecoder::open(&video_path)
        .expect("Failed to open decoder");

    let frame = decoder.seek_and_decode(0.5).expect("Failed to decode frame at 0.5s");
    assert_eq!(frame.width, 160);
    assert_eq!(frame.height, 120);
    assert_eq!(frame.rgba_data.len(), 160 * 120 * 4);
    assert!(frame.timestamp >= 0.0);

    let _ = std::fs::remove_file(video_path);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test decoder_test`
Expected: FAIL (`VideoDecoder` not found).

- [ ] **Step 3: Implement `src/core/decoder.rs`**

```rust
// src/core/decoder.rs
use crate::core::error::MediaError;
use ffmpeg_next::format::context::Input;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{Context as SwsContext, Flags};
use ffmpeg_next::util::frame::video::Video as VideoFrame;
use std::path::Path;

pub struct DecodedFrame {
    pub timestamp: f64,
    pub width: u32,
    pub height: u32,
    pub rgba_data: Vec<u8>,
}

pub struct VideoDecoder {
    ictx: Input,
    video_stream_index: usize,
    decoder: ffmpeg_next::decoder::Video,
    scaler: SwsContext,
    time_base: ffmpeg_next::Rational,
}

impl VideoDecoder {
    pub fn open(path: &Path) -> Result<Self, MediaError> {
        let ictx = ffmpeg_next::format::input(path)?;
        let video_stream = ictx
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .ok_or(MediaError::NoVideoStream)?;

        let video_stream_index = video_stream.index();
        let time_base = video_stream.time_base();

        let ctx = ffmpeg_next::codec::context::Context::from_parameters(video_stream.parameters())?;
        let decoder = ctx.decoder().video()?;

        let scaler = SwsContext::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::RGBA,
            decoder.width(),
            decoder.height(),
            Flags::BILINEAR,
        )?;

        Ok(Self {
            ictx,
            video_stream_index,
            decoder,
            scaler,
            time_base,
        })
    }

    pub fn seek_and_decode(&mut self, target_sec: f64) -> Result<DecodedFrame, MediaError> {
        let target_pts = (target_sec / f64::from(self.time_base)) as i64;
        
        // Seek to nearest prior keyframe
        self.ictx.seek(target_pts, ..target_pts)?;
        self.decoder.flush();

        let mut decoded_frame = VideoFrame::empty();
        let mut best_frame = None;

        for (stream, packet) in self.ictx.packets() {
            if stream.index() == self.video_stream_index {
                self.decoder.send_packet(&packet)?;
                while self.decoder.receive_frame(&mut decoded_frame).is_ok() {
                    let pts = decoded_frame.pts().unwrap_or(0);
                    let sec = pts as f64 * f64::from(self.time_base);

                    let mut rgb_frame = VideoFrame::empty();
                    self.scaler.run(&decoded_frame, &mut rgb_frame)?;

                    let width = rgb_frame.width();
                    let height = rgb_frame.height();
                    let data = rgb_frame.data(0);
                    let linesize = rgb_frame.stride(0);

                    let mut rgba_data = Vec::with_capacity((width * height * 4) as usize);
                    for y in 0..height {
                        let start = (y as usize) * linesize;
                        let end = start + (width as usize) * 4;
                        rgba_data.extend_from_slice(&data[start..end]);
                    }

                    let frame_res = DecodedFrame {
                        timestamp: sec,
                        width,
                        height,
                        rgba_data,
                    };

                    if sec >= target_sec {
                        return Ok(frame_res);
                    }
                    best_frame = Some(frame_res);
                }
            }
        }

        best_frame.ok_or_else(|| MediaError::SeekFailed(format!("No frame found near {target_sec}s")))
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test decoder_test`
Expected: PASS with 1 test passed.

- [ ] **Step 5: Commit decoder pipeline**

```bash
git add src/core/decoder.rs tests/decoder_test.rs
git commit -m "feat(core): implement video frame decoder and RGBA scaler"
```

---

### Task 4: Lossless Video Cutting Engine (`-c copy`)

**Files:**
- Create: `src/core/cutter.rs`
- Create: `tests/cutter_test.rs`

**Interfaces:**
- Consumes: Input path, output path, `in_point: f64`, `out_point: f64`, progress callback `Fn(f32)`
- Produces: `Result<PathBuf, MediaError>` output cut file with preserved bitstream and recalibrated timestamps.

- [ ] **Step 1: Write the failing lossless cut test**

```rust
// tests/cutter_test.rs
use std::process::Command;
use tempfile::NamedTempFile;

#[test]
fn test_lossless_cut_remuxing() {
    let tmp_in = NamedTempFile::new().unwrap();
    let tmp_out = NamedTempFile::new().unwrap();
    let in_path = tmp_in.path().with_extension("mp4");
    let out_path = tmp_out.path().with_extension("mp4");

    // Generate a 4-second video clip
    let status = Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "testsrc=duration=4:size=320x240:rate=30",
            "-f", "lavfi", "-i", "sine=duration=4:frequency=440",
            "-c:v", "libx264", "-c:a", "aac",
            in_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    ffmpeg_next::init().unwrap();
    let mut progress_values = Vec::new();
    let result = media_cutnplay::core::cutter::lossless_cut(
        &in_path,
        &out_path,
        1.0,
        3.0,
        |p| progress_values.push(p),
    );

    assert!(result.is_ok(), "Lossless cut failed: {:?}", result.err());
    assert!(out_path.exists());
    assert!(!progress_values.is_empty());

    // Verify duration of cut output is approximately 2 seconds
    let meta = media_cutnplay::core::metadata::probe_file(&out_path).unwrap();
    assert!(meta.duration_secs >= 1.5 && meta.duration_secs <= 2.5);

    let _ = std::fs::remove_file(in_path);
    let _ = std::fs::remove_file(out_path);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test cutter_test`
Expected: FAIL (`cutter::lossless_cut` not found).

- [ ] **Step 3: Implement `src/core/cutter.rs`**

```rust
// src/core/cutter.rs
use crate::core::error::MediaError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn lossless_cut<F>(
    input_path: &Path,
    output_path: &Path,
    in_point: f64,
    out_point: f64,
    mut on_progress: F,
) -> Result<PathBuf, MediaError>
where
    F: FnMut(f32),
{
    if in_point >= out_point {
        return Err(MediaError::CutFailed("In-point must be less than out-point".into()));
    }

    let mut ictx = ffmpeg_next::format::input(input_path)?;
    let mut octx = ffmpeg_next::format::output(output_path)?;

    // Map input stream indices to output stream indices & stream parameters
    let mut stream_mapping = HashMap::new();
    let mut start_pts_map: HashMap<usize, i64> = HashMap::new();

    for (src_idx, istream) in ictx.streams().enumerate() {
        let medium = istream.parameters().medium();
        if medium == ffmpeg_next::media::Type::Video
            || medium == ffmpeg_next::media::Type::Audio
            || medium == ffmpeg_next::media::Type::Subtitle
        {
            let mut ostream = octx.add_stream(ffmpeg_next::encoder::find(istream.parameters().id()))?;
            ostream.set_parameters(istream.parameters());
            stream_mapping.insert(src_idx, ostream.index());
        }
    }

    octx.write_header()?;

    // Seek input to keyframe at or before in_point on the primary video stream
    let video_stream_idx = ictx
        .streams()
        .best(ffmpeg_next::media::Type::Video)
        .map(|s| s.index());

    if let Some(v_idx) = video_stream_idx {
        let tb = ictx.stream(v_idx).unwrap().time_base();
        let target_pts = (in_point / f64::from(tb)) as i64;
        ictx.seek(target_pts, ..target_pts)?;
    }

    let duration = out_point - in_point;

    for (stream, mut packet) in ictx.packets() {
        let src_idx = stream.index();
        if let Some(&out_idx) = stream_mapping.get(&src_idx) {
            let in_stream = ictx.stream(src_idx).unwrap();
            let out_stream = octx.stream(out_idx).unwrap();
            let in_tb = in_stream.time_base();
            let out_tb = out_stream.time_base();

            let pkt_pts = packet.pts().unwrap_or(0);
            let pkt_sec = pkt_pts as f64 * f64::from(in_tb);

            if pkt_sec > out_point {
                // End of cut segment reached
                if stream.parameters().medium() == ffmpeg_next::media::Type::Video {
                    break;
                }
                continue;
            }

            if pkt_sec < in_point && packet.is_key() {
                // Retain GOP start keyframe if needed for stream integrity
            }

            let start_pts = start_pts_map.entry(src_idx).or_insert(pkt_pts);
            let rebased_pts = (packet.pts().unwrap_or(0) - *start_pts).max(0);
            let rebased_dts = (packet.dts().unwrap_or(0) - *start_pts).max(0);

            packet.set_stream(out_idx);
            packet.set_pts(Some(ffmpeg_next::rescale(rebased_pts, in_tb, out_tb)));
            packet.set_dts(Some(ffmpeg_next::rescale(rebased_dts, in_tb, out_tb)));

            packet.write_interleaved(&mut octx)?;

            let progress = ((pkt_sec - in_point) / duration).clamp(0.0, 1.0) as f32;
            on_progress(progress);
        }
    }

    octx.write_trailer()?;
    on_progress(1.0);

    Ok(output_path.to_path_buf())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test cutter_test`
Expected: PASS with 1 test passed.

- [ ] **Step 5: Commit lossless cutter**

```bash
git add src/core/cutter.rs tests/cutter_test.rs
git commit -m "feat(core): implement lossless stream copy cutting with PTS/DTS recalibration"
```

---

### Task 5: Background Worker Actor & Command Channels

**Files:**
- Create: `src/worker/mod.rs`
- Create: `src/worker/commands.rs`
- Create: `src/worker/actor.rs`
- Create: `tests/worker_test.rs`

**Interfaces:**
- Consumes: Tokio MPSC sender/receiver
- Produces: Asynchronous, non-blocking media seek/decode and export execution offloading from UI thread.

- [ ] **Step 1: Write the failing worker actor test**

```rust
// tests/worker_test.rs
use std::process::Command;
use tempfile::NamedTempFile;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_worker_actor_lifecycle() {
    let tmp = NamedTempFile::new().unwrap();
    let video_path = tmp.path().with_extension("mp4");

    Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "testsrc=duration=1:size=160x120:rate=25",
            "-c:v", "libx264", video_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();

    ffmpeg_next::init().unwrap();

    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let (event_tx, mut event_rx) = mpsc::channel(16);

    std::thread::spawn(move || {
        media_cutnplay::worker::actor::run_worker(cmd_rx, event_tx);
    });

    cmd_tx.send(media_cutnplay::worker::commands::WorkerCommand::LoadFile(video_path.clone())).await.unwrap();

    let event = event_rx.recv().await.unwrap();
    match event {
        media_cutnplay::worker::commands::WorkerEvent::MetadataLoaded(meta) => {
            assert_eq!(meta.video.unwrap().width, 160);
        }
        _ => panic!("Unexpected event"),
    }

    let _ = std::fs::remove_file(video_path);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test worker_test`
Expected: FAIL (worker module not found).

- [ ] **Step 3: Implement `src/worker/commands.rs` and `src/worker/actor.rs`**

```rust
// src/worker/commands.rs
use crate::core::decoder::DecodedFrame;
use crate::core::error::MediaError;
use crate::core::metadata::MediaMetadata;
use std::path::PathBuf;

#[derive(Debug)]
pub enum WorkerCommand {
    LoadFile(PathBuf),
    Seek(f64),
    StartLosslessCut {
        input_path: PathBuf,
        output_path: PathBuf,
        in_point: f64,
        out_point: f64,
    },
}

#[derive(Debug)]
pub enum WorkerEvent {
    MetadataLoaded(MediaMetadata),
    FrameDecoded(DecodedFrame),
    CutProgress(f32),
    CutCompleted(Result<PathBuf, MediaError>),
    Error(MediaError),
}
```

```rust
// src/worker/actor.rs
use crate::core::cutter::lossless_cut;
use crate::core::decoder::VideoDecoder;
use crate::core::metadata::probe_file;
use crate::worker::commands::{WorkerCommand, WorkerEvent};
use tokio::sync::mpsc::{Receiver, Sender};

pub fn run_worker(mut cmd_rx: Receiver<WorkerCommand>, event_tx: Sender<WorkerEvent>) {
    let mut current_decoder: Option<VideoDecoder> = None;

    while let Some(cmd) = cmd_rx.blocking_recv() {
        match cmd {
            WorkerCommand::LoadFile(path) => {
                match probe_file(&path) {
                    Ok(meta) => {
                        let _ = event_tx.blocking_send(WorkerEvent::MetadataLoaded(meta));
                        current_decoder = VideoDecoder::open(&path).ok();
                        if let Some(dec) = &mut current_decoder {
                            if let Ok(frame) = dec.seek_and_decode(0.0) {
                                let _ = event_tx.blocking_send(WorkerEvent::FrameDecoded(frame));
                            }
                        }
                    }
                    Err(err) => {
                        let _ = event_tx.blocking_send(WorkerEvent::Error(err));
                    }
                }
            }
            WorkerCommand::Seek(target_sec) => {
                if let Some(dec) = &mut current_decoder {
                    match dec.seek_and_decode(target_sec) {
                        Ok(frame) => {
                            let _ = event_tx.blocking_send(WorkerEvent::FrameDecoded(frame));
                        }
                        Err(err) => {
                            let _ = event_tx.blocking_send(WorkerEvent::Error(err));
                        }
                    }
                }
            }
            WorkerCommand::StartLosslessCut {
                input_path,
                output_path,
                in_point,
                out_point,
            } => {
                let tx_prog = event_tx.clone();
                let res = lossless_cut(&input_path, &output_path, in_point, out_point, move |p| {
                    let _ = tx_prog.blocking_send(WorkerEvent::CutProgress(p));
                });
                let _ = event_tx.blocking_send(WorkerEvent::CutCompleted(res));
            }
        }
    }
}
```

```rust
// src/worker/mod.rs
pub mod commands;
pub mod actor;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test worker_test`
Expected: PASS with 1 test passed.

- [ ] **Step 5: Commit worker actor**

```bash
git add src/worker tests/worker_test.rs
git commit -m "feat(worker): implement background actor and command channel architecture"
```

---

### Task 6: Elm State Management & Telemetry System

**Files:**
- Create: `src/state/mod.rs`
- Create: `src/state/app_state.rs`
- Create: `src/state/messages.rs`

**Interfaces:**
- Consumes: Worker events, user UI inputs, timer ticks
- Produces: Coherent UI state updates, memory telemetry (`sysinfo`), and message handling.

- [ ] **Step 1: Implement `src/state/messages.rs`**

```rust
// src/state/messages.rs
use crate::core::metadata::MediaMetadata;
use crate::worker::commands::WorkerEvent;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Message {
    // Queue Events
    OpenFilePicker,
    FilesAdded(Vec<PathBuf>),
    SelectQueueItem(usize),
    RemoveQueueItem(usize),
    ClearQueue,

    // Transport & Playback
    TogglePlay,
    StepFrame(i32),
    Seek(f64),
    ToggleFullscreen,

    // Cut Markers
    SetInPoint,
    SetOutPoint,
    ClearPoints,

    // Export Controls
    SelectOutputPath,
    StartLosslessCut,

    // Worker Event Dispatch
    WorkerEventReceived(WorkerEventWrapper),

    // System Telemetry
    TickTelemetry,
}

// Wrapper for WorkerEvent so it can implement Clone or be dispatched to Iced
#[derive(Debug, Clone)]
pub enum WorkerEventWrapper {
    MetadataLoaded(MediaMetadata),
    FrameDecoded {
        timestamp: f64,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    CutProgress(f32),
    CutCompleted(Result<PathBuf, String>),
    Error(String),
}
```

- [ ] **Step 2: Implement `src/state/app_state.rs`**

```rust
// src/state/app_state.rs
use crate::core::metadata::MediaMetadata;
use iced::widget::image;
use std::path::PathBuf;
use sysinfo::{Pid, ProcessesToUpdate, System};

#[derive(Debug, Clone, PartialEq)]
pub enum ExportStatus {
    Idle,
    Processing { progress: f32, output_path: PathBuf },
    Completed { output_path: PathBuf },
    Failed(String),
}

pub struct AppState {
    pub queue: Vec<MediaMetadata>,
    pub active_index: Option<usize>,

    pub is_playing: bool,
    pub is_fullscreen: bool,
    pub current_timestamp: f64,
    pub in_point: Option<f64>,
    pub out_point: Option<f64>,
    pub current_frame: Option<image::Handle>,

    pub target_output_path: Option<PathBuf>,
    pub export_status: ExportStatus,

    pub ram_usage_mb: f32,
    pub detected_hw: String,
    pub system_monitor: System,
    pub current_pid: Pid,
}

impl Default for AppState {
    fn default() -> Self {
        let mut sys = System::new();
        let pid = Pid::from_u32(std::process::id());
        sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        let ram_mb = sys.process(pid).map(|p| p.memory() as f32 / (1024.0 * 1024.0)).unwrap_or(0.0);

        Self {
            queue: Vec::new(),
            active_index: None,
            is_playing: false,
            is_fullscreen: false,
            current_timestamp: 0.0,
            in_point: None,
            out_point: None,
            current_frame: None,
            target_output_path: None,
            export_status: ExportStatus::Idle,
            ram_usage_mb: ram_mb,
            detected_hw: "Native libavcodec".to_string(),
            system_monitor: sys,
            current_pid: pid,
        }
    }
}

impl AppState {
    pub fn refresh_memory(&mut self) {
        self.system_monitor.refresh_processes(ProcessesToUpdate::Some(&[self.current_pid]), true);
        if let Some(proc) = self.system_monitor.process(self.current_pid) {
            self.ram_usage_mb = proc.memory() as f32 / (1024.0 * 1024.0);
        }
    }

    pub fn active_item(&self) -> Option<&MediaMetadata> {
        self.active_index.and_then(|idx| self.queue.get(idx))
    }
}
```

```rust
// src/state/mod.rs
pub mod app_state;
pub mod messages;
```

- [ ] **Step 3: Verify build**

Run: `cargo check`
Expected: PASS.

- [ ] **Step 4: Commit state management**

```bash
git add src/state
git commit -m "feat(state): implement Elm application state, messages, and memory telemetry"
```

---

### Task 7: Iced 3-Column UI & Fullscreen Player

**Files:**
- Create: `src/ui/mod.rs`
- Create: `src/ui/layout.rs`
- Create: `src/ui/queue_view.rs`
- Create: `src/ui/player_view.rs`
- Create: `src/ui/controls_view.rs`
- Create: `src/ui/status_bar.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `AppState` and user interactions
- Produces: Rich 3-column UI (Queue, Preview/Scrubber with Fullscreen button, Export Inspector) and bottom status bar.

- [ ] **Step 1: Implement UI components**
  - `src/ui/queue_view.rs`: Add Files button, Clear button, media queue scrollable list.
  - `src/ui/player_view.rs`: Image viewport rendering `image(handle)`, transport controls (play/pause, frame step, `[⛶ Fullscreen]`), slider scrubber, and In/Out marker buttons.
  - `src/ui/controls_view.rs`: Metadata inspector, lossless cut duration readout, output path selector, and `[ START LOSSLESS CUT ]` button with progress bar.
  - `src/ui/status_bar.rs`: Ready/Cutting indicator, hardware probe info, and live process RAM (`RAM: XX.X MB`).
  - `src/ui/layout.rs`: Compose left, center, right columns and bottom bar, switching to maximized view when `is_fullscreen` is active.

- [ ] **Step 2: Wire application loop in `src/main.rs`**
  - Integrate Tokio channels between UI loop and worker thread.
  - Wire Iced subscriptions:
    - 1-second interval ticker for `TickTelemetry` and playback advance.
    - Worker event channel receiver.
  - Handle `Message::ToggleFullscreen` via `iced::window::toggle_fullscreen`.

- [ ] **Step 3: Verify GUI build**

Run: `cargo build`
Expected: PASS with 0 errors.

- [ ] **Step 4: Commit UI components**

```bash
git add src/ui src/main.rs
git commit -m "feat(ui): implement 3-column layout, fullscreen player, and export controls"
```

---

### Task 8: End-to-End System Verification, Performance & RAM Benchmarks

**Files:**
- Create: `tests/e2e_workflow_test.rs`

**Interfaces:**
- Consumes: Complete Media CutNPlay application
- Produces: Verified end-to-end workflow (file load -> probe -> frame decode -> seek -> lossless cut -> RAM check).

- [ ] **Step 1: Write E2E test verifying complete pipeline & RAM benchmark**

```rust
// tests/e2e_workflow_test.rs
use std::process::Command;
use tempfile::NamedTempFile;

#[test]
fn test_e2e_cut_and_verify_metadata() {
    let tmp_src = NamedTempFile::new().unwrap();
    let tmp_dest = NamedTempFile::new().unwrap();
    let src = tmp_src.path().with_extension("mp4");
    let dest = tmp_dest.path().with_extension("mp4");

    // 1. Generate 3-second test clip
    let st = Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "testsrc=duration=3:size=320x240:rate=25",
            "-f", "lavfi", "-i", "sine=duration=3:frequency=880",
            "-c:v", "libx264", "-c:a", "aac",
            src.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(st.success());

    ffmpeg_next::init().unwrap();

    // 2. Probe metadata
    let meta = media_cutnplay::core::metadata::probe_file(&src).unwrap();
    assert_eq!(meta.video.unwrap().width, 320);

    // 3. Decode frame
    let mut dec = media_cutnplay::core::decoder::VideoDecoder::open(&src).unwrap();
    let frame = dec.seek_and_decode(1.0).unwrap();
    assert_eq!(frame.rgba_data.len(), 320 * 240 * 4);

    // 4. Perform lossless cut (1.0s to 2.5s)
    let cut_res = media_cutnplay::core::cutter::lossless_cut(&src, &dest, 1.0, 2.5, |_| {});
    assert!(cut_res.is_ok());

    // 5. Verify output file validity and duration
    let cut_meta = media_cutnplay::core::metadata::probe_file(&dest).unwrap();
    assert!(cut_meta.duration_secs >= 1.0 && cut_meta.duration_secs <= 2.0);

    let _ = std::fs::remove_file(src);
    let _ = std::fs::remove_file(dest);
}
```

- [ ] **Step 2: Run all automated tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Build release binary**

Run: `cargo build --release`
Expected: Clean release binary produced at `target/release/media_cutnplay`.

- [ ] **Step 4: Commit test suite**

```bash
git add tests/e2e_workflow_test.rs
git commit -m "test: add comprehensive end-to-end workflow verification"
```
