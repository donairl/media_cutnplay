use std::process::Command;
use tempfile::NamedTempFile;

#[test]
fn test_decode_frame_to_rgba() {
    let tmp = NamedTempFile::new().unwrap();
    let video_path = tmp.path().with_extension("mp4");

    let status = Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "testsrc=duration=2:size=160x120:rate=25",
            "-c:v", "libx264",
            video_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    ffmpeg_next::init().unwrap();
    let mut decoder = media_cutnplay::core::decoder::VideoDecoder::open(&video_path)
        .expect("Failed to open decoder");

    let frame = decoder.seek_and_decode(0.5).expect("Failed to decode frame at 0.5s");
    assert_eq!(frame.width, 160);
    assert_eq!(frame.height, 120);
    assert_eq!(frame.rgba_data.len(), 160 * 120 * 4);
    assert!(frame.timestamp >= 0.0);

    let _ = std::fs::remove_file(video_path);
}
