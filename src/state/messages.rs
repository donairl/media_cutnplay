use crate::core::metadata::MediaMetadata;
use crate::player::PlayerEvent;
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

    // Fullscreen pointer tracking (drives the auto-hiding controls)
    WindowResized(f32),
    CursorMoved(f32),
    CursorLeft,
    PointerPressed(bool),

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

    // Playback engine
    PlayerEvent(PlayerEvent),
    /// Display refresh while playing: present whichever frame is due.
    FrameTick,

    // System Telemetry
    TickTelemetry,
}

#[derive(Debug, Clone)]
pub enum WorkerEventWrapper {
    MetadataLoaded(MediaMetadata),
    CutProgress(f32),
    CutCompleted(Result<PathBuf, String>),
    Error(String),
}
