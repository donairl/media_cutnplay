use crate::core::error::MediaError;
use crate::core::smart_cut::smart_cut;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A span of the source, in seconds, kept by a cut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
}

impl Segment {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
}

/// How a cut treats frames between a cut point and its nearest keyframe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CutMode {
    /// Pure stream copy: instant, but each segment starts on a keyframe.
    #[default]
    Lossless,
    /// Frame-accurate: re-encodes only the partial GOPs at the cut points.
    Smart,
}

/// Adds `new` to a sorted list of non-overlapping segments, fusing it with
/// any it touches so the list stays sorted and non-overlapping.
pub fn insert_segment(segments: &mut Vec<Segment>, new: Segment) {
    let mut merged = new;
    segments.retain(|s| {
        let touches = s.start <= merged.end && merged.start <= s.end;
        if touches {
            merged.start = merged.start.min(s.start);
            merged.end = merged.end.max(s.end);
        }
        !touches
    });
    let at = segments.partition_point(|s| s.start < merged.start);
    segments.insert(at, merged);
}

/// `clip.mp4` -> `clip_02.mp4` for the second of several output files.
pub fn numbered_path(base: &Path, number: usize) -> PathBuf {
    let stem = base.file_stem().and_then(|s| s.to_str()).unwrap_or("media");
    let name = match base.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{stem}_{number:02}.{ext}"),
        None => format!("{stem}_{number:02}"),
    };
    base.with_file_name(name)
}

/// Stream-copies one span of the input into `output_path`.
pub fn lossless_cut<F>(
    input_path: &Path,
    output_path: &Path,
    in_point: f64,
    out_point: f64,
    on_progress: F,
) -> Result<PathBuf, MediaError>
where
    F: FnMut(f32),
{
    let segment = Segment {
        start: in_point,
        end: out_point,
    };
    lossless_concat(input_path, output_path, &[segment], on_progress)
}

/// Cuts the segments out of the input. With `merge` they are joined back to
/// back in `output_path`; otherwise each gets its own numbered file next to
/// it. A single segment always lands in `output_path` itself.
pub fn export_segments<F>(
    input_path: &Path,
    output_path: &Path,
    segments: &[Segment],
    merge: bool,
    mode: CutMode,
    mut on_progress: F,
) -> Result<Vec<PathBuf>, MediaError>
where
    F: FnMut(f32),
{
    let cut = |output: &Path, segments: &[Segment], on_progress: &mut dyn FnMut(f32)| match mode {
        CutMode::Lossless => lossless_concat(input_path, output, segments, on_progress),
        CutMode::Smart => smart_cut(input_path, output, segments, on_progress),
    };

    if merge || segments.len() <= 1 {
        return cut(output_path, segments, &mut on_progress).map(|p| vec![p]);
    }

    let total = segments.iter().map(Segment::duration).sum::<f64>().max(0.001);
    let mut done = 0.0;
    let mut outputs = Vec::with_capacity(segments.len());

    for (index, segment) in segments.iter().enumerate() {
        let path = numbered_path(output_path, index + 1);
        let share = segment.duration();
        cut(&path, &[*segment], &mut |p| {
            on_progress(((done + f64::from(p) * share) / total) as f32);
        })?;
        done += share;
        outputs.push(path);
    }

    Ok(outputs)
}

/// Stream-copies the segments, in the order given, into one file.
pub fn lossless_concat<F>(
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

    let mut ictx = ffmpeg_next::format::input(input_path)?;
    let mut octx = ffmpeg_next::format::output(output_path)?;

    // Map input stream indices to output stream indices & stream parameters
    let mut stream_mapping = HashMap::new();
    let mut in_time_bases = HashMap::new();
    let mut has_video = false;

    for (src_idx, istream) in ictx.streams().enumerate() {
        let medium = istream.parameters().medium();
        if medium == ffmpeg_next::media::Type::Video
            || medium == ffmpeg_next::media::Type::Audio
            || medium == ffmpeg_next::media::Type::Subtitle
        {
            let mut ostream = octx.add_stream(ffmpeg_next::encoder::find(istream.parameters().id()))?;
            ostream.set_parameters(istream.parameters());
            stream_mapping.insert(src_idx, ostream.index());
            in_time_bases.insert(src_idx, istream.time_base());
            has_video |= medium == ffmpeg_next::media::Type::Video;
        }
    }

    octx.write_header()?;

    let total = segments.iter().map(Segment::duration).sum::<f64>().max(0.001);
    // Source seconds already copied, and where the next segment starts in
    // the output.
    let mut done = 0.0;
    let mut offset_sec = 0.0;
    // Packets arrive in decode order, so their times jitter; only ever
    // report forward progress.
    let mut reported = 0.0_f32;

    for segment in segments {
        // Seek input to keyframe at or before the segment start
        let target_av_pts = (segment.start * f64::from(ffmpeg_next::ffi::AV_TIME_BASE)) as i64;
        let _ = ictx.seek(target_av_pts, ..target_av_pts);

        let mut start_pts_map: HashMap<usize, i64> = HashMap::new();
        let mut segment_end_sec = offset_sec;

        for (stream, mut packet) in ictx.packets() {
            let src_idx = stream.index();
            let Some(&out_idx) = stream_mapping.get(&src_idx) else {
                continue;
            };
            let in_tb = in_time_bases[&src_idx];
            let tb_secs = f64::from(in_tb);
            let out_tb = octx.stream(out_idx).map(|s| s.time_base()).unwrap_or(in_tb);

            let pkt_pts = packet.pts().unwrap_or(0);
            let pkt_sec = pkt_pts as f64 * tb_secs;

            if pkt_sec > segment.end {
                // Video decides where the segment stops; other streams just
                // drop what runs past it.
                let is_video = stream.parameters().medium() == ffmpeg_next::media::Type::Video;
                if is_video || !has_video {
                    break;
                }
                continue;
            }

            // Each stream restarts at the segment's place in the output.
            let start_pts = *start_pts_map.entry(src_idx).or_insert(pkt_pts);
            let shift = (offset_sec / tb_secs).round() as i64 - start_pts;
            if let Some(pts) = packet.pts() {
                packet.set_pts(Some(pts.saturating_add(shift)));
            }
            if let Some(dts) = packet.dts() {
                packet.set_dts(Some(dts.saturating_add(shift)));
            }

            let ticks = packet.duration().max(1);
            let end_sec = (pkt_pts + shift + ticks) as f64 * tb_secs;
            segment_end_sec = segment_end_sec.max(end_sec);

            packet.set_stream(out_idx);
            packet.rescale_ts(in_tb, out_tb);
            packet.set_position(-1);

            let _ = packet.write_interleaved(&mut octx);

            let copied = (pkt_sec - segment.start).clamp(0.0, segment.duration());
            let progress = ((done + copied) / total) as f32;
            if progress > reported {
                reported = progress;
                on_progress(progress);
            }
        }

        done += segment.duration();
        offset_sec = segment_end_sec;
    }

    octx.write_trailer()?;
    on_progress(1.0);

    Ok(output_path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f64, end: f64) -> Segment {
        Segment { start, end }
    }

    #[test]
    fn insert_keeps_segments_sorted() {
        let mut list = vec![seg(10.0, 20.0)];
        insert_segment(&mut list, seg(1.0, 2.0));
        insert_segment(&mut list, seg(30.0, 40.0));
        assert_eq!(list, vec![seg(1.0, 2.0), seg(10.0, 20.0), seg(30.0, 40.0)]);
    }

    #[test]
    fn insert_fuses_overlapping_segments() {
        let mut list = vec![seg(1.0, 2.0), seg(10.0, 20.0), seg(30.0, 40.0)];
        insert_segment(&mut list, seg(15.0, 35.0));
        assert_eq!(list, vec![seg(1.0, 2.0), seg(10.0, 40.0)]);
    }

    #[test]
    fn numbered_path_keeps_directory_and_extension() {
        assert_eq!(
            numbered_path(Path::new("/tmp/clip_cut.mp4"), 2),
            PathBuf::from("/tmp/clip_cut_02.mp4")
        );
        assert_eq!(numbered_path(Path::new("clip"), 11), PathBuf::from("clip_11"));
    }
}
