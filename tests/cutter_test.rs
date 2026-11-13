use std::process::Command;
use tempfile::NamedTempFile;

#[test]
fn test_lossless_cut_remuxing() {
    let tmp_in = NamedTempFile::new().unwrap();
    let tmp_out = NamedTempFile::new().unwrap();
    let in_path = tmp_in.path().with_extension("mp4");
    let out_path = tmp_out.path().with_extension("mp4");

    // Generate a 4-second video clip with keyframes every 1 second (30 frames)
    let status = Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "testsrc=duration=4:size=320x240:rate=30",
            "-f", "lavfi", "-i", "sine=duration=4:frequency=440",
            "-g", "30",
            "-c:v", "libx264", "-c:a", "aac",
            in_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    ffmpeg_next::init().unwrap();
    let mut progress_values = Vec::new();
    let result = media_cutnplay::core::cutter::lossless_cut(
        &in_path,
        &out_path,
        1.0,
        3.0,
        |p| progress_values.push(p),
    );

    assert!(result.is_ok(), "Lossless cut failed: {:?}", result.err());
    assert!(out_path.exists());
    assert!(!progress_values.is_empty());

    // Verify duration of cut output is approximately 2 seconds (1.0 to 3.0)
    let meta = media_cutnplay::core::metadata::probe_file(&out_path).unwrap();
    assert!(meta.duration_secs >= 1.5 && meta.duration_secs <= 2.5, "Duration was {}", meta.duration_secs);

    let _ = std::fs::remove_file(in_path);
    let _ = std::fs::remove_file(out_path);
}

/// An 8-second clip with a keyframe every second.
fn make_clip(path: &std::path::Path) {
    let status = Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "testsrc=duration=8:size=320x240:rate=30",
            "-f", "lavfi", "-i", "sine=duration=8:frequency=440",
            "-g", "30",
            "-c:v", "libx264", "-c:a", "aac",
            path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
}

/// Decodes the whole file; fails on broken timestamps or corrupt packets.
fn assert_decodes_cleanly(path: &std::path::Path) {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i", path.to_str().unwrap(), "-f", "null", "-"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && stderr.trim().is_empty(), "decode errors: {stderr}");
}

#[test]
fn test_segments_merge_into_one_file() {
    use media_cutnplay::core::cutter::{export_segments, CutMode, Segment};

    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("clip.mp4");
    let out_path = dir.path().join("clip_cut.mp4");
    make_clip(&in_path);
    ffmpeg_next::init().unwrap();

    let segments = [
        Segment { start: 1.0, end: 3.0 },
        Segment { start: 5.0, end: 6.0 },
    ];
    let mut last_progress = 0.0;
    let mut monotonic = true;
    let outputs = export_segments(&in_path, &out_path, &segments, true, CutMode::Lossless, |p| {
        monotonic &= p >= last_progress;
        last_progress = p;
    })
    .unwrap();

    assert_eq!(outputs, vec![out_path.clone()]);
    assert!(monotonic, "progress went backwards");
    assert_eq!(last_progress, 1.0);

    let meta = media_cutnplay::core::metadata::probe_file(&out_path).unwrap();
    assert!(meta.video.is_some() && meta.audio.is_some());
    assert!(
        meta.duration_secs >= 2.7 && meta.duration_secs <= 3.4,
        "Merged duration expected ~3.0s, got {}",
        meta.duration_secs
    );
    assert_decodes_cleanly(&out_path);
}

#[test]
fn test_segments_export_to_separate_files() {
    use media_cutnplay::core::cutter::{export_segments, CutMode, Segment};

    let dir = tempfile::tempdir().unwrap();
    let in_path = dir.path().join("clip.mp4");
    let out_path = dir.path().join("clip_cut.mp4");
    make_clip(&in_path);
    ffmpeg_next::init().unwrap();

    let segments = [
        Segment { start: 1.0, end: 3.0 },
        Segment { start: 5.0, end: 6.0 },
    ];
    let outputs = export_segments(&in_path, &out_path, &segments, false, CutMode::Lossless, |_| {}).unwrap();

    assert_eq!(
        outputs,
        vec![dir.path().join("clip_cut_01.mp4"), dir.path().join("clip_cut_02.mp4")]
    );
    assert!(!out_path.exists());

    for (path, expected) in outputs.iter().zip([2.0, 1.0]) {
        let meta = media_cutnplay::core::metadata::probe_file(path).unwrap();
        assert!(
            (meta.duration_secs - expected).abs() <= 0.4,
            "{} expected ~{expected}s, got {}",
            path.display(),
            meta.duration_secs
        );
        assert_decodes_cleanly(path);
    }
}
