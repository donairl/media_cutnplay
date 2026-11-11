# Product Requirements Document (PRD)

## Media CutNPlay

**Offline Bulk Media Transcoder, Processor & Smart Video Cutter**

* **Document Version:** 1.0.0
* **Target Release:** Q1 2027
* **Architecture:** 100% Pure Rust Stack (Iced GUI + Tokio + Libavcodec Native Bindings)
* **Target Platforms:** Linux (X11/Wayland), Windows 10/11, macOS (Intel & Apple Silicon)

---

## 1. Executive Summary & Vision

### 1.1 Problem Statement

Sebagian besar alat pemotong dan pengonversi media desktop yang ada saat ini terbagi menjadi dua kategori bermasalah:

1. **Webview-Based Apps (Electron/Tauri):** Memiliki konsumsi memori tinggi (RAM > 400 MB), latensi rendering tinggi saat *scrubbing* video, dan overhead IPC (Inter-Process Communication) yang besar saat mentransfer buffer video *raw*.
2. **GUI Wrappers FFmpeg Konvensional:** Memiliki antarmuka yang usang, kurva pembelajaran tinggi, dan tidak mendukung fitur *Smart Cut* (hanya mendukung konversi lambat penuh atau pemotongan *lossless* yang tidak akurat di luar *keyframe*).

### 1.2 Product Vision

**MediaForge Native** adalah aplikasi desktop pemroses media serba *offline*, ultra-ringan, dan berperforma tinggi. Dibangun menggunakan **Rust & Iced GUI**, aplikasi ini menghilangkan kebutuhan layer browser/IPC, mengonsumsi memori < 35 MB saat *idle*, serta menghadirkan fitur *Lossless* dan *Smart Video Cutting* presisi tingkat frame (*frame-accurate*) dengan pemanfaatan akselerasi GPU lokal secara penuh.

---

## 2. Target Audience & User Personas

| Persona | Primary Needs | Key Pain Point Solved |
| --- | --- | --- |
| **Content Creator / Video Editor** | Memotong bagian video berukuran besar secara instan tanpa menunggu proses *re-render*. | Mengeliminasi waktu ekspor berjam-jam dengan *Lossless/Smart Cut*. |
| **Developer / Power User** | Konversi media massal secara cepat, pembersihan EXIF, dan konsumsi resource sistem minimal. | *Memory footprint* ultra-kecil (~30 MB RAM) dan eksekusi background non-blocking. |
| **Privacy-Conscious User** | Mengompres dan menghapus metadata sensitif (GPS, model perangkat) secara 100% lokal. | Jaminan *Zero Network Calls* (100% pemrosesan media offline di perangkat). |

---

## 3. System Architecture & Tech Stack

```
┌─────────────────────────────────────────────────────────────────┐
│                    Iced GUI Layer (Pure Rust)                   │
│   - Elm Architecture (Model-View-Update)                        │
│   - Custom Canvas Widget (Interactive Timeline & Waveform)      │
│   - Direct Image Handle Rendering (wgpu / Softbuffer)           │
└───────────────────────────────┬─────────────────────────────────┘
                                │ Tokio MPSC Channels / Direct Calls
┌───────────────────────────────▼─────────────────────────────────┐
│                   Core Processing Engine (Rust)                 │
│   - Asynchronous Batch Task Queue (Tokio Runtime)               │
│   - Hardware Acceleration Probe (NVENC / QSV / AMF / VT)        │
│   - Smart-Cut GOP Segment Manager & Concat Logic                │
└───────────────────────────────┬─────────────────────────────────┘
                                │ C-Bindings (rsmpeg / ffmpeg-next)
┌───────────────────────────────▼─────────────────────────────────┐
│              FFmpeg Shared Libraries (C Native)                 │
│         libavcodec • libavformat • libavfilter • libswscale     │
└─────────────────────────────────────────────────────────────────┘

```

* **UI Framework:** `iced` 0.13+ (WGPU / Softbuffer backend).
* **Async Runtime:** `tokio` (Multithreaded async I/O & task scheduling).
* **Media Core:** C-Bindings native ke Shared Libraries FFmpeg (`libavcodec`, `libavformat`, `libswscale`, `libavfilter`).
* **Hardware Acceleration Matrix:**
* **NVIDIA:** NVENC / NVDEC (`h264_nvenc`, `hevc_nvenc`, `av1_nvenc`)
* **Intel:** QuickSync Video / QSV (`h264_qsv`, `hevc_qsv`)
* **AMD:** Advanced Media Framework / AMF (`h264_amf`, `hevc_amf`)
* **Apple:** VideoToolbox (`h264_videotoolbox`, `hevc_videotoolbox`)



---

## 4. Functional Requirements & Feature Specifications

### 4.1 Video Cutting Engine

#### FR-1.1: Lossless Cut Mode (`-c copy`)

* **Deskripsi:** Memotong video hanya pada titik *Keyframe* (I-Frame) terdekat tanpa proses *re-encoding*.
* **Performa:** Kecepatan ekspor dibatasi hanya oleh kecepatan baca/tulis media penyimpanan (I/O disk).
* **Persyaratan:** Toleransi pemotongan berada pada *GOP boundary* (Group of Pictures).

#### FR-1.2: Smart Cut Mode (Frame-Accurate Hybrid Cut)

* **Deskripsi:** Memungkinkan pemotongan presisi tepat pada frame mana saja (non-keyframe) tanpa melakukan *re-encode* pada seluruh isi video.
* **Logika Pemrosesan:**
1. **Head GOP Re-encode:** Re-encode bagian video dari titik *In-Point* terpilih sampai ke *Keyframe* berikutnya.
2. **Body Stream Copy:** Salin stream video secara *lossless* untuk seluruh GOP utuh di bagian tengah.
3. **Tail GOP Re-encode:** Re-encode bagian video dari *Keyframe* terakhir sebelum *Out-Point* hingga ke titik *Out-Point*.
4. **Stitch / Concat:** Menggabungkan ketiga segmen tersebut secara seamless menggunakan `concat demuxer`.



#### FR-1.3: Multi-Segment Trimming & Merging

* Mengizinkan pengguna menentukan lebih dari satu rentang *In/Out* pada satu video (misal: membuang segmen 02:10–03:15 dan 10:00–11:30).
* Menggabungkan segmen tersisa menjadi satu file output atau mengeluarkannya sebagai file terpisah.

---

### 4.2 Bulk Media Transcoder & Processor Engine

#### FR-2.1: Multi-Format Batch Conversion

* **Video Formats:** MP4, MKV, MOV, WebM, AVI, TS.
* **Audio Formats:** MP3, WAV, FLAC, AAC, OGG, M4A.
* **Image Formats:** PNG, JPG, WebP, AVIF, HEIC/HEIF.
* **Fitur:** Dukungan penambahan antrean massal via *drag-and-drop* direktori atau file berantai.

#### FR-2.2: Preset & Custom Encoding Configuration

* **Presets:** "Discord 25MB Limit", "Web-Optimized AVIF", "4K to 1080p Downscale", "Audio Extractor (FLAC/MP3)".
* **Custom Controls:**
* Kontrol Rate (CRF / CQ / CBR / VBR).
* Pemilihan Codec Video (H.264, H.265, AV1, VP9, ProRes).
* Resizing & Aspect Ratio Scaling (Lanczos / Bicubic filtering).



#### FR-2.3: Metadata Stripper & Privacy Engine

* Opsi *toggle* instan untuk menghapus seluruh data EXIF/IPTC/XMP dari file gambar, audio, dan video sebelum proses ekspor.

---

### 4.3 Native UI/UX System (Iced Framework)

```
┌────────────────────────────────────────────────────────────────────────────────────────┐
│  Media Cut N Play v1.0                                                  [-][□][X]     │
├──────────────────────┬──────────────────────────────────────────┬──────────────────────┤
│ MEDIA QUEUE (BATCH)  │ PREVIEW WINDOW                           │ ENCODING SETTINGS    │
│ -------------------- │ ┌──────────────────────────────────────┐ │ -------------------- │
│ [+] Add Files        │ │                                      │ │ Preset: [ Custom   ▼]│
│ [x] Clear Queue      │ │                                      │ │                      │
│                      │ │            [ VIDEO FRAME ]           │ │ Codec:  [ H.265/HEVC]│
│ 📄 video_01.mp4      │ │                                      │ │ HW Acc: [ NVENC    ▼]│
│    1080p | 1.2 GB    │ │                                      │ │ Quality:[ CRF 23  █]│
│                      │ └──────────────────────────────────────┘ │                      │
│ 📄 clip_02.mkv       │  ⏮  ⏯  ⏭   00:04:12 / 00:15:30         │ 🔲 Strip EXIF        │
│    4K | 3.4 GB       │ ──────────────────────────────────────── │ 🔲 Lossless Cut      │
│                      │ TIMELINE SCRUBBER                        │ ☑ Smart Cut          │
│ 📄 image_batch.zip   │ [||||||||||||||||||||||||||||||||||||||] │                      │
│    150 Files         │      ▲ In-Point            ▲ Out-Point │ [ START BATCH EXPORT]│
├──────────────────────┴──────────────────────────────────────────┴──────────────────────┤
│ Status: Ready | GPU: NVIDIA RTX 4060 (NVENC Active) | RAM: 28.4 MB                      │
└────────────────────────────────────────────────────────────────────────────────────────┘

```

#### UI Components Breakdown:

1. **Panel Kiri (Media Queue Panel):** List item antrean dengan indikator status (*Pending, Processing, Done, Failed*), info resolusi, dan ukuran file.
2. **Panel Tengah (Player & Custom Timeline Canvas):**
* Window *preview* berbasis render `iced::widget::image` langsung dari RGBA buffer decoding FFmpeg.
* Custom Canvas Widget untuk *Timeline Scrubber*, visual penanda *In/Out Point*, penanda *Keyframe* (I-Frame ticks), dan visualisasi *Audio Waveform*.


3. **Panel Kanan (Inspector & Export Controls):** Pilihan preset konversi, pengaturan codec, *slider* CRF/Bitrate, serta switch mode pemotongan (*Lossless vs Smart Cut*).
4. **Panel Bawah (Status & Resource Bar):** Menampilkan penggunaan memori real-time, status akselerasi hardware GPU yang terdeteksi, dan *progress bar* konversi batch.

---

## 5. Non-Functional Requirements (NFR)

### 5.1 Performance & Resource Efficiency

* **Idle RAM Usage:** Tidak boleh melebihi **35 MB RAM**.
* **Active Scrubbing RAM Usage:** Peak RAM tidak boleh melebihi **150 MB** saat mendekode video 4K H.265.
* **Startup Time:** Cold boot < **500 ms** pada NVMe SSD.
* **Zero IPC Overhead:** Seluruh alur data dari file I/O → FFmpeg Decode → Render Frame Iced berjalan dalam satu alokasi memori process space.

### 5.2 Privacy & Security

* **100% Offline Capability:** Aplikasi wajib dapat berjalan penuh tanpa koneksi internet. Tidak ada telemetry, tracking, atau request HTTP latar belakang.
* **Memory Safety:** Tidak ada *undefined behavior* di luar *FFmpeg C-FFI boundary* (seluruh panggilan FFI di-wrap menggunakan abstraksi Rust yang aman).

### 5.3 Compatibility & Portability

* **Standalone Binary:** Distribusi biner tunggal tanpa dependensi eksternal (*portable executable*).
* **Cross-Platform Parity:** Fitur dan performa antarmuka identik di Linux (X11/Wayland), Windows, dan macOS.

---

## 6. Architecture & Elm State Model (Iced Specification)

### 6.1 State Management Structure

```rust
pub struct AppState {
    // Media Management
    pub queue: Vec<MediaItem>,
    pub active_index: Option<usize>,
    
    // Playback & Trimmer State
    pub is_playing: bool,
    pub current_timestamp: f64,
    pub in_point: Option<f64>,
    pub out_point: Option<f64>,
    pub current_frame_buffer: Option<iced::widget::image::Handle>,
    
    // Settings & Hardware
    pub export_config: ExportConfig,
    pub detected_gpu: GpuVendor,
    
    // Batch Execution
    pub processing_status: ProcessState,
}

```

### 6.2 Event Message Flow

```rust
#[derive(Debug, Clone)]
pub enum Message {
    // Queue Events
    FileDropped(Vec<std::path::PathBuf>),
    SelectQueueItem(usize),
    RemoveQueueItem(usize),
    
    // Timeline Events
    SeekToTimestamp(f64),
    SetInPoint,
    SetOutPoint,
    ClearCutPoints,
    
    // Decoding & Frame Pipeline
    FrameDecoded(Result<iced::widget::image::Handle, DecodeError>),
    
    // Export Operations
    StartExport,
    ExportProgressUpdated { index: usize, progress: f32 },
    ExportCompleted(usize),
    
    // UI Interactions
    UpdateCodec(VideoCodec),
    ToggleMetadataStripper(bool),
    SetQualitySlider(f32),
}

```

---

## 7. Milestone & Release Roadmap

### Phase 1: MVP Core Engine (Minggu 1 – 4)

* [x] Inisialisasi proyek Iced + Tokio.
* [x] Integrasi FFI FFmpeg (`ffmpeg-next` / `rsmpeg`) untuk pembacaan metadata dan dekode frame RGBA.
* [x] Implementasi fitur **Lossless Cut** dasar via Rust async task.
* [x] Layout UI dasar (Queue List, Player Preview, Preset Options).

### Phase 2: Smart Cut & Batch Processor (Minggu 5 – 8)

* [x] Pengembangan algoritma **Smart Cut** (Head Re-encode + Body Stream Copy + Concat).
* [x] Integrasi deteksi akselerasi GPU (NVENC, QSV, VideoToolbox).
* [x] Modul pemrosesan batch untuk konversi audio/gambar dan pembersihan EXIF metadata.
* [x] Custom Canvas Widget untuk *Timeline Scrubber* dan penanda *In/Out Point*.

### Phase 3: Optimization & Polish (Minggu 9 – 12)

* [x] Visualisasi *Audio Waveform* di timeline canvas.
* [x] Pengujian performa lintang platform (Linux Wayland, Windows 11, macOS M-Series).
* [x] Stabilitas *memory leak check* menggunakan Valgrind / Heaptrack.
* [x] Packaging biner tunggal (.AppImage/tar.gz untuk Linux, .exe portabel untuk Windows, .dmg untuk macOS).

---

## 8. Success Metrics (KPIs)

1. **Kecepatan Lossless Cut:** Pemotongan video 1 GB selesai dalam waktu **< 3 detik** pada NVMe Storage.
2. **Kecepatan Smart Cut:** Pemotongan presisi frame pada file 1 GB selesai dalam waktu **< 8 detik** (hanya 2-5% video yang di-encode ulang).
3. **Efisiensi Memory Footprint:** Konsumsi RAM < **35 MB** pada posisi idle dan < **150 MB** saat proses konversi batch berjalan.
4. **Stabilitas Stres Tes:** Berhasil memproses antrean 1.000+ file media secara berurutan tanpa *crash* atau *memory leak*.
