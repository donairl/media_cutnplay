use std::fmt;

#[derive(Debug, Clone)]
pub enum MediaError {
    Ffmpeg(String),
    Io(String),
    NoVideoStream,
    NoAudioStream,
    SeekFailed(String),
    CutFailed(String),
    TranscodeFailed(String),
    Cancelled,
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ffmpeg(msg) => write!(f, "FFmpeg error: {msg}"),
            Self::Io(msg) => write!(f, "I/O error: {msg}"),
            Self::NoVideoStream => write!(f, "No video stream found in file"),
            Self::NoAudioStream => write!(f, "No audio stream found in file"),
            Self::SeekFailed(msg) => write!(f, "Seek failed: {msg}"),
            Self::CutFailed(msg) => write!(f, "Cut failed: {msg}"),
            Self::TranscodeFailed(msg) => write!(f, "Transcode failed: {msg}"),
            Self::Cancelled => write!(f, "Cancelled"),
        }
    }
}

impl std::error::Error for MediaError {}

impl From<ffmpeg_next::Error> for MediaError {
    fn from(err: ffmpeg_next::Error) -> Self {
        Self::Ffmpeg(err.to_string())
    }
}

impl From<std::io::Error> for MediaError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}
