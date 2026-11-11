# Media CutNPlay - Phase 1 Design Specification

* **Date:** 2026-11-11
* **Target Version:** 0.1.0 (Phase 1 MVP)
* **Architecture:** 100% Pure Rust Stack (Iced 0.14 + Tokio + FFmpeg Native C-FFI via `ffmpeg-next`)
* **Target Platforms:** Linux (X11 / Wayland), Windows 10/11, macOS

---

## 1. Scope & Objective

Media CutNPlay is an ultra-lightweight, high-performance, 100% offline desktop media processor.
This specification governs **Phase 1: MVP Core Engine & Basic UI**, covering:
1. Pure Rust native application initialization using `iced` 0.14 and `tokio`.
2. Safe FFmpeg C-FFI bindings using `ffmpeg-next` (targeting native `libavformat`, `libavcodec`, `libswscale`, `libavutil`).
3. Container probe & metadata inspection (duration, resolution, framerate, video/audio stream codecs).
4. Video preview player with frame scrubbing (video-only visual decoding to RGBA buffers for `iced::widget::image`).
5. Lossless Video Cutting (`-c copy`) on keyframe boundaries with stream copying and PTS/DTS recalibration.
6. 3-column UI layout (Media Queue, Center Preview Scrubber with Fullscreen Mode, Cut & Export Inspector) and bottom telemetry bar (RAM usage via `sysinfo`, progress bar).

---

## 2. Architecture & Module Structure

```
media_cutnplay/
├── Cargo.toml
├── src/
│   ├── main.rs              # App entrypoint, window initialization & event loop
│   ├── core/                # Safe FFmpeg Native C-FFI Engine
│   │   ├── mod.rs
│   │   ├── error.rs         # Structured MediaError type
│   │   ├── metadata.rs      # Probes container & stream parameters
│   │   ├── decoder.rs       # Video stream demuxer & SwsContext RGBA frame converter
│   │   └── cutter.rs        # Lossless packet copy remuxer with PTS/DTS re-basing
│   ├── worker/              # Dedicated Background Actor Thread
│   │   ├── mod.rs
│   │   ├── actor.rs         # Manages FFmpeg stateful contexts safely
│   │   └── commands.rs      # DecodeRequest, SeekRequest, LosslessCutRequest, Progress
│   ├── state/               # Application State & Messages (Elm Architecture)
│   │   ├── mod.rs
│   │   ├── app_state.rs     # Queue, active selection, player state, cut markers
│   │   └── messages.rs      # Iced Messages (transport, queue events, cut actions)
│   └── ui/                  # Iced 0.14 UI Widgets & Views
│       ├── mod.rs
│       ├── layout.rs        # 3-column split view + bottom status bar
│       ├── queue_view.rs    # Left panel: media queue list & actions
│       ├── player_view.rs   # Center panel: image viewer, transport bar, scrubber & fullscreen toggle
│       ├── controls_view.rs # Right panel: In/Out markers, export triggers
│       └── status_bar.rs    # Bottom: RAM usage (via sysinfo) & export progress
```

### 2.1 Dependencies
* `iced = { version = "0.14", features = ["image", "tokio"] }`: Responsive native GUI without webviews or IPC.
* `ffmpeg-next = "9.0"`: Safe Rust bindings to system `libavcodec`, `libavformat`, `libswscale`, `libavutil`.
* `tokio = { version = "1", features = ["full"] }`: Async runtime and MPSC channels.
* `sysinfo = "0.33"`: Sampling process RSS to display live RAM usage (< 35 MB target).
* `rfd = "0.15"`: Native cross-platform file open dialogs.

### 2.2 Actor Thread Model
FFmpeg `AVCodecContext` instances are not thread-safe. To prevent race conditions and eliminate UI thread blocking:
* A dedicated OS thread hosts the active media decoder and handles sequential seek/decode requests sent via Tokio channels.
* Decoded frames are converted directly to RGBA pixel bytes on the worker thread and delivered to the Iced application loop as `iced::widget::image::Handle`.
* Export operations run concurrently on async worker tasks, emitting progress updates back to the UI.

---

## 3. Media Core Engine Specification

### 3.1 Metadata Inspection (`metadata.rs`)
* Opens input container using `ffmpeg::format::input(&path)`.
* Extracts:
  * Container duration (seconds) and bitrate.
  * Best video stream: codec identifier, pixel format, dimensions (`width` x `height`), framerate (`fps`), aspect ratio.
  * Audio streams: codec identifier, channel count, sample rate.

### 3.2 Frame Decoding & Seeking (`decoder.rs`)
* Maintains open demuxer and video decoder context.
* Seeking logic:
  * On `Seek(timestamp)`: jumps demuxer to the nearest keyframe at or before target timestamp using `context.seek(target_pts, ..)`.
  * Decodes consecutive packets until frame timestamp matches or exceeds target timestamp.
* Pixel format conversion:
  * Allocates `ffmpeg::software::scaling::Context` from source pixel format (`YUV420P`, `NV12`, etc.) to `RGBA`.
  * Converts output `VideoFrame` to an 8-bit RGBA buffer and packages it into `iced::widget::image::Handle::from_rgba(width, height, bytes)`.

### 3.3 Lossless Video Cutting (`cutter.rs`)
* Operates in pure stream copy mode (`-c copy`):
  1. Opens input format context and initializes output format context with matching container format.
  2. Copies stream parameters (`codecpar`) from input streams to output streams without re-encoding.
  3. Seeks input to the keyframe (I-frame) closest to `in_point`.
  4. Loops through packets:
     * Ignores packets prior to `in_point`.
     * Breaks processing when packet PTS reaches `out_point`.
     * Re-bases PTS and DTS by subtracting the starting timestamp offset.
     * Rescales timestamps from input stream `time_base` to output stream `time_base`.
     * Writes packet using `output.write_interleaved_packet(&mut packet)`.
  5. Periodically transmits progress `(current_sec - in_sec) / (out_sec - in_sec)` over progress channel.

---

## 4. UI & State Machine Specification (Iced 0.14)

### 4.1 State Model (`AppState`)
```rust
pub struct AppState {
    pub queue: Vec<MediaItem>,
    pub active_index: Option<usize>,
    pub is_playing: bool,
    pub is_fullscreen: bool,
    pub current_timestamp: f64,
    pub in_point: Option<f64>,
    pub out_point: Option<f64>,
    pub current_frame: Option<iced::widget::image::Handle>,
    pub export_status: ExportStatus,
    pub ram_usage_mb: f32,
    pub detected_hw: String,
}

pub enum ExportStatus {
    Idle,
    Processing { progress: f32, output_path: PathBuf },
    Completed { output_path: PathBuf },
    Failed(String),
}
```

### 4.2 Message Events (`Message`)
* **Queue:** `OpenFilePicker`, `FilesAdded(Vec<PathBuf>)`, `SelectQueueItem(usize)`, `RemoveQueueItem(usize)`, `ClearQueue`.
* **Transport:** `TogglePlay`, `StepFrame(i32)`, `Seek(f64)`, `FrameDecoded(image::Handle, f64)`.
* **Fullscreen:** `ToggleFullscreen` (calls `iced::window::toggle_fullscreen` and expands video preview area).
* **Markers:** `SetInPoint`, `SetOutPoint`, `ClearCutPoints`.
* **Export:** `StartLosslessCut`, `CutProgressUpdated(f32)`, `CutFinished(Result<PathBuf, String>)`.
* **Telemetry:** `TickTelemetry` (periodic 1-second interval to update `ram_usage_mb`).

### 4.3 UI Layout Components
1. **Left Panel (Media Queue):**
   * Action buttons: `[+ Add Files]`, `[Clear Queue]`.
   * Scrollable list of queued media files showing file name, resolution badge, duration, file size, and status tag.
2. **Center Panel (Preview & Transport):**
   * Video frame viewport rendering `image(handle)` with letterboxing / aspect ratio preservation.
   * Timecode readout (`00:01:23.450 / 00:05:00.000`).
   * Transport controls: `[⏮ -1s] [◀ Prev] [▶ Play / ⏸ Pause] [Next ▶] [⏭ +1s]   [⛶ Fullscreen]`.
   * Timeline scrubber slider with marked indicators for `In-Point` and `Out-Point`.
   * In/Out buttons: `[Set In (I)]`, `[Set Out (O)]`, `[Clear Points]`.
3. **Right Panel (Cut & Export Controls):**
   * Active media technical information summary (container, codecs, bitrate).
   * Mode indicator: `Lossless Cut (Keyframe-aligned)`.
   * In/Out points timestamps and calculated cut duration.
   * Target destination picker.
   * Prominent `[ START LOSSLESS CUT ]` button with active progress indicator.
4. **Bottom Status Bar:**
   * Left: Overall status message (`Ready`, `Cutting...`, `Export Complete`).
   * Center: Hardware detection status (`libavcodec native`).
   * Right: Real-time process memory footprint (`RAM: XX.X MB`).

---

## 5. Non-Functional Requirements & Performance Targets

* **Memory Footprint:** Idle RAM usage < 35 MB; Peak scrubbing RAM < 150 MB.
* **Lossless Cut Speed:** 1 GB video cut in < 3 seconds on NVMe SSD (I/O bounded).
* **Startup Latency:** Cold boot < 500 ms.
* **Offline Execution:** 100% offline, zero network requests, zero telemetry.
* **FFI Safety:** All C-FFI calls wrapped with safe Rust abstractions, preventing undefined behavior.

---

## 6. Verification & Testing Plan

1. **Automated Unit Tests (`cargo test`):**
   * Metadata extraction: validates duration, resolution, stream detection on sample media files.
   * Keyframe seek offset calculation: verifies correct calculation of PTS offsets and timestamp re-basing.
   * Lossless cut engine test: generates synthetic video test clip, executes lossless cut, and asserts valid output format, stream continuity, and file duration.
2. **GUI & System Telemetry Verification:**
   * Build in release mode: `cargo build --release`.
   * Run application, add video files via file picker, verify frame preview rendering.
   * Scrub along timeline, test frame stepping (+/- 1s and +/- 1 frame).
   * Test Fullscreen toggle (`[⛶ Fullscreen]` button and `Escape`/`F11` shortcut).
   * Execute lossless cut, verify cut file integrity with `ffprobe` and media playback.
   * Monitor live process RAM in the status bar to verify adherence to the < 35 MB idle memory constraint.
