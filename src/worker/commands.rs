use crate::core::cutter::{CutMode, Segment};
use crate::core::error::MediaError;
use crate::core::metadata::MediaMetadata;
use crate::core::transcode::TranscodeSettings;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[derive(Debug)]
pub enum WorkerCommand {
    /// Probe a file's metadata for the queue.
    LoadFile(PathBuf),
    /// Cut the segments out of the input: joined into `output_path` when `merge`,
    /// otherwise one numbered file per segment.
    StartLosslessCut {
        input_path: PathBuf,
        output_path: PathBuf,
        segments: Vec<Segment>,
        merge: bool,
        mode: CutMode,
    },
    /// Re-encode the whole file. Runs on its own thread, so probing and
    /// cutting stay available; setting `cancel` stops it.
    StartTranscode {
        input_path: PathBuf,
        output_path: PathBuf,
        settings: TranscodeSettings,
        cancel: Arc<AtomicBool>,
    },
}

#[derive(Debug)]
pub enum WorkerEvent {
    MetadataLoaded(MediaMetadata),
    CutProgress(f32),
    /// The files written, in segment order.
    CutCompleted(Result<Vec<PathBuf>, MediaError>),
    TranscodeProgress(f32),
    TranscodeCompleted(Result<PathBuf, MediaError>),
    Error(MediaError),
}
