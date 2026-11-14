# Media CutNPlay

<div align="center">

**Offline Bulk Media Transcoder, Processor & Smart Video Cutter**

[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg?logo=rust)](https://www.rust-lang.org)
[![Iced GUI](https://img.shields.io/badge/gui-iced%200.14-blue.svg)](https://iced.rs)
[![FFmpeg](https://img.shields.io/badge/ffmpeg-libavcodec%209.0-green.svg?logo=ffmpeg)](https://ffmpeg.org)
[![Offline](https://img.shields.io/badge/offline-100%25%20Zero--IPC-brightgreen.svg)](#privacy--zero-ipc-architecture)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](#license)

</div>

---

## Overview

**Media CutNPlay** is an ultra-lightweight, high-performance, 100% offline desktop application for video playback, precise stream cutting, and bulk media transcoding.

Most desktop media utilities fall into two troublesome camps:
1. **Webview-Based Applications (Electron / Tauri):** Heavy memory footprint (> 400 MB RAM), high scrubbing latency, and bloated IPC serialization when moving uncompressed video buffers.
2. **Conventional FFmpeg Frontends:** Cluttered, dated interfaces that only support either full, slow re-encoding or inaccurate keyframe-only lossless cutting.

**Media CutNPlay** is built with a pure Rust stack using the **Iced 0.14** GUI framework, **Tokio**, and direct C-FFI bindings to **FFmpeg 9** (`libavcodec`, `libavformat`, `libswscale`, `libavfilter`, `libswresample`) alongside **CPAL** for low-latency audio.

### Key Highlights
- **Ultra-Low Memory Footprint:** Idles under **35 MB RAM**; peak memory stays under **150 MB** during 4K video scrubbing.
- **Zero-IPC Architecture:** Single address space execution from file I/O to FFmpeg decoding to hardware-accelerated GUI rendering.
- **100% Offline & Private:** Zero network requests, zero telemetry, zero analytics, zero external daemon dependencies.
- **Instant Lossless Cutting:** Stream copy (`-c copy`) aligned to keyframes with PTS/DTS recalibration. Cuts 1 GB in under 3 seconds on SSDs.
- **Frame-Accurate Smart Cut:** Re-encodes only partial boundary GOPs (Head & Tail) while stream-copying intact body GOPs without generational loss.
- **Audio-Clocked Playback Engine:** Smooth playback driven by audio device clock synchronization with video drift compensation.
- **Hardware Acceleration:** Native NVDEC (CUDA) and VAAPI hardware decoding with automatic fallback to software decoding; NVIDIA NVENC GPU-accelerated transcoding.

---

## Features

### 1. Video Cutting & Trimming

* **Lossless Cut Mode (`-c copy`):**
  * Instantaneous stream cutting on GOP keyframe boundaries.
  * Preserves bitstream intact without re-encoding quality degradation.
  * Automatic PTS/DTS timestamp recalibration for glitch-free playback.
* **Smart Cut Mode (Frame-Accurate Hybrid Cut):**
  * Cut on **any arbitrary frame** without re-encoding the entire video.
  * Re-encodes only the partial GOP at the head (In-point to next I-frame) and tail (last I-frame to Out-point).
  * Stream-copies the vast majority (95%+) of GOPs in between.
  * Preserves parameter sets (SPS/PPS) in-band for seamless stream concatenation.
* **Multi-Segment Trimming & Merging:**
  * Define multiple In/Out segments on a single timeline.
  * Overlapping spans automatically fuse into clean continuous segments.
  * Export options:
    * **Merge:** Seamlessly join all segments into a single output file.
    * **Split:** Batch export segments as numbered individual clips (`_01`, `_02`, etc.).

### 2. Audio-Clocked Video Player

* **CPAL Master Audio Synchronization:**
  * Uses the audio output device clock as the master reference clock to prevent A/V drift.
  * Decoded audio streams are resampled and mixed in a dedicated background thread.
* **Hardware Video Decoding:**
  * Probes and attaches to **NVDEC (CUDA)** on NVIDIA systems and **VAAPI** on Linux.
  * Gracefully and transparently falls back to multithreaded software decoding if hardware decoding is unavailable or unsupported.
  * Override via environment variable: `CUTNPLAY_HWDEC=cuda|vaapi|off`.
* **Precision Transport:**
  * Exact frame stepping (`Left` / `Right` arrows).
  * Short jumps (`Shift + Left/Right`, ±5s) and long jumps (`J` / `L`, ±10s).
  * Smooth scrubbing with visual In/Out marker flags on the timeline.
* **Live Picture Adjustments:**
  * Real-time Brightness adjustment slider (-100% to +100%).
  * Real-time Contrast adjustment slider (-100% to +100%).
  * Instant Monochrome (black & white) toggle.
* **Distraction-Free Fullscreen:**
  * True borderless fullscreen mode (`F` or `Escape`).
  * Floating transport controls that auto-hide and smoothly slide in when hovering the bottom screen edge.

### 3. Whole-File Transcoder

* **Supported Codecs:**
  * **Video:** H.264 (`libx264` / `h264_nvenc`), HEVC/H.265 (`libx265` / `hevc_nvenc`), AV1 (`libsvtav1` / `av1_nvenc`), VP9 (`libvpx-vp9`).
  * **Audio:** AAC (`aac` encoder) or direct Stream Copy (`copy`).
* **Encoder Backends:**
  * **Hardware (NVENC):** NVIDIA GPU accelerated encoding for H.264, HEVC, and AV1.
  * **Software (CPU):** Multithreaded software encoding with codec-tailored presets.
* **Rate Control Modes:**
  * **Constant Quality (CRF / CQ):** Codec-normalized quality sliders (High, Medium, Low presets or custom slider).
  * **Average Bitrate:** 250 kbit/s to 40,000 kbit/s with 250 kbit/s increments.
* **Resolution Downscaling:**
  * Source (original), 1080p, 720p, 480p.
  * Aspect ratio preserving, scaled down only (no upscaling), guaranteed even dimensions for encoder compatibility.
* **Containers:**
  * MP4 (`.mp4`) and Matroska (`.mkv`).
* **Non-Blocking & Cancellable:**
  * Real-time percentage progress bar.
  * Atomic cancellation support to abort in-flight transcode jobs safely without leaving corrupt artifacts.

### 4. Modern Material Design 3 Interface

* Built with custom Material Design 3 design tokens: Dark surface palette, high-contrast text, tonal container elevation, rounded sliders, and custom Material Symbols font glyphs.
* **Left Action Bar:** Quick toggles for Media Queue, Cut & Export panel, Transcode panel, and Picture Adjustment flyouts.
* **Bottom Status Bar:** Live process RAM consumption monitor (via `sysinfo`) and active decode engine status (`NVDEC (CUDA)`, `VAAPI`, or `software`).

---

## System Architecture

```
┌────────────────────────────────────────────────────────────────────────┐
│                        Iced 0.14 Application UI                        │
│   - Model-View-Update (Elm Architecture)                               │
│   - Material Design 3 styling & embedded Material Symbols glyphs       │
│   - Hardware video frame presentation (WGPU / Softbuffer)              │
└───────────────────────────┬────────────────────────────────────────────┘
                            │ Tokio MPSC Channels
┌───────────────────────────▼────────────────────────────────────────────┐
│                    Dedicated Playback & Worker Actors                  │
│   - Player Engine: CPAL master audio clock & video queue synchronizer  │
│   - Worker Actor: Async task queue for metadata probing & exports      │
└───────────────────────────┬────────────────────────────────────────────┘
                            │ Native C-FFI Calls
┌───────────────────────────▼────────────────────────────────────────────┐
│                      Native FFmpeg 9 Shared C-FFI                      │
│   - libavformat: Demuxing & remuxing (MP4, MKV, WebM, TS, MOV)         │
│   - libavcodec:  NVDEC / VAAPI / CPU decoders & NVENC / CPU encoders   │
│   - libswscale:  Pixel format conversion (YUV420P/NV12 -> RGBA)        │
│   - libswresample: Audio sample rate conversion & channel layout mixing│
└────────────────────────────────────────────────────────────────────────┘
```

---

## Keyboard Shortcuts

| Shortcut | Action |
|:---|:---|
| **`Space`** or **`K`** | Toggle Play / Pause |
| **`Left Arrow`** | Step 1 frame backward |
| **`Right Arrow`** | Step 1 frame forward |
| **`Shift + Left Arrow`** | Jump backward 5 seconds |
| **`Shift + Right Arrow`** | Jump forward 5 seconds |
| **`J`** | Jump backward 10 seconds |
| **`L`** | Jump forward 10 seconds |
| **`Home`** | Seek to start (00:00:00) |
| **`End`** | Seek to end of active media |
| **`I`** | Set In-point at current playhead |
| **`O`** | Set Out-point at current playhead |
| **`Enter`** | Commit pending In/Out span to segment list |
| **`X`** | Clear pending In/Out markers |
| **`F`** | Toggle Fullscreen mode |
| **`Escape`** | Exit Fullscreen mode |
| **`Ctrl + O`** / **`Cmd + O`** | Open native file picker dialog |

---

## Installation & Building

### Prerequisites

You will need the **Rust toolchain** (1.80+) and system development libraries for **FFmpeg 9** and **ALSA** (on Linux).

#### Ubuntu / Debian (or derivatives with FFmpeg 9+):
```bash
sudo apt update
sudo apt install -y \
    build-essential \
    pkg-config \
    clang \
    libavcodec-dev \
    libavformat-dev \
    libavfilter-dev \
    libswscale-dev \
    libswresample-dev \
    libavutil-dev \
    libasound2-dev
```

#### Arch Linux:
```bash
sudo pacman -S --needed \
    base-devel \
    pkgconf \
    clang \
    ffmpeg \
    alsa-lib
```

#### Fedora:
```bash
sudo dnf install \
    gcc \
    pkgconfig \
    clang \
    ffmpeg-free-devel \
    alsa-lib-devel
```

### Building from Source

1. Clone the repository:
   ```bash
   git clone https://github.com/your-username/media_cutnplay.git
   cd media_cutnplay
   ```

2. Compile in release mode:
   ```bash
   cargo build --release
   ```

3. The compiled binary will be placed at:
   ```bash
   ./target/release/media_cutnplay
   ```

### Running the Application

Launch the GUI:
```bash
cargo run --release
```

Or open media files directly from the command line:
```bash
./target/release/media_cutnplay /path/to/video1.mp4 /path/to/video2.mkv
```

### Packaging (Linux Tarball & Desktop Entry)

A helper script is provided to automate release builds, packaging, checksumming, and desktop entry generation:

```bash
./release.sh
```

This generates:
- `dist/media_cutnplay-v0.1.0-linux-<arch>.tar.gz`
- `dist/media_cutnplay-v0.1.0-linux-<arch>.tar.gz.sha256`
- Embedded `.desktop` entry file for application launchers.

---

## Hardware Acceleration Configuration

Media CutNPlay automatically detects available hardware acceleration features on startup.

### Decoding (NVDEC / VAAPI)
By default, the player probes hardware devices in order:
1. `CUDA` (NVIDIA NVDEC)
2. `VAAPI` (Intel / AMD / Linux standard)
3. `Software` (CPU fallback)

You can force a specific decoder using the `CUTNPLAY_HWDEC` environment variable:
```bash
# Force NVIDIA NVDEC
CUTNPLAY_HWDEC=cuda cargo run --release

# Force VAAPI
CUTNPLAY_HWDEC=vaapi cargo run --release

# Force pure software decoding
CUTNPLAY_HWDEC=off cargo run --release
```

### Encoding (NVENC)
The transcoder probes NVIDIA NVENC encoders (`h264_nvenc`, `hevc_nvenc`, `av1_nvenc`) on startup.
- If NVENC is detected, the **NVENC** backend option is enabled in the Transcode panel.
- If NVENC is unsupported on your hardware or driver, the UI automatically restricts the backend to **Software** (CPU).

---

## Testing

Media CutNPlay includes a comprehensive automated test suite covering metadata parsing, video decoding, remuxing, smart GOP cutting, transcoding, hardware detection, timeline synchronization, and end-to-end workflows:

```bash
# Run all unit and integration tests
cargo test
```

### Test Suite Breakdown
- `tests/metadata_test.rs`: Container probing, stream format identification, and duration computation.
- `tests/cutter_test.rs`: Lossless GOP cutting, stream copying, and timestamp offset re-basing.
- `tests/smart_cut_test.rs`: Frame-accurate GOP segmentation, parameter set parsing, and Annex B / AVCC reframing.
- `tests/transcode_test.rs`: Software and hardware video transcoding, CRF quality scaling, downscaling filters, and audio resampling.
- `tests/player_test.rs`: Master clock audio synchronization, frame queue polling, drift correction, and seek accuracy.
- `tests/hwdec_test.rs`: Hardware acceleration attachment, environment override handling, and surface download.
- `tests/e2e_workflow_test.rs`: End-to-end multi-segment extraction, merging, and full workflow validation.

---

## Project Structure

```
media_cutnplay/
├── Cargo.toml            # Project dependencies and crate definition
├── release.sh            # Linux release packaging script
├── assets/
│   └── fonts/            # Embedded Material Symbols font subset
├── src/
│   ├── lib.rs            # Library entrypoint and module exports
│   ├── main.rs           # Application bootstrapper, window management & event loop
│   ├── core/             # Safe FFmpeg native engine abstractions
│   │   ├── mod.rs
│   │   ├── error.rs      # MediaError enum definitions
│   │   ├── metadata.rs   # Audio/video container & stream inspection
│   │   ├── cutter.rs     # Lossless stream copy remuxer & segment manager
│   │   ├── smart_cut.rs  # Frame-accurate hybrid GOP re-encoder & concat logic
│   │   └── transcode.rs  # Multi-codec video & audio transcoding pipeline
│   ├── player/           # Real-time AV playback engine
│   │   ├── mod.rs
│   │   ├── audio.rs      # CPAL audio backend & resampling ring buffer
│   │   ├── engine.rs     # Video decoding thread & frame synchronization
│   │   ├── hwaccel.rs    # NVDEC (CUDA) & VAAPI hardware acceleration hooks
│   │   └── timeline.rs   # Audio-driven master timeline & clock controller
│   ├── worker/           # Dedicated background task actors
│   │   ├── mod.rs
│   │   ├── actor.rs      # Sequential background execution thread
│   │   └── commands.rs   # Tokio MPSC command & event contracts
│   ├── state/            # Elm state architecture
│   │   ├── mod.rs
│   │   ├── app_state.rs  # Central application state & settings models
│   │   └── messages.rs   # Comprehensive Iced message taxonomy
│   └── ui/               # Iced 0.14 views and Material Design 3 widgets
│       ├── mod.rs
│       ├── action_bar.rs # Left vertical toggle navigation & picture adjustments
│       ├── controls_view.rs # Cut & Export inspector panel
│       ├── player_view.rs# Video preview, scrubber & fullscreen overlay
│       ├── queue_view.rs # Batch media queue panel
│       ├── transcode_view.rs # Transcode settings & progress panel
│       ├── status_bar.rs # Bottom telemetry bar (RAM & hardware status)
│       ├── layout.rs     # Responsive 3-pane layout coordinator
│       ├── shortcuts.rs  # Keyboard shortcut mapping
│       ├── icons.rs      # Material Symbols glyph constants
│       └── theme.rs      # Material Design 3 dark palette & widget styling
└── tests/                # Automated integration and end-to-end test suites
```

---

## License

This project is licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](http://www.apache.org/licenses/LICENSE-2.0))
- MIT license ([LICENSE-MIT](http://opensource.org/licenses/MIT))

at your option.

Embedded icon font `MaterialSymbolsRounded-subset.ttf` is derived from Google's [Material Symbols](https://fonts.google.com/icons) and licensed under the Apache License 2.0.
