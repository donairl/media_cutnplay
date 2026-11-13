use media_cutnplay::core::cutter::Segment;
use media_cutnplay::core::smart_cut::smart_cut;
use std::path::Path;
use std::process::Command;

/// An 8-second, 30 fps H.264 clip with B-frames and a keyframe every second.
fn make_clip(path: &Path) {
    let status = Command::new("ffmpeg")
        .args([
            "-y", "-v", "error",
            "-f", "lavfi", "-i", "testsrc=duration=8:size=320x240:rate=30",
            "-f", "lavfi", "-i", "sine=duration=8:frequency=440",
            "-g", "30", "-bf", "2", "-pix_fmt", "yuv420p",
            "-c:v", "libx264", "-c:a", "aac",
            path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
}

/// Decodes the whole file; any broken timestamp or corrupt frame fails.
fn assert_decodes_cleanly(path: &Path) {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i", path.to_str().unwrap(), "-f", "null", "-"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && stderr.trim().is_empty(), "decode errors: {stderr}");
}

fn video_frame_count(path: &Path) -> u32 {
    let out = Command::new("ffprobe")
        .args([
            "-v", "error", "-select_streams", "v:0", "-count_frames",
            "-show_entries", "stream=nb_read_frames", "-of", "csv=p=0",
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    // Transport streams list the stream once per program; one line is enough.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let first = stdout.lines().next().unwrap_or_default();
    first.trim().trim_end_matches(',').parse().unwrap()
}

/// Lowest per-frame PSNR of `cut` against the source frames `first..`.
/// A wrong or damaged frame drags it far below encoder noise.
fn min_psnr_against_source(source: &Path, cut: &Path, first: u32, count: u32) -> f64 {
    let filter = format!(
        "[0:v]trim=start_frame={first}:end_frame={},setpts=PTS-STARTPTS[a];\
         [1:v]setpts=PTS-STARTPTS[b];[a][b]psnr=stats_file=-",
        first + count
    );
    let out = Command::new("ffmpeg")
        .args([
            "-v", "error", "-i", source.to_str().unwrap(), "-i", cut.to_str().unwrap(),
            "-lavfi", &filter, "-f", "null", "-",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let stats = String::from_utf8_lossy(&out.stdout);
    let values: Vec<f64> = stats
        .lines()
        .filter_map(|line| line.split_whitespace().find_map(|f| f.strip_prefix("psnr_avg:")))
        .map(|v| if v == "inf" { f64::INFINITY } else { v.parse().unwrap() })
        .collect();
    assert_eq!(values.len(), count as usize, "compared frame count");
    values.into_iter().fold(f64::INFINITY, f64::min)
}

#[test]
fn test_smart_cut_is_frame_accurate() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mp4");
    let out = dir.path().join("clip_cut.mp4");
    make_clip(&src);
    ffmpeg_next::init().unwrap();

    // Frames 45..126: both ends fall mid-GOP, with whole GOPs between.
    let segment = Segment { start: 1.5, end: 4.2 };
    let mut last = 0.0;
    smart_cut(&src, &out, &[segment], |p| last = p).unwrap();
    assert_eq!(last, 1.0);

    assert_decodes_cleanly(&out);
    assert_eq!(video_frame_count(&out), 81);
    let psnr = min_psnr_against_source(&src, &out, 45, 81);
    assert!(psnr > 35.0, "worst frame PSNR {psnr}");

    let meta = media_cutnplay::core::metadata::probe_file(&out).unwrap();
    assert!(meta.audio.is_some());
    assert!((meta.duration_secs - 2.7).abs() < 0.1, "duration {}", meta.duration_secs);
}

#[test]
fn test_smart_cut_inside_one_gop() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mp4");
    let out = dir.path().join("clip_cut.mp4");
    make_clip(&src);
    ffmpeg_next::init().unwrap();

    // Frames 67..82: no keyframe inside, so everything is re-encoded.
    smart_cut(&src, &out, &[Segment { start: 2.25, end: 2.75 }], |_| {}).unwrap();

    assert_decodes_cleanly(&out);
    assert_eq!(video_frame_count(&out), 15);
    let psnr = min_psnr_against_source(&src, &out, 67, 15);
    assert!(psnr > 35.0, "worst frame PSNR {psnr}");
}

#[test]
fn test_smart_cut_on_keyframes_copies_everything() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mp4");
    let out = dir.path().join("clip_cut.mp4");
    make_clip(&src);
    ffmpeg_next::init().unwrap();

    smart_cut(&src, &out, &[Segment { start: 2.0, end: 5.0 }], |_| {}).unwrap();

    assert_decodes_cleanly(&out);
    assert_eq!(video_frame_count(&out), 90);
    // Stream copy: the frames are the source's own, bit for bit.
    assert_eq!(min_psnr_against_source(&src, &out, 60, 90), f64::INFINITY);
}

#[test]
fn test_smart_cut_merges_segments() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mp4");
    let out = dir.path().join("clip_cut.mp4");
    make_clip(&src);
    ffmpeg_next::init().unwrap();

    let segments = [
        Segment { start: 0.5, end: 2.5 },
        Segment { start: 5.1, end: 7.9 },
    ];
    smart_cut(&src, &out, &segments, |_| {}).unwrap();

    assert_decodes_cleanly(&out);
    assert_eq!(video_frame_count(&out), 60 + 84);
    let meta = media_cutnplay::core::metadata::probe_file(&out).unwrap();
    assert!((meta.duration_secs - 4.8).abs() < 0.1, "duration {}", meta.duration_secs);
}

#[test]
fn test_smart_cut_matroska() {
    let dir = tempfile::tempdir().unwrap();
    let mp4 = dir.path().join("clip.mp4");
    let src = dir.path().join("clip.mkv");
    let out = dir.path().join("clip_cut.mkv");
    make_clip(&mp4);
    let remux = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-i", mp4.to_str().unwrap(), "-c", "copy", src.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(remux.success());
    ffmpeg_next::init().unwrap();

    smart_cut(&src, &out, &[Segment { start: 1.5, end: 4.2 }], |_| {}).unwrap();

    assert_decodes_cleanly(&out);
    assert_eq!(video_frame_count(&out), 81);
    let psnr = min_psnr_against_source(&src, &out, 45, 81);
    assert!(psnr > 35.0, "worst frame PSNR {psnr}");
}

#[test]
fn test_smart_cut_rejects_other_codecs() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mkv");
    let out = dir.path().join("clip_cut.mkv");
    let status = Command::new("ffmpeg")
        .args([
            "-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=duration=2:size=160x120:rate=25",
            "-c:v", "mpeg4", src.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    ffmpeg_next::init().unwrap();

    let err = smart_cut(&src, &out, &[Segment { start: 0.5, end: 1.5 }], |_| {}).unwrap_err();
    assert!(err.to_string().contains("H.264"), "{err}");
    assert!(!out.exists(), "failed cut left a file behind");
}

#[test]
fn test_smart_cut_transport_stream() {
    let dir = tempfile::tempdir().unwrap();
    let mp4 = dir.path().join("clip.mp4");
    let src = dir.path().join("clip.ts");
    let out = dir.path().join("clip_cut.ts");
    make_clip(&mp4);
    let remux = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-i", mp4.to_str().unwrap(), "-c", "copy", src.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(remux.success());
    ffmpeg_next::init().unwrap();

    // Transport streams seek only roughly; no leading frame may go missing.
    smart_cut(&src, &out, &[Segment { start: 2.0, end: 4.7 }], |_| {}).unwrap();

    assert_decodes_cleanly(&out);
    assert_eq!(video_frame_count(&out), 81);
}
