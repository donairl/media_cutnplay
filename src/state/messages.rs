use crate::core::cutter::CutMode;
use crate::core::metadata::MediaMetadata;
use crate::core::transcode::{NvencSupport, TranscodeSettings};
use crate::player::PlayerEvent;
use crate::state::app_state::AdjustKind;
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
    /// Relative seek in seconds (keyboard jumps).
    SeekBy(f64),
    ToggleFullscreen,

    // Fullscreen pointer tracking (drives the auto-hiding controls)
    WindowResized(f32),
    CursorMoved(f32),
    CursorLeft,
    PointerPressed(bool),

    // Side panels
    ToggleQueuePanel,
    ToggleExportPanel,
    ToggleTranscodePanel,

    // Video settings (preview only)
    ToggleAdjustPanel(AdjustKind),
    SetAdjust(AdjustKind, f32),
    ResetAdjust(AdjustKind),
    ToggleMonochrome,

    // Cut Markers
    SetInPoint,
    SetOutPoint,
    ClearPoints,
    /// Commit the current in/out pair to the segment list.
    AddSegment,
    RemoveSegment(usize),
    ClearSegments,
    /// Export all segments joined into one file instead of one file each.
    SetMergeSegments(bool),
    SetCutMode(CutMode),

    // Export Controls
    SelectOutputPath,
    OutputPathSelected(Option<PathBuf>),
    StartLosslessCut,

    // Transcode
    SetTranscode(TranscodeSettings),
    SelectTranscodeOutput,
    TranscodeOutputSelected(Option<PathBuf>),
    StartTranscode,
    CancelTranscode,
    NvencProbed(NvencSupport),

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
    CutCompleted(Result<Vec<PathBuf>, String>),
    TranscodeProgress(f32),
    /// `None` when the job was cancelled.
    TranscodeCompleted(Option<Result<PathBuf, String>>),
    Error(String),
}
