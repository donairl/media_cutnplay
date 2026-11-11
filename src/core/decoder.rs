use crate::core::error::MediaError;
use ffmpeg_next::format::context::Input;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{Context as SwsContext, Flags};
use ffmpeg_next::util::frame::video::Video as VideoFrame;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub timestamp: f64,
    pub width: u32,
    pub height: u32,
    pub rgba_data: Vec<u8>,
}

pub struct VideoDecoder {
    ictx: Input,
    video_stream_index: usize,
    decoder: ffmpeg_next::decoder::Video,
    scaler: SwsContext,
    time_base: ffmpeg_next::Rational,
}

impl VideoDecoder {
    pub fn open(path: &Path) -> Result<Self, MediaError> {
        let ictx = ffmpeg_next::format::input(path)?;
        let video_stream = ictx
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .ok_or(MediaError::NoVideoStream)?;

        let video_stream_index = video_stream.index();
        let time_base = video_stream.time_base();

        let ctx = ffmpeg_next::codec::context::Context::from_parameters(video_stream.parameters())?;
        let decoder = ctx.decoder().video()?;

        let width = decoder.width();
        let height = decoder.height();
        let format = decoder.format();

        let scaler = SwsContext::get(
            format,
            width,
            height,
            Pixel::RGBA,
            width,
            height,
            Flags::BILINEAR,
        )?;

        Ok(Self {
            ictx,
            video_stream_index,
            decoder,
            scaler,
            time_base,
        })
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.decoder.width(), self.decoder.height())
    }

    pub fn seek_and_decode(&mut self, target_sec: f64) -> Result<DecodedFrame, MediaError> {
        // Seek in AV_TIME_BASE units because ffmpeg-next context.seek passes stream_index = -1
        let target_av_pts = (target_sec * f64::from(ffmpeg_next::ffi::AV_TIME_BASE)) as i64;
        
        let _ = self.ictx.seek(target_av_pts, ..target_av_pts);
        self.decoder.flush();

        let mut decoded_frame = VideoFrame::empty();
        let mut best_frame = None;

        for (stream, packet) in self.ictx.packets() {
            if stream.index() == self.video_stream_index {
                let _ = self.decoder.send_packet(&packet);
                while self.decoder.receive_frame(&mut decoded_frame).is_ok() {
                    let pts = decoded_frame.pts().unwrap_or(0);
                    let sec = pts as f64 * f64::from(self.time_base);

                    let mut rgb_frame = VideoFrame::empty();
                    if self.scaler.run(&decoded_frame, &mut rgb_frame).is_ok() {
                        let width = rgb_frame.width();
                        let height = rgb_frame.height();
                        let data = rgb_frame.data(0);
                        let linesize = rgb_frame.stride(0);

                        let mut rgba_data = Vec::with_capacity((width * height * 4) as usize);
                        for y in 0..height {
                            let start = (y as usize) * linesize;
                            let end = start + (width as usize) * 4;
                            if end <= data.len() {
                                rgba_data.extend_from_slice(&data[start..end]);
                            }
                        }

                        let frame_res = DecodedFrame {
                            timestamp: sec,
                            width,
                            height,
                            rgba_data,
                        };

                        if sec >= target_sec {
                            return Ok(frame_res);
                        }
                        best_frame = Some(frame_res);
                    }
                }
            }
        }

        best_frame.ok_or_else(|| MediaError::SeekFailed(format!("No frame found near {target_sec}s")))
    }
}
