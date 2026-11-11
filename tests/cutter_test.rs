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
