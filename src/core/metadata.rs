use crate::core::error::MediaError;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct VideoMetadata {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub pixel_format: String,
}

#[derive(Debug, Clone)]
pub struct AudioMetadata {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone)]
pub struct MediaMetadata {
    pub file_path: PathBuf,
    pub file_size_bytes: u64,
    pub duration_secs: f64,
    pub bitrate: i64,
    pub format_name: String,
    pub video: Option<VideoMetadata>,
    pub audio: Option<AudioMetadata>,
}

pub fn probe_file(path: &Path) -> Result<MediaMetadata, MediaError> {
    let ictx = ffmpeg_next::format::input(path)?;
    let file_size_bytes = std::fs::metadata(path)
        .map(|m| m.len())
        .unwrap_or(0);

    let duration_secs = if ictx.duration() > 0 {
        ictx.duration() as f64 / f64::from(ffmpeg_next::ffi::AV_TIME_BASE)
    } else {
        0.0
    };

    let bitrate = ictx.bit_rate();
    let format_name = ictx.format().name().to_string();

    let mut video = None;
    let mut audio = None;

    for stream in ictx.streams() {
        let codecpar = stream.parameters();
        match codecpar.medium() {
            ffmpeg_next::media::Type::Video if video.is_none() => {
                if let Ok(ctx) = ffmpeg_next::codec::context::Context::from_parameters(codecpar.clone()) {
                    if let Ok(decoder) = ctx.decoder().video() {
                        let rate = stream.rate();
                        let fps = if rate.denominator() > 0 {
                            f64::from(rate.numerator()) / f64::from(rate.denominator())
                        } else {
                            f64::from(stream.avg_frame_rate().numerator()) / f64::from(stream.avg_frame_rate().denominator().max(1))
                        };
                        video = Some(VideoMetadata {
                            codec: decoder.codec().map(|c| c.name().to_string()).unwrap_or_default(),
                            width: decoder.width(),
                            height: decoder.height(),
                            fps: if fps.is_nan() || fps <= 0.0 { 30.0 } else { fps },
                            pixel_format: format!("{:?}", decoder.format()),
                        });
                    }
                }
            }
            ffmpeg_next::media::Type::Audio if audio.is_none() => {
                if let Ok(ctx) = ffmpeg_next::codec::context::Context::from_parameters(codecpar.clone()) {
                    if let Ok(decoder) = ctx.decoder().audio() {
                        audio = Some(AudioMetadata {
                            codec: decoder.codec().map(|c| c.name().to_string()).unwrap_or_default(),
                            sample_rate: decoder.rate(),
                            channels: decoder.channels(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    Ok(MediaMetadata {
        file_path: path.to_path_buf(),
        file_size_bytes,
        duration_secs,
        bitrate,
        format_name,
        video,
        audio,
    })
}
