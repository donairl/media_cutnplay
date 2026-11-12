use media_cutnplay::player::{AudioBackend, Player, PlayerEvent, VideoFrame};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

/// 25fps test pattern, optionally with a 440Hz tone.
fn make_media(duration_secs: u32, with_audio: bool) -> PathBuf {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().with_extension("mp4");
    let video = format!("testsrc=duration={duration_secs}:size=160x120:rate=25");
    let audio = format!("sine=frequency=440:duration={duration_secs}:sample_rate=44100");

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-loglevel", "error", "-f", "lavfi", "-i", &video]);
    if with_audio {
        cmd.args(["-f", "lavfi", "-i", &audio, "-c:a", "aac"]);
    }
    cmd.args(["-c:v", "libx264", "-pix_fmt", "yuv420p", path.to_str().unwrap()]);
    assert!(cmd.status().unwrap().success());

    ffmpeg_next::init().unwrap();
    path
}

fn spawn(backend: AudioBackend) -> (Player, Receiver<PlayerEvent>) {
    let (tx, rx) = mpsc::channel();
    let player = Player::spawn(backend, move |event| {
        let _ = tx.send(event);
    });
    (player, rx)
}

fn wait_frame_ready(rx: &Receiver<PlayerEvent>) {
    loop {
        match rx.recv_timeout(Duration::from_secs(5)).expect("no FrameReady") {
            PlayerEvent::FrameReady => return,
            PlayerEvent::Error(err) => panic!("player error: {err}"),
            PlayerEvent::Ended | PlayerEvent::Decoder(_) => {}
        }
    }
}

fn wait_frame(player: &Player, rx: &Receiver<PlayerEvent>) -> Arc<VideoFrame> {
    wait_frame_ready(rx);
    player.poll_frame().expect("FrameReady without a frame")
}

/// Polls like the UI does on every display refresh until playback ends.
fn play_to_end(player: &Player, rx: &Receiver<PlayerEvent>) -> (Vec<(f64, Duration)>, Duration) {
    let started = Instant::now();
    player.play();
    let mut shown = Vec::new();
    loop {
        if let Some(frame) = player.poll_frame() {
            shown.push((frame.pts, started.elapsed()));
        }
        match rx.try_recv() {
            Ok(PlayerEvent::Ended) => break,
            Ok(PlayerEvent::Error(err)) => panic!("player error: {err}"),
            _ => {}
        }
        assert!(started.elapsed() < Duration::from_secs(8), "playback never ended");
        std::thread::sleep(Duration::from_millis(4));
    }
    (shown, started.elapsed())
}

#[test]
fn load_shows_first_frame_paused() {
    let path = make_media(1, false);
    let (player, rx) = spawn(AudioBackend::Disabled);

    player.load(path.clone());
    let frame = wait_frame(&player, &rx);

    assert_eq!((frame.width, frame.height), (160, 120));
    assert_eq!(frame.rgba.len(), 160 * 120 * 4);
    assert!(frame.pts.abs() < 0.001);

    std::thread::sleep(Duration::from_millis(200));
    assert!(!player.is_playing());
    assert_eq!(player.position(), 0.0, "clock must not run while paused");
    assert!(player.poll_frame().is_none(), "no frames may be shown while paused");

    let _ = std::fs::remove_file(path);
}

#[test]
fn plays_every_frame_in_real_time() {
    let path = make_media(2, false);
    let (player, rx) = spawn(AudioBackend::Disabled);
    player.load(path.clone());
    wait_frame(&player, &rx);

    let (shown, elapsed) = play_to_end(&player, &rx);

    assert_eq!(shown.len(), 49, "frames after the first: {shown:?}");
    for pair in shown.windows(2) {
        let step = pair[1].0 - pair[0].0;
        assert!((step - 0.04).abs() < 0.001, "dropped/duplicated frame: step {step}");
    }
    // Each frame must appear when the clock reaches its PTS, not in bursts.
    for (pts, at) in &shown {
        let late = at.as_secs_f64() - pts;
        assert!((-0.01..0.06).contains(&late), "frame {pts} shown at {at:?}");
    }
    assert!(elapsed >= Duration::from_millis(1900), "too fast: {elapsed:?}");
    assert!(elapsed <= Duration::from_millis(2500), "too slow: {elapsed:?}");
    assert!(player.has_ended());

    let _ = std::fs::remove_file(path);
}

#[test]
fn seek_is_frame_accurate_and_coalesced() {
    let path = make_media(2, false);
    let (player, rx) = spawn(AudioBackend::Disabled);
    player.load(path.clone());
    wait_frame(&player, &rx);

    player.seek(1.0);
    let frame = wait_frame(&player, &rx);
    assert!((frame.pts - 1.0).abs() < 0.001, "got {}", frame.pts);
    assert!((player.position() - 1.0).abs() < 0.001);

    // A scrub burst: the last target must win.
    for i in 0..20 {
        player.seek(0.1 + f64::from(i) * 0.04);
    }
    let mut last = None;
    let deadline = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < deadline {
        if let Some(frame) = player.poll_frame() {
            last = Some(frame.pts);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    // Final target is 0.86; frames sit on multiples of 0.04, so 0.88 is next.
    assert!((last.unwrap() - 0.88).abs() < 0.001, "got {last:?}");

    // Past the end: land on the final frame.
    while rx.try_recv().is_ok() {}
    player.seek(5.0);
    let frame = wait_frame(&player, &rx);
    assert!((frame.pts - 1.96).abs() < 0.001, "got {}", frame.pts);

    let _ = std::fs::remove_file(path);
}

#[test]
fn pause_freezes_and_resume_continues() {
    let path = make_media(2, false);
    let (player, rx) = spawn(AudioBackend::Disabled);
    player.load(path.clone());
    wait_frame(&player, &rx);

    player.play();
    let until = Instant::now() + Duration::from_millis(400);
    while Instant::now() < until {
        player.poll_frame();
        std::thread::sleep(Duration::from_millis(4));
    }
    player.pause();
    std::thread::sleep(Duration::from_millis(50));
    let paused_at = player.position();
    assert!((0.3..0.6).contains(&paused_at), "paused at {paused_at}");
    player.poll_frame();

    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(player.position(), paused_at);
    assert!(player.poll_frame().is_none(), "frames advanced while paused");

    let (shown, _) = play_to_end(&player, &rx);
    assert!(shown.first().unwrap().0 > paused_at - 0.05);
    assert!((shown.last().unwrap().0 - 1.96).abs() < 0.001);

    let _ = std::fs::remove_file(path);
}

#[test]
fn audio_plays_in_sync_with_video() {
    let path = make_media(2, true);
    let (player, rx) = spawn(AudioBackend::Null {
        rate: 48_000,
        channels: 2,
    });
    player.load(path.clone());
    wait_frame(&player, &rx);
    assert_eq!(player.audio_samples_played(), 0, "audio leaked while paused");

    let (shown, elapsed) = play_to_end(&player, &rx);

    // 2s of 44.1k mono resampled to 48k stereo.
    let played = player.audio_samples_played() as f64 / (48_000.0 * 2.0);
    assert!((played - 2.0).abs() < 0.1, "audio played: {played}s");
    assert!(shown.len() >= 47, "video frames shown: {}", shown.len());
    // Video follows the audio clock.
    for (pts, at) in &shown {
        let late = at.as_secs_f64() - pts;
        assert!((-0.02..0.08).contains(&late), "frame {pts} shown at {at:?}");
    }
    assert!(elapsed >= Duration::from_millis(1900) && elapsed <= Duration::from_millis(2600));

    let _ = std::fs::remove_file(path);
}

#[test]
fn seek_during_playback_keeps_audio_and_video_together() {
    let path = make_media(3, true);
    let (player, rx) = spawn(AudioBackend::Null {
        rate: 48_000,
        channels: 2,
    });
    player.load(path.clone());
    wait_frame(&player, &rx);

    player.play();
    std::thread::sleep(Duration::from_millis(300));
    player.seek(2.0);
    wait_frame_ready(&rx);
    let before = player.audio_samples_played();

    let (shown, elapsed) = play_to_end(&player, &rx);
    assert!(shown.iter().all(|(pts, _)| *pts >= 1.999), "{shown:?}");
    assert!((shown.last().unwrap().0 - 2.96).abs() < 0.001);

    // Only the remaining ~1s of audio may play after the seek.
    let played = (player.audio_samples_played() - before) as f64 / (48_000.0 * 2.0);
    assert!((played - 1.0).abs() < 0.15, "audio after seek: {played}s");
    assert!(elapsed <= Duration::from_millis(1600), "{elapsed:?}");

    let _ = std::fs::remove_file(path);
}
