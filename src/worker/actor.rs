use crate::core::cutter::lossless_cut;
use crate::core::decoder::VideoDecoder;
use crate::core::metadata::probe_file;
use crate::worker::commands::{WorkerCommand, WorkerEvent};
use tokio::sync::mpsc::{Receiver, Sender};

pub fn run_worker(mut cmd_rx: Receiver<WorkerCommand>, event_tx: Sender<WorkerEvent>) {
    let mut current_decoder: Option<VideoDecoder> = None;

    while let Some(cmd) = cmd_rx.blocking_recv() {
        match cmd {
            WorkerCommand::LoadFile(path) => {
                match probe_file(&path) {
                    Ok(meta) => {
                        let _ = event_tx.blocking_send(WorkerEvent::MetadataLoaded(meta));
                        match VideoDecoder::open(&path) {
                            Ok(mut dec) => {
                                if let Ok(frame) = dec.seek_and_decode(0.0) {
                                    let _ = event_tx.blocking_send(WorkerEvent::FrameDecoded(frame));
                                }
                                current_decoder = Some(dec);
                            }
                            Err(err) => {
                                let _ = event_tx.blocking_send(WorkerEvent::Error(err));
                            }
                        }
                    }
                    Err(err) => {
                        let _ = event_tx.blocking_send(WorkerEvent::Error(err));
                    }
                }
            }
            WorkerCommand::Seek(target_sec) => {
                if let Some(dec) = &mut current_decoder {
                    match dec.seek_and_decode(target_sec) {
                        Ok(frame) => {
                            let _ = event_tx.blocking_send(WorkerEvent::FrameDecoded(frame));
                        }
                        Err(err) => {
                            let _ = event_tx.blocking_send(WorkerEvent::Error(err));
                        }
                    }
                }
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
