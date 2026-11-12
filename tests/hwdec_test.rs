//! Hardware decoding must show the same picture as software decoding.
//!
//! Everything lives in one test: the backend override is a process-wide
//! environment variable. Backends this machine lacks fall back to software
//! and are reported as skipped, so the test passes without a GPU.

use media_cutnplay::player::{AudioBackend, Player, PlayerEvent, VideoFrame};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

fn make_media(dir: &Path, name: &str, codec_args: &[&str]) -> Option<PathBuf> {
    let path = dir.join(name);
    let ok = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-f", "lavfi"])
        .args(["-i", "testsrc2=duration=2:size=1280x720:rate=25"])
        .args(codec_args)
        .arg(&path)
        .status()
        .unwrap()
        .success();
    ok.then_some(path)
}

/// Loads `path` with the given backend override, seeks, and returns the
/// decoder label plus the frame at the seek target.
fn decode(path: &Path, backend: &str) -> (&'static str, Arc<VideoFrame>) {
    // SAFETY: this is the only test in the binary, and no player thread is
    // alive while the variable changes.
    unsafe { std::env::set_var("CUTNPLAY_HWDEC", backend) };

    let (tx, rx) = mpsc::channel();
    let player = Player::spawn(AudioBackend::Disabled, move |event| {
        let _ = tx.send(event);
    });
    player.load(path.to_path_buf());

    let mut label = None;
    let wait_frame = |label: &mut Option<&'static str>| loop {
        match rx.recv_timeout(Duration::from_secs(10)).expect("no frame") {
            PlayerEvent::FrameReady => return player.poll_frame().expect("frame"),
            PlayerEvent::Decoder(name) => *label = Some(name),
            PlayerEvent::Error(err) => panic!("player error: {err}"),
            PlayerEvent::Ended => {}
        }
    };

    wait_frame(&mut label);
    player.seek(1.0);
    let frame = wait_frame(&mut label);
    (label.expect("decoder was never reported"), frame)
}

fn mean_abs_diff(a: &VideoFrame, b: &VideoFrame) -> f64 {
    let total: u64 = a
        .rgba
        .iter()
        .zip(&b.rgba)
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    total as f64 / a.rgba.len() as f64
}

#[test]
fn hardware_decode_matches_software() {
    ffmpeg_next::init().unwrap();
    let dir = TempDir::new().unwrap();

    let files = [
        ("h264.mp4", &["-c:v", "libx264", "-pix_fmt", "yuv420p"][..]),
        ("hevc10.mp4", &["-c:v", "libx265", "-pix_fmt", "yuv420p10le"][..]),
    ];

    for (name, codec_args) in files {
        let Some(path) = make_media(dir.path(), name, codec_args) else {
            eprintln!("{name}: encoder unavailable, skipped");
            continue;
        };

        let (label, reference) = decode(&path, "off");
        assert_eq!(label, "Software");
        assert_eq!((reference.width, reference.height), (1280, 720));

        for backend in ["cuda", "vaapi", "auto"] {
            let (label, frame) = decode(&path, backend);
            if label == "Software" {
                eprintln!("{name}: {backend} unavailable, fell back to software");
            } else {
                eprintln!("{name}: {backend} decoded with {label}");
            }

            assert_eq!((frame.width, frame.height), (1280, 720));
            assert!((frame.pts - reference.pts).abs() < 0.001, "{name}/{backend}: wrong frame");
            let diff = mean_abs_diff(&frame, &reference);
            assert!(diff < 3.0, "{name}/{backend}: picture differs from software by {diff:.2}");
        }
    }
}
