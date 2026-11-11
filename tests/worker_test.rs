use std::process::Command;
use tempfile::NamedTempFile;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_worker_actor_lifecycle() {
    let tmp = NamedTempFile::new().unwrap();
    let video_path = tmp.path().with_extension("mp4");

    let status = Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "testsrc=duration=1:size=160x120:rate=25",
            "-c:v", "libx264", video_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    ffmpeg_next::init().unwrap();

    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let (event_tx, mut event_rx) = mpsc::channel(16);

    std::thread::spawn(move || {
        media_cutnplay::worker::actor::run_worker(cmd_rx, event_tx);
    });

    cmd_tx
        .send(media_cutnplay::worker::commands::WorkerCommand::LoadFile(video_path.clone()))
        .await
        .unwrap();

    let mut loaded_meta = false;
    let mut decoded_frame = false;

    // Collect initial events from loading file
    while let Some(event) = event_rx.recv().await {
        match event {
            media_cutnplay::worker::commands::WorkerEvent::MetadataLoaded(meta) => {
                assert_eq!(meta.video.unwrap().width, 160);
                loaded_meta = true;
            }
            media_cutnplay::worker::commands::WorkerEvent::FrameDecoded(frame) => {
                assert_eq!(frame.width, 160);
                decoded_frame = true;
                break;
            }
            _ => {}
        }
    }

    assert!(loaded_meta, "Metadata was not loaded");
    assert!(decoded_frame, "Initial frame was not decoded");

    let _ = std::fs::remove_file(video_path);
}
