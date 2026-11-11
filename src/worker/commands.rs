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
