use media_cutnplay::core::error::MediaError;
use media_cutnplay::core::metadata::probe_file;
use media_cutnplay::core::transcode::{
    transcode, AudioMode, NvencSupport, Container, EncoderBackend, RateMode, Resolution, TranscodeSettings,
    VideoCodec,
};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;

/// A 3-second 1280x720 MPEG-4 clip with MP3 audio: nothing in it matches
/// what the transcoder produces.
fn make_clip(path: &Path) {
    let status = Command::new("ffmpeg")
        .args([
            "-y", "-v", "error",
            "-f", "lavfi", "-i", "testsrc=duration=3:size=1280x720:rate=30",
            "-f", "lavfi", "-i", "sine=duration=3:frequency=440",
            "-c:v", "mpeg4", "-q:v", "5", "-c:a", "libmp3lame",
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
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout.lines().next().unwrap_or_default().trim().trim_end_matches(',').parse().unwrap()
}

#[test]
fn test_transcode_to_h264_aac_mp4_scaled() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mkv");
    let out = dir.path().join("clip_transcoded.mp4");
    make_clip(&src);
    ffmpeg_next::init().unwrap();

    let settings = TranscodeSettings {
        resolution: Resolution::P480,
        ..TranscodeSettings::default()
    };
    let mut last = 0.0;
    let mut monotonic = true;
    transcode(&src, &out, &settings, &AtomicBool::new(false), |p| {
        monotonic &= p >= last;
        last = p;
    })
    .unwrap();
    assert!(monotonic, "progress went backwards");
    assert_eq!(last, 1.0);

    assert_decodes_cleanly(&out);
    assert_eq!(video_frame_count(&out), 90);
    let meta = probe_file(&out).unwrap();
    let video = meta.video.unwrap();
    assert_eq!(video.codec, "h264");
    assert_eq!((video.width, video.height), (854, 480));
    assert_eq!(meta.audio.unwrap().codec, "aac");
    assert!((meta.duration_secs - 3.0).abs() < 0.15, "duration {}", meta.duration_secs);
}

#[test]
fn test_transcode_to_hevc_mkv_with_audio_copy() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mkv");
    let out = dir.path().join("clip_transcoded.mkv");
    make_clip(&src);
    ffmpeg_next::init().unwrap();

    let settings = TranscodeSettings {
        codec: VideoCodec::Hevc,
        backend: EncoderBackend::Software,
        crf: 32,
        resolution: Resolution::Source,
        audio: AudioMode::Copy,
        container: Container::Mkv,
        ..TranscodeSettings::default()
    };
    transcode(&src, &out, &settings, &AtomicBool::new(false), |_| {}).unwrap();

    assert_decodes_cleanly(&out);
    assert_eq!(video_frame_count(&out), 90);
    let meta = probe_file(&out).unwrap();
    let video = meta.video.unwrap();
    assert_eq!(video.codec, "hevc");
    assert_eq!((video.width, video.height), (1280, 720));
    // Probing names the decoder, which for MP3 is "mp3float".
    assert!(meta.audio.unwrap().codec.starts_with("mp3"));
}

#[test]
fn test_transcode_cancel_leaves_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mkv");
    let out = dir.path().join("clip_transcoded.mp4");
    make_clip(&src);
    ffmpeg_next::init().unwrap();

    let cancel = AtomicBool::new(true);
    let result = transcode(&src, &out, &TranscodeSettings::default(), &cancel, |_| {});

    assert!(matches!(result, Err(MediaError::Cancelled)), "{result:?}");
    assert!(!out.exists());
}

/// Bitrate of the first video stream, in kbit/s.
fn video_kbps(path: &Path) -> f64 {
    let out = Command::new("ffprobe")
        .args([
            "-v", "error", "-select_streams", "v:0",
            "-show_entries", "stream=bit_rate", "-of", "csv=p=0",
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout.trim().trim_end_matches(',').parse::<f64>().unwrap() / 1000.0
}

/// 6 seconds of noise over a test card: hard enough that the encoder cannot
/// undershoot a bitrate target. `noise` is the grain strength, 0-100.
fn make_busy_clip(path: &Path, noise: u8) {
    let status = Command::new("ffmpeg")
        .args([
            "-y", "-v", "error",
            "-f", "lavfi", "-i", &format!("testsrc2=duration=6:size=1280x720:rate=30,noise=alls={noise}:allf=t"),
            "-c:v", "mpeg4", "-q:v", "2",
            path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn test_transcode_bitrate_mode_hits_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mkv");
    make_busy_clip(&src, 40);
    ffmpeg_next::init().unwrap();

    let mut measured = Vec::new();
    for target in [1_000, 4_000] {
        let out = dir.path().join(format!("clip_{target}.mp4"));
        let settings = TranscodeSettings {
            rate_mode: RateMode::Bitrate,
            bitrate_kbps: target,
            ..TranscodeSettings::default()
        };
        transcode(&src, &out, &settings, &AtomicBool::new(false), |_| {}).unwrap();
        assert_decodes_cleanly(&out);

        let kbps = video_kbps(&out);
        let ratio = kbps / f64::from(target);
        assert!((0.7..=1.3).contains(&ratio), "target {target} kbps, got {kbps:.0}");
        measured.push(kbps);
    }
    assert!(measured[1] > measured[0] * 2.5, "{measured:?}");
}

#[test]
fn test_transcode_quality_level_drives_the_size() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mkv");
    make_busy_clip(&src, 40);
    ffmpeg_next::init().unwrap();

    let size_at = |crf: u8| {
        let out = dir.path().join(format!("clip_crf{crf}.mp4"));
        let settings = TranscodeSettings { crf, ..TranscodeSettings::default() };
        transcode(&src, &out, &settings, &AtomicBool::new(false), |_| {}).unwrap();
        std::fs::metadata(&out).unwrap().len()
    };
    let (fine, coarse) = (size_at(18), size_at(30));
    assert!(fine > coarse * 2, "crf 18: {fine} bytes, crf 30: {coarse} bytes");
}

/// Codec of the first video stream, by FFmpeg's name for it.
fn video_codec(path: &Path) -> String {
    let out = Command::new("ffprobe")
        .args([
            "-v", "error", "-select_streams", "v:0",
            "-show_entries", "stream=codec_name", "-of", "csv=p=0",
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().trim_end_matches(',').to_string()
}

#[test]
fn test_transcode_to_av1_and_vp9() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mkv");
    make_clip(&src);
    ffmpeg_next::init().unwrap();

    for (codec, name, container) in [
        (VideoCodec::Av1, "av1", Container::Mp4),
        (VideoCodec::Av1, "av1", Container::Mkv),
        (VideoCodec::Vp9, "vp9", Container::Mp4),
        (VideoCodec::Vp9, "vp9", Container::Mkv),
    ] {
        let settings = TranscodeSettings {
            resolution: Resolution::P480,
            container,
            ..TranscodeSettings::default().with_codec(codec)
        };
        let out = dir.path().join(format!("clip_{name}.{}", settings.extension()));
        transcode(&src, &out, &settings, &AtomicBool::new(false), |_| {}).unwrap();

        assert_decodes_cleanly(&out);
        assert_eq!(video_codec(&out), name);
        assert_eq!(video_frame_count(&out), 90, "{name} in {}", settings.extension());
        let meta = probe_file(&out).unwrap();
        assert!((meta.duration_secs - 3.0).abs() < 0.15, "duration {}", meta.duration_secs);
    }
}

#[test]
fn test_transcode_av1_and_vp9_bitrate_mode() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("clip.mkv");
    // Heavier grain than this and one-pass VP9 overshoots by design: the
    // ffmpeg CLI lands on the same bitrate for the same input.
    make_busy_clip(&src, 12);
    ffmpeg_next::init().unwrap();

    for codec in [VideoCodec::Av1, VideoCodec::Vp9] {
        let out = dir.path().join(format!("clip_{codec:?}.mp4"));
        let settings = TranscodeSettings {
            rate_mode: RateMode::Bitrate,
            bitrate_kbps: 2_000,
            resolution: Resolution::P480,
            ..TranscodeSettings::default().with_codec(codec)
        };
        transcode(&src, &out, &settings, &AtomicBool::new(false), |_| {}).unwrap();
        assert_decodes_cleanly(&out);

        let kbps = video_kbps(&out);
        assert!((1_000.0..=3_000.0).contains(&kbps), "{codec:?}: target 2000 kbps, got {kbps:.0}");
    }
}

/// The probe must agree with what actually encoding on the GPU does.
#[test]
fn nvenc_probe_matches_real_encodes() {
    ffmpeg_next::init().unwrap();
    let support = NvencSupport::probe();
    println!("nvenc support: {support:?}");
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.mp4");
    make_clip(&input);

    for codec in [VideoCodec::H264, VideoCodec::Hevc, VideoCodec::Av1] {
        let output = dir.path().join(format!("{codec:?}.mkv"));
        let settings = TranscodeSettings {
            backend: EncoderBackend::Nvenc,
            container: Container::Mkv,
            ..TranscodeSettings::default().with_codec(codec)
        };
        let result = transcode(&input, &output, &settings, &AtomicBool::new(false), |_| {});
        assert_eq!(result.is_ok(), support.supports(codec), "{codec:?}: {result:?}");
    }
}
