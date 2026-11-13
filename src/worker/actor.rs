use crate::core::cutter::export_segments;
use crate::core::metadata::probe_file;
use crate::core::transcode::transcode;
use crate::worker::commands::{WorkerCommand, WorkerEvent};
use tokio::sync::mpsc::{Receiver, Sender};

/// Background jobs that are not playback: probing files, exporting cuts and
/// transcoding.
/// Playback lives in [`crate::player`] so an export never stalls it.
pub fn run_worker(mut cmd_rx: Receiver<WorkerCommand>, event_tx: Sender<WorkerEvent>) {
    while let Some(cmd) = cmd_rx.blocking_recv() {
        match cmd {
            WorkerCommand::LoadFile(path) => {
                let event = match probe_file(&path) {
                    Ok(meta) => WorkerEvent::MetadataLoaded(meta),
                    Err(err) => WorkerEvent::Error(err),
                };
                let _ = event_tx.blocking_send(event);
            }
            WorkerCommand::StartLosslessCut {
                input_path,
                output_path,
                segments,
                merge,
                mode,
            } => {
                let tx_prog = event_tx.clone();
                let res = export_segments(&input_path, &output_path, &segments, merge, mode, move |p| {
                    let _ = tx_prog.blocking_send(WorkerEvent::CutProgress(p));
                });
                let _ = event_tx.blocking_send(WorkerEvent::CutCompleted(res));
            }
            WorkerCommand::StartTranscode {
                input_path,
                output_path,
                settings,
                cancel,
            } => {
                // A transcode can run for hours; keep this loop free.
                let event_tx = event_tx.clone();
                std::thread::spawn(move || {
                    let tx_prog = event_tx.clone();
                    let res = transcode(&input_path, &output_path, &settings, &cancel, move |p| {
                        let _ = tx_prog.blocking_send(WorkerEvent::TranscodeProgress(p));
                    });
                    let _ = event_tx.blocking_send(WorkerEvent::TranscodeCompleted(res));
                });
            }
        }
    }
}
