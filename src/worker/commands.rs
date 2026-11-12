use crate::core::error::MediaError;
use crate::core::metadata::MediaMetadata;
use std::path::PathBuf;

#[derive(Debug)]
pub enum WorkerCommand {
    /// Probe a file's metadata for the queue.
    LoadFile(PathBuf),
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
    CutProgress(f32),
    CutCompleted(Result<PathBuf, MediaError>),
    Error(MediaError),
}
