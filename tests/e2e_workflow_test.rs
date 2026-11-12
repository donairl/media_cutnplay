use std::process::Command;
use tempfile::NamedTempFile;

#[test]
fn test_e2e_cut_and_verify_metadata() {
    let tmp_src = NamedTempFile::new().unwrap();
    let tmp_dest = NamedTempFile::new().unwrap();
    let src = tmp_src.path().with_extension("mp4");
    let dest = tmp_dest.path().with_extension("mp4");

    // 1. Generate 4-second test clip with 1-second keyframe intervals
    let st = Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "testsrc=duration=4:size=320x240:rate=25",
            "-f", "lavfi", "-i", "sine=duration=4:frequency=880",
            "-g", "25",
            "-c:v", "libx264", "-c:a", "aac",
            src.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(st.success());

    ffmpeg_next::init().unwrap();

    // 2. Probe metadata
    let meta = media_cutnplay::core::metadata::probe_file(&src).unwrap();
    assert_eq!(meta.video.as_ref().unwrap().width, 320);
    assert_eq!(meta.video.as_ref().unwrap().height, 240);
    assert!(meta.duration_secs >= 3.8 && meta.duration_secs <= 4.2);

    // 3. Decode frame at 1.0s
    let (frame_tx, frame_rx) = std::sync::mpsc::channel();
    let player = media_cutnplay::player::Player::spawn(
        media_cutnplay::player::AudioBackend::Disabled,
        move |event| {
            let _ = frame_tx.send(event);
        },
    );
    player.load(src.clone());
    player.seek(1.0);
    let frame = loop {
        let event = frame_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("player produced no frame");
        if let media_cutnplay::player::PlayerEvent::FrameReady = event {
            let frame = player.poll_frame().unwrap();
            if (frame.pts - 1.0).abs() < 0.001 {
                break frame;
            }
        }
    };
    assert_eq!(frame.width, 320);
    assert_eq!(frame.height, 240);
    assert_eq!(frame.rgba.len(), 320 * 240 * 4);

    // 4. Perform lossless cut (1.0s to 3.0s)
    let mut reported_progress = false;
    let cut_res = media_cutnplay::core::cutter::lossless_cut(
        &src,
        &dest,
        1.0,
        3.0,
        |p| {
            if p > 0.0 {
                reported_progress = true;
            }
        },
    );
    assert!(cut_res.is_ok(), "Cut failed: {:?}", cut_res.err());
    assert!(reported_progress, "No progress was reported during cut");

    // 5. Verify output file validity, duration and audio presence
    let cut_meta = media_cutnplay::core::metadata::probe_file(&dest).unwrap();
    assert!(cut_meta.video.is_some(), "Cut file missing video stream");
    assert!(cut_meta.audio.is_some(), "Cut file missing audio stream");
    assert!(
        cut_meta.duration_secs >= 1.8 && cut_meta.duration_secs <= 2.2,
        "Cut duration expected ~2.0s, got {}",
        cut_meta.duration_secs
    );

    let _ = std::fs::remove_file(src);
    let _ = std::fs::remove_file(dest);
}
