use crate::core::cutter::lossless_cut;
use crate::core::metadata::probe_file;
use crate::worker::commands::{WorkerCommand, WorkerEvent};
use tokio::sync::mpsc::{Receiver, Sender};

/// Background jobs that are not playback: probing files and exporting cuts.
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
                in_point,
                out_point,
            } => {
                let tx_prog = event_tx.clone();
                let res = lossless_cut(&input_path, &output_path, in_point, out_point, move |p| {
                    let _ = tx_prog.blocking_send(WorkerEvent::CutProgress(p));
                });
                let _ = event_tx.blocking_send(WorkerEvent::CutCompleted(res));
            }
        }
    }
}
