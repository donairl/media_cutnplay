//! Frame-accurate cutting that stays lossless wherever it can.
//!
//! Video is handled one GOP (keyframe to keyframe) at a time. A GOP lying
//! wholly inside a segment is stream-copied; a GOP the segment only partly
//! covers is decoded and just the wanted frames are re-encoded. Audio and
//! subtitles are always copied, trimmed to packet boundaries.
//!
//! Re-encoded frames come from a different encoder than the source, so they
//! carry their own SPS/PPS in-band, and every copied keyframe is prefixed
//! with the source's so a decoder is always holding the right set.

use crate::core::cutter::Segment;
use crate::core::error::MediaError;
use ffmpeg_next::{codec, encoder, format, frame, media, picture, Dictionary, Packet, Rational};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Slack for comparing timestamps that went through floating point.
const EPS: f64 = 0.0005;
/// How far past a segment's end to keep reading for audio that is muxed
/// behind the video.
const LOOKAHEAD_SECS: f64 = 2.0;

/// Cuts the segments, in the order given, into one file, re-encoding only
/// the frames between a cut point and its nearest keyframe.
pub fn smart_cut<F>(
    input_path: &Path,
    output_path: &Path,
    segments: &[Segment],
    on_progress: F,
) -> Result<PathBuf, MediaError>
where
    F: FnMut(f32),
{
    let result = run(input_path, output_path, segments, on_progress);
    if result.is_err() {
        // Never leave a half-written file that looks like a finished cut.
        let _ = std::fs::remove_file(output_path);
    }
    result
}

fn run<F>(
    input_path: &Path,
    output_path: &Path,
    segments: &[Segment],
    mut on_progress: F,
) -> Result<PathBuf, MediaError>
where
    F: FnMut(f32),
{
    if segments.is_empty() {
        return Err(MediaError::CutFailed("No segment to cut".into()));
    }
    if segments.iter().any(|s| s.start >= s.end) {
        return Err(MediaError::CutFailed("In-point must be less than out-point".into()));
    }

    let mut ictx = format::input(input_path)?;
    let mut octx = format::output(output_path)?;

    let main_video = ictx.streams().best(media::Type::Video).map(|s| s.index());
    let mut mapping: HashMap<usize, usize> = HashMap::new();
    let mut time_bases: HashMap<usize, Rational> = HashMap::new();
    let mut audio_streams: HashSet<usize> = HashSet::new();
    let mut video = None;

    for (src_idx, istream) in ictx.streams().enumerate() {
        let params = istream.parameters();
        let medium = params.medium();
        let is_main_video = Some(src_idx) == main_video;
        // Extra video streams are cover art; they have no place in a cut.
        let wanted = is_main_video
            || medium == media::Type::Audio
            || medium == media::Type::Subtitle;
        if !wanted {
            continue;
        }

        let mut ostream = octx.add_stream(encoder::find(params.id()))?;
        ostream.set_parameters(params.clone());
        mapping.insert(src_idx, ostream.index());
        time_bases.insert(src_idx, istream.time_base());

        if medium == media::Type::Audio {
            audio_streams.insert(src_idx);
        }
        if is_main_video {
            if params.id() != codec::Id::H264 {
                return Err(MediaError::CutFailed(
                    "Smart cut supports H.264 video only; use the lossless mode for this file"
                        .into(),
                ));
            }
            video = Some(VideoTrack {
                src_idx,
                out_idx: ostream.index(),
                time_base: istream.time_base(),
                frame_rate: istream.avg_frame_rate(),
                headers: Headers::parse(&extradata(&params)),
                params,
            });
        }
    }

    octx.write_header()?;

    let mut job = Job {
        octx,
        video,
        delay: 0,
        last_video_dts: None,
    };

    let total = segments.iter().map(Segment::duration).sum::<f64>().max(0.001);
    let mut done = 0.0;
    // Packets arrive in decode order, so their times jitter; only ever
    // report forward progress.
    let mut reported = 0.0_f32;

    for segment in segments {
        // Output time = source time + shift: segments sit back to back.
        let shift_sec = done - segment.start;

        let video_stream = job.video.as_ref().map(|v| (v.src_idx, f64::from(v.time_base)));
        seek_to_keyframe_before(&mut ictx, video_stream, segment.start);

        let mut gop: Vec<Packet> = Vec::new();
        let mut video_done = job.video.is_none();
        let mut audio_done: HashSet<usize> = HashSet::new();

        for (stream, mut packet) in ictx.packets() {
            let src_idx = stream.index();
            let Some(&out_idx) = mapping.get(&src_idx) else {
                continue;
            };
            let time_base = time_bases[&src_idx];
            let tb_secs = f64::from(time_base);
            let pts_sec = packet.pts().or(packet.dts()).unwrap_or(0) as f64 * tb_secs;
            let dts_sec = packet.dts().or(packet.pts()).unwrap_or(0) as f64 * tb_secs;

            if job.video.as_ref().is_some_and(|v| v.src_idx == src_idx) {
                if !video_done {
                    if packet.is_key() && !gop.is_empty() {
                        job.finish_gop(&mut gop, GopEnd::Keyframe(pts_sec), segment, shift_sec)?;
                    }
                    // Decode time passing the end means every later frame
                    // is past it too.
                    if dts_sec >= segment.end - EPS {
                        video_done = true;
                        job.finish_gop(&mut gop, GopEnd::Cut, segment, shift_sec)?;
                    } else {
                        gop.push(packet);
                    }
                }
            } else if pts_sec >= segment.end - EPS {
                audio_done.insert(src_idx);
            } else if pts_sec >= segment.start - EPS {
                let shift = (shift_sec / tb_secs).round() as i64;
                packet.set_pts(packet.pts().map(|t| t + shift));
                packet.set_dts(packet.dts().map(|t| t + shift));
                job.write(packet, out_idx, time_base)?;
            }

            let copied = (dts_sec - segment.start).clamp(0.0, segment.duration());
            let progress = ((done + copied) / total) as f32;
            if progress > reported {
                reported = progress;
                on_progress(progress);
            }

            let audio_finished = audio_streams.iter().all(|s| audio_done.contains(s));
            if video_done && (audio_finished || dts_sec >= segment.end + LOOKAHEAD_SECS) {
                break;
            }
        }

        // The file ended inside the segment.
        job.finish_gop(&mut gop, GopEnd::EndOfFile, segment, shift_sec)?;

        done += segment.duration();
    }

    job.octx.write_trailer()?;
    on_progress(1.0);

    Ok(output_path.to_path_buf())
}

/// Positions the input on a video keyframe at or before `start`.
///
/// Some containers (MPEG-TS) seek only roughly and can land past the
/// target, which would silently drop the segment's first frames. So look at
/// where the seek landed and back off further until it is early enough.
fn seek_to_keyframe_before(
    ictx: &mut format::context::Input,
    video: Option<(usize, f64)>,
    start: f64,
) {
    let seek = |ictx: &mut format::context::Input, secs: f64| {
        let target = (secs * f64::from(ffmpeg_next::ffi::AV_TIME_BASE)) as i64;
        let _ = ictx.seek(target, ..target);
    };

    let mut margin = 0.0;
    loop {
        let target = start - margin;
        seek(ictx, target);
        let Some((video_idx, tb_secs)) = video else {
            return;
        };

        let first_keyframe = ictx
            .packets()
            .find(|(stream, packet)| stream.index() == video_idx && packet.is_key())
            .and_then(|(_, packet)| packet.pts().or(packet.dts()))
            .map(|pts| pts as f64 * tb_secs);

        if first_keyframe.is_some_and(|key| key <= start + EPS) {
            // Looking consumed the packets; go back to the same spot.
            seek(ictx, target);
            return;
        }
        if target <= 0.0 {
            // Nothing earlier to try: start from the top of the file.
            let _ = ictx.seek(0, ..);
            return;
        }
        margin = if margin == 0.0 { 1.0 } else { margin * 4.0 };
    }
}

/// What closed a GOP.
enum GopEnd {
    /// The next keyframe, at this presentation time in seconds.
    Keyframe(f64),
    /// Reading stopped at the segment end; the GOP runs on past it.
    Cut,
    EndOfFile,
}

struct VideoTrack {
    src_idx: usize,
    out_idx: usize,
    time_base: Rational,
    frame_rate: Rational,
    params: codec::Parameters,
    headers: Headers,
}

struct Job {
    octx: format::context::Output,
    video: Option<VideoTrack>,
    /// Ticks the source's decode time runs ahead of presentation time
    /// (B-frame reordering); re-encoded packets are given the same lead so
    /// they slot in between copied ones.
    delay: i64,
    last_video_dts: Option<i64>,
}

impl Job {
    /// Emits the buffered GOP: copied when the segment covers all of it,
    /// re-encoded when it covers only part, dropped when it covers none.
    fn finish_gop(
        &mut self,
        gop: &mut Vec<Packet>,
        end: GopEnd,
        segment: &Segment,
        shift_sec: f64,
    ) -> Result<(), MediaError> {
        let packets = std::mem::take(gop);
        let (Some(first), Some(video)) = (packets.first(), self.video.as_ref()) else {
            return Ok(());
        };
        // Without its keyframe the GOP cannot be decoded, let alone copied.
        if !first.is_key() {
            return Ok(());
        }

        let tb_secs = f64::from(video.time_base);
        let first_pts = first.pts().or(first.dts()).unwrap_or(0);
        let gop_start = first_pts as f64 * tb_secs;
        let gop_end = match end {
            GopEnd::Keyframe(next) => next,
            GopEnd::Cut => f64::INFINITY,
            GopEnd::EndOfFile => packets
                .iter()
                .map(|p| (p.pts().unwrap_or(first_pts) + p.duration().max(1)) as f64 * tb_secs)
                .fold(gop_start, f64::max),
        };

        if gop_end <= segment.start + EPS || gop_start >= segment.end - EPS {
            return Ok(());
        }

        // Frames shown before their GOP's keyframe lean on the previous GOP,
        // so this GOP cannot be cut loose from it.
        if packets.iter().any(|p| p.pts().is_some_and(|pts| pts < first_pts)) {
            return Err(MediaError::CutFailed(
                "Smart cut does not support open-GOP video; use the lossless mode for this file"
                    .into(),
            ));
        }

        if let Some(dts) = first.dts() {
            self.delay = (first_pts - dts).max(0);
        }
        let shift = (shift_sec / tb_secs).round() as i64;

        if gop_start >= segment.start - EPS && gop_end <= segment.end + EPS {
            self.copy_gop(packets, shift)
        } else {
            self.reencode_gop(packets, segment, shift)
        }
    }

    fn copy_gop(&mut self, packets: Vec<Packet>, shift: i64) -> Result<(), MediaError> {
        for mut packet in packets {
            if packet.is_key() {
                if let Some(video) = &self.video {
                    packet = video.headers.prefixed(&packet);
                }
            }
            packet.set_pts(packet.pts().map(|t| t + shift));
            packet.set_dts(packet.dts().map(|t| t + shift));
            self.write_video(packet)?;
        }
        Ok(())
    }

    /// Decodes the GOP and encodes the frames that fall inside the segment.
    fn reencode_gop(
        &mut self,
        packets: Vec<Packet>,
        segment: &Segment,
        shift: i64,
    ) -> Result<(), MediaError> {
        let Some(video) = self.video.as_ref() else {
            return Ok(());
        };
        let tb_secs = f64::from(video.time_base);
        let mut decoder = codec::context::Context::from_parameters(video.params.clone())?
            .decoder()
            .video()?;

        let mut frames: Vec<frame::Video> = Vec::new();
        let mut decoded = frame::Video::empty();
        let mut collect = |decoder: &mut ffmpeg_next::decoder::Video| {
            while decoder.receive_frame(&mut decoded).is_ok() {
                let Some(pts) = decoded.timestamp().or(decoded.pts()) else {
                    continue;
                };
                let sec = pts as f64 * tb_secs;
                if sec >= segment.start - EPS && sec < segment.end - EPS {
                    let mut kept = decoded.clone();
                    kept.set_pts(Some(pts));
                    // Let the encoder pick its own frame types.
                    kept.set_kind(picture::Type::None);
                    frames.push(kept);
                }
            }
        };

        for packet in &packets {
            // A damaged packet costs its frames, not the whole cut.
            let _ = decoder.send_packet(packet);
            collect(&mut decoder);
        }
        let _ = decoder.send_eof();
        collect(&mut decoder);

        let Some(first) = frames.first() else {
            return Ok(());
        };
        let mut encoder = open_encoder(video, first)?;
        // Encoders may leave the duration blank; fall back to one frame.
        let frame_ticks = {
            let fps = f64::from(video.frame_rate);
            if fps > 0.0 { (1.0 / (fps * tb_secs)).round().max(1.0) as i64 } else { 0 }
        };

        let mut encoded = Packet::empty();
        for frame in &frames {
            encoder.send_frame(frame)?;
            while encoder.receive_packet(&mut encoded).is_ok() {
                self.write_encoded(&encoded, shift, frame_ticks)?;
            }
        }
        encoder.send_eof()?;
        while encoder.receive_packet(&mut encoded).is_ok() {
            self.write_encoded(&encoded, shift, frame_ticks)?;
        }
        Ok(())
    }

    fn write_encoded(
        &mut self,
        encoded: &Packet,
        shift: i64,
        frame_ticks: i64,
    ) -> Result<(), MediaError> {
        let Some(video) = self.video.as_ref() else {
            return Ok(());
        };
        let mut packet = Packet::copy(&video.headers.from_annex_b(encoded.data().unwrap_or(&[])));
        packet.set_flags(encoded.flags());
        // The encoder runs without B-frames, so pts is also its decode order.
        let pts = encoded.pts().or(encoded.dts()).unwrap_or(0) + shift;
        packet.set_pts(Some(pts));
        packet.set_dts(Some(pts - self.delay));
        packet.set_duration(if encoded.duration() > 0 { encoded.duration() } else { frame_ticks });
        self.write_video(packet)
    }

    fn write_video(&mut self, mut packet: Packet) -> Result<(), MediaError> {
        let Some(video) = self.video.as_ref() else {
            return Ok(());
        };
        let (out_idx, time_base) = (video.out_idx, video.time_base);

        // Muxers reject decode times that do not advance.
        if let (Some(dts), Some(last)) = (packet.dts(), self.last_video_dts) {
            if dts <= last {
                packet.set_dts(Some(last + 1));
                if packet.pts().is_some_and(|pts| pts <= last) {
                    packet.set_pts(Some(last + 1));
                }
            }
        }
        if let Some(dts) = packet.dts() {
            self.last_video_dts = Some(dts);
        }
        self.write(packet, out_idx, time_base)
    }

    fn write(
        &mut self,
        mut packet: Packet,
        out_idx: usize,
        time_base: Rational,
    ) -> Result<(), MediaError> {
        let out_tb = self
            .octx
            .stream(out_idx)
            .map(|s| s.time_base())
            .unwrap_or(time_base);
        packet.set_stream(out_idx);
        packet.rescale_ts(time_base, out_tb);
        packet.set_position(-1);
        packet.write_interleaved(&mut self.octx)?;
        Ok(())
    }
}

/// An x264 encoder matching the decoded picture, with parameter sets
/// repeated in-band on every keyframe.
fn open_encoder(
    video: &VideoTrack,
    sample: &frame::Video,
) -> Result<ffmpeg_next::encoder::Video, MediaError> {
    let codec = encoder::find_by_name("libx264").ok_or_else(|| {
        MediaError::CutFailed("Smart cut needs the libx264 encoder, which this FFmpeg lacks".into())
    })?;
    let mut encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()?;

    encoder.set_width(sample.width());
    encoder.set_height(sample.height());
    encoder.set_format(sample.format());
    encoder.set_time_base(video.time_base);
    if f64::from(video.frame_rate) > 0.0 {
        encoder.set_frame_rate(Some(video.frame_rate));
    }
    encoder.set_aspect_ratio(sample.aspect_ratio());
    encoder.set_colorspace(sample.color_space());
    encoder.set_color_range(sample.color_range());
    encoder.set_color_primaries(sample.color_primaries());
    encoder.set_color_transfer_characteristic(sample.color_transfer_characteristic());
    encoder.set_max_b_frames(0);

    let mut options = Dictionary::new();
    // Visually transparent; these are only ever a GOP's worth of frames.
    options.set("crf", "16");
    options.set("preset", "medium");
    // Keep clear of the source's parameter-set id (almost always 0).
    options.set("x264-params", "sps-id=3");

    Ok(encoder.open_with(options)?)
}

fn extradata(params: &codec::Parameters) -> Vec<u8> {
    // SAFETY: `params` owns a valid AVCodecParameters, and FFmpeg keeps
    // `extradata` either null or pointing at `extradata_size` bytes.
    unsafe {
        let raw = params.as_ptr();
        let (data, size) = ((*raw).extradata, (*raw).extradata_size);
        if data.is_null() || size <= 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(data, size as usize).to_vec()
        }
    }
}

/// How the stream frames its NAL units, plus the source's SPS/PPS in that
/// framing, ready to be put in front of a keyframe.
#[derive(Debug, PartialEq)]
enum Headers {
    /// MP4/MKV style: each NAL unit behind a big-endian length.
    LengthPrefixed { length_size: usize, parameter_sets: Vec<u8> },
    /// Transport-stream style: NAL units behind start codes.
    AnnexB { parameter_sets: Vec<u8> },
}

impl Headers {
    fn parse(extradata: &[u8]) -> Self {
        Self::parse_avcc(extradata).unwrap_or_else(|| Self::AnnexB {
            parameter_sets: extradata.to_vec(),
        })
    }

    /// Reads an `avcC` box: configuration bytes, then the SPS list and the
    /// PPS list, each entry behind a 16-bit length.
    fn parse_avcc(data: &[u8]) -> Option<Self> {
        if data.len() < 7 || data[0] != 1 {
            return None;
        }
        let length_size = usize::from(data[4] & 0x03) + 1;
        let mut parameter_sets = Vec::new();
        let mut at = 5;

        for count_mask in [0x1F, 0xFF] {
            let count = *data.get(at)? & count_mask;
            at += 1;
            for _ in 0..count {
                let len = usize::from(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]));
                at += 2;
                push_nal(&mut parameter_sets, data.get(at..at + len)?, length_size);
                at += len;
            }
        }

        Some(Self::LengthPrefixed { length_size, parameter_sets })
    }

    /// A copy of `packet` with the source's parameter sets in front.
    fn prefixed(&self, packet: &Packet) -> Packet {
        let (Self::LengthPrefixed { parameter_sets, .. } | Self::AnnexB { parameter_sets }) = self;
        let mut data = parameter_sets.clone();
        data.extend_from_slice(packet.data().unwrap_or(&[]));

        let mut out = Packet::copy(&data);
        out.set_flags(packet.flags());
        out.set_pts(packet.pts());
        out.set_dts(packet.dts());
        out.set_duration(packet.duration());
        out
    }

    /// Reframes encoder output (always Annex B) the way the stream expects.
    fn from_annex_b(&self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::AnnexB { .. } => data.to_vec(),
            Self::LengthPrefixed { length_size, .. } => {
                let mut out = Vec::with_capacity(data.len() + 8);
                for nal in annex_b_nals(data) {
                    push_nal(&mut out, nal, *length_size);
                }
                out
            }
        }
    }
}

fn push_nal(out: &mut Vec<u8>, nal: &[u8], length_size: usize) {
    let len = (nal.len() as u32).to_be_bytes();
    out.extend_from_slice(&len[4 - length_size..]);
    out.extend_from_slice(nal);
}

/// Splits an Annex B buffer into NAL units, without their start codes.
fn annex_b_nals(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i..i + 3] == [0, 0, 1] {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }

    let mut nals = Vec::with_capacity(starts.len());
    for (n, &start) in starts.iter().enumerate() {
        // The next start code's leading zeros belong to it, not to this NAL.
        let mut end = starts.get(n + 1).map_or(data.len(), |next| next - 3);
        while end > start && data[end - 1] == 0 {
            end -= 1;
        }
        if end > start {
            nals.push(&data[start..end]);
        }
    }
    nals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_annex_b_on_both_start_code_lengths() {
        let data = [0, 0, 0, 1, 0x67, 0xAA, 0, 0, 1, 0x68, 0xBB, 0, 0, 0, 1, 0x65, 1, 2];
        assert_eq!(
            annex_b_nals(&data),
            vec![&[0x67, 0xAA][..], &[0x68, 0xBB][..], &[0x65, 1, 2][..]]
        );
    }

    #[test]
    fn parses_avcc_parameter_sets() {
        // version, profile, compat, level, 4-byte lengths, 1 SPS, 1 PPS
        let avcc = [1, 100, 0, 31, 0xFF, 0xE1, 0, 2, 0x67, 0xAA, 1, 0, 1, 0x68];
        assert_eq!(
            Headers::parse(&avcc),
            Headers::LengthPrefixed {
                length_size: 4,
                parameter_sets: vec![0, 0, 0, 2, 0x67, 0xAA, 0, 0, 0, 1, 0x68],
            }
        );
    }

    #[test]
    fn start_coded_extradata_is_annex_b() {
        let extradata = [0, 0, 0, 1, 0x67, 0xAA];
        assert_eq!(
            Headers::parse(&extradata),
            Headers::AnnexB { parameter_sets: extradata.to_vec() }
        );
    }

    #[test]
    fn reframes_encoder_output_to_length_prefixes() {
        let headers = Headers::LengthPrefixed { length_size: 4, parameter_sets: Vec::new() };
        assert_eq!(
            headers.from_annex_b(&[0, 0, 0, 1, 0x65, 9, 0, 0, 1, 0x41, 7]),
            vec![0, 0, 0, 2, 0x65, 9, 0, 0, 0, 2, 0x41, 7]
        );
    }
}
