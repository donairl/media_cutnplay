use crate::core::metadata::MediaMetadata;
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
    OutputPathSelected(Option<PathBuf>),
    StartLosslessCut,

    // Worker Event Dispatch
    WorkerEventReceived(WorkerEventWrapper),

    // System Telemetry & Playback Ticks
    TickTelemetry,
    TickPlayback,
}

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
