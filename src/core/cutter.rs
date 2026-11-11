use crate::core::error::MediaError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn lossless_cut<F>(
    input_path: &Path,
    output_path: &Path,
    in_point: f64,
    out_point: f64,
    mut on_progress: F,
) -> Result<PathBuf, MediaError>
where
    F: FnMut(f32),
{
    if in_point >= out_point {
        return Err(MediaError::CutFailed("In-point must be less than out-point".into()));
    }

    let mut ictx = ffmpeg_next::format::input(input_path)?;
    let mut octx = ffmpeg_next::format::output(output_path)?;

    // Map input stream indices to output stream indices & stream parameters
    let mut stream_mapping = HashMap::new();
    let mut in_time_bases = HashMap::new();
    let mut out_time_bases = HashMap::new();

    for (src_idx, istream) in ictx.streams().enumerate() {
        let medium = istream.parameters().medium();
        if medium == ffmpeg_next::media::Type::Video
            || medium == ffmpeg_next::media::Type::Audio
            || medium == ffmpeg_next::media::Type::Subtitle
        {
            let mut ostream = octx.add_stream(ffmpeg_next::encoder::find(istream.parameters().id()))?;
            ostream.set_parameters(istream.parameters());
            let out_idx = ostream.index();
            stream_mapping.insert(src_idx, out_idx);
            in_time_bases.insert(src_idx, istream.time_base());
            out_time_bases.insert(out_idx, ostream.time_base());
        }
    }

    octx.write_header()?;

    // Seek input to keyframe at or before in_point
    let target_av_pts = (in_point * f64::from(ffmpeg_next::ffi::AV_TIME_BASE)) as i64;
    let _ = ictx.seek(target_av_pts, ..target_av_pts);

    let duration = (out_point - in_point).max(0.001);
    let mut start_pts_map: HashMap<usize, i64> = HashMap::new();

    for (stream, mut packet) in ictx.packets() {
        let src_idx = stream.index();
        if let Some(&out_idx) = stream_mapping.get(&src_idx) {
            let in_tb = in_time_bases[&src_idx];
            let out_tb = octx.stream(out_idx).map(|s| s.time_base()).unwrap_or(in_tb);

            let pkt_pts = packet.pts().unwrap_or(0);
            let pkt_sec = pkt_pts as f64 * f64::from(in_tb);

            if stream.parameters().medium() == ffmpeg_next::media::Type::Video {
                if pkt_sec > out_point {
                    break;
                }
            }

            let start_pts = *start_pts_map.entry(src_idx).or_insert(pkt_pts);
            if let Some(pts) = packet.pts() {
                packet.set_pts(Some(pts.saturating_sub(start_pts)));
            }
            if let Some(dts) = packet.dts() {
                packet.set_dts(Some(dts.saturating_sub(start_pts)));
            }

            packet.set_stream(out_idx);
            packet.rescale_ts(in_tb, out_tb);
            packet.set_position(-1);

            let _ = packet.write_interleaved(&mut octx);

            let progress = ((pkt_sec - in_point) / duration).clamp(0.0, 1.0) as f32;
            on_progress(progress);
        }
    }

    octx.write_trailer()?;
    on_progress(1.0);

    Ok(output_path.to_path_buf())
}
