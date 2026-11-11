use std::process::Command;
use tempfile::NamedTempFile;

#[test]
fn test_probe_metadata_valid_video() {
    let tmp = NamedTempFile::new().unwrap();
    let video_path = tmp.path().with_extension("mp4");

    // Generate a 1-second synthetic MP4 test clip using ffmpeg CLI
    let status = Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=30",
            "-f", "lavfi", "-i", "sine=duration=1:frequency=1000",
            "-c:v", "libx264", "-c:a", "aac",
            video_path.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to run ffmpeg generator");
    assert!(status.success());

    ffmpeg_next::init().unwrap();
    let meta = media_cutnplay::core::metadata::probe_file(&video_path)
        .expect("Failed to probe metadata");

    assert!(meta.duration_secs >= 0.9 && meta.duration_secs <= 1.2, "Duration was {}", meta.duration_secs);
    let video = meta.video.expect("Missing video stream");
    assert_eq!(video.width, 320);
    assert_eq!(video.height, 240);
    assert_eq!(video.fps.round() as u32, 30);
    assert!(meta.audio.is_some());

    let _ = std::fs::remove_file(video_path);
}
