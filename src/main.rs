use iced::futures::SinkExt;
use iced::widget::image;
use iced::{window, Element, Subscription, Task};
use media_cutnplay::state::app_state::{AppState, ExportStatus};
use media_cutnplay::state::messages::{Message, WorkerEventWrapper};
use media_cutnplay::ui::layout;
use media_cutnplay::worker::actor::run_worker;
use media_cutnplay::worker::commands::{WorkerCommand, WorkerEvent};
use std::time::Duration;
use tokio::sync::mpsc;

fn main() -> iced::Result {
    ffmpeg_next::init().expect("Failed to initialize FFmpeg native libraries");

    iced::application(boot, update, view)
        .title("Media CutNPlay - Offline Bulk Media Transcoder & Smart Cutter")
        .subscription(subscription)
        .window(window::Settings {
            size: iced::Size::new(1150.0, 720.0),
            min_size: Some(iced::Size::new(800.0, 500.0)),
            ..Default::default()
        })
        .run()
}

fn boot() -> (AppState, Task<Message>) {
    let (cmd_tx, cmd_rx) = mpsc::channel(64);
    let (event_tx, mut event_rx) = mpsc::channel(64);

    std::thread::spawn(move || {
        run_worker(cmd_rx, event_tx);
    });

    let state = AppState::new(cmd_tx);

    let event_stream_task = Task::stream(iced::stream::channel(64, async move |mut output| {
        while let Some(event) = event_rx.recv().await {
            let wrapper = match event {
                WorkerEvent::MetadataLoaded(m) => WorkerEventWrapper::MetadataLoaded(m),
                WorkerEvent::FrameDecoded(f) => WorkerEventWrapper::FrameDecoded {
                    timestamp: f.timestamp,
                    width: f.width,
                    height: f.height,
                    rgba: f.rgba_data,
                },
                WorkerEvent::CutProgress(p) => WorkerEventWrapper::CutProgress(p),
                WorkerEvent::CutCompleted(res) => {
                    WorkerEventWrapper::CutCompleted(res.map_err(|e| e.to_string()))
                }
                WorkerEvent::Error(err) => WorkerEventWrapper::Error(err.to_string()),
            };
            let _ = output.send(Message::WorkerEventReceived(wrapper)).await;
        }
    }));

    (state, event_stream_task)
}

fn update(state: &mut AppState, message: Message) -> Task<Message> {
    match message {
        Message::OpenFilePicker => Task::perform(
            async {
                let files = rfd::AsyncFileDialog::new()
                    .add_filter(
                        "Media Files",
                        &[
                            "mp4", "mkv", "mov", "webm", "avi", "ts", "mp3", "wav", "flac", "aac",
                            "ogg", "m4a", "png", "jpg", "jpeg", "webp", "avif",
                        ],
                    )
                    .pick_files()
                    .await;
                files.map(|handles| {
                    handles
                        .into_iter()
                        .map(|h| h.path().to_path_buf())
                        .collect::<Vec<_>>()
                })
            },
            |opt_paths| Message::FilesAdded(opt_paths.unwrap_or_default()),
        ),

        Message::FilesAdded(paths) => {
            if paths.is_empty() {
                return Task::none();
            }

            let was_empty = state.queue.is_empty();
            let first_path = paths[0].clone();

            for path in paths {
                let _ = state.cmd_tx.try_send(WorkerCommand::LoadFile(path));
            }

            if was_empty {
                state.active_index = Some(0);
                let _ = state.cmd_tx.try_send(WorkerCommand::LoadFile(first_path));
            }

            Task::none()
        }

        Message::SelectQueueItem(idx) => {
            if idx < state.queue.len() {
                state.active_index = Some(idx);
                state.current_timestamp = 0.0;
                state.in_point = None;
                state.out_point = None;
                state.current_frame = None;
                state.export_status = ExportStatus::Idle;
                let path = state.queue[idx].file_path.clone();
                let _ = state.cmd_tx.try_send(WorkerCommand::LoadFile(path));
            }
            Task::none()
        }

        Message::RemoveQueueItem(idx) => {
            if idx < state.queue.len() {
                state.queue.remove(idx);
                if state.queue.is_empty() {
                    state.active_index = None;
                    state.current_frame = None;
                    state.current_timestamp = 0.0;
                } else if let Some(active) = state.active_index {
                    if active >= state.queue.len() {
                        state.active_index = Some(state.queue.len() - 1);
                        let path = state.queue[state.queue.len() - 1].file_path.clone();
                        let _ = state.cmd_tx.try_send(WorkerCommand::LoadFile(path));
                    }
                }
            }
            Task::none()
        }

        Message::ClearQueue => {
            state.queue.clear();
            state.active_index = None;
            state.current_frame = None;
            state.current_timestamp = 0.0;
            state.in_point = None;
            state.out_point = None;
            state.export_status = ExportStatus::Idle;
            Task::none()
        }

        Message::TogglePlay => {
            if state.active_item().is_some() {
                state.is_playing = !state.is_playing;
            }
            Task::none()
        }

        Message::StepFrame(steps) => {
            if state.active_item().is_some() {
                let fps = state
                    .active_item()
                    .and_then(|i| i.video.as_ref())
                    .map(|v| v.fps)
                    .unwrap_or(30.0);
                let delta = steps as f64 / fps;
                let new_sec = (state.current_timestamp + delta).clamp(0.0, state.active_duration());
                state.current_timestamp = new_sec;
                let _ = state.cmd_tx.try_send(WorkerCommand::Seek(new_sec));
            }
            Task::none()
        }

        Message::Seek(sec) => {
            state.current_timestamp = sec.clamp(0.0, state.active_duration());
            let _ = state.cmd_tx.try_send(WorkerCommand::Seek(state.current_timestamp));
            Task::none()
        }

        Message::ToggleFullscreen => {
            state.is_fullscreen = !state.is_fullscreen;
            let mode = if state.is_fullscreen {
                window::Mode::Fullscreen
            } else {
                window::Mode::Windowed
            };
            window::latest().and_then(move |id| {
                window::set_mode(id, mode)
            })
        }

        Message::SetInPoint => {
            state.in_point = Some(state.current_timestamp);
            Task::none()
        }

        Message::SetOutPoint => {
            state.out_point = Some(state.current_timestamp);
            Task::none()
        }

        Message::ClearPoints => {
            state.in_point = None;
            state.out_point = None;
            Task::none()
        }

        Message::SelectOutputPath => {
            let initial_name = state
                .active_item()
                .and_then(|item| item.file_path.file_stem().and_then(|s| s.to_str()))
                .map(|s| format!("{s}_cut.mp4"))
                .unwrap_or_else(|| "output_cut.mp4".into());

            Task::perform(
                async move {
                    let file = rfd::AsyncFileDialog::new()
                        .set_file_name(&initial_name)
                        .add_filter("MP4 Video", &["mp4"])
                        .add_filter("MKV Video", &["mkv"])
                        .save_file()
                        .await;
                    file.map(|h| h.path().to_path_buf())
                },
                Message::OutputPathSelected,
            )
        }

        Message::OutputPathSelected(opt_path) => {
            if let Some(path) = opt_path {
                state.target_output_path = Some(path);
            }
            Task::none()
        }

        Message::StartLosslessCut => {
            let active_info = state.active_item().map(|item| (item.file_path.clone(), state.in_point, state.out_point));
            if let Some((input_path, Some(in_pt), Some(out_pt))) = active_info {
                if out_pt > in_pt {
                    let default_out = input_path
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."))
                        .join(format!(
                            "{}_cut.{}",
                            input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("media"),
                            input_path.extension().and_then(|e| e.to_str()).unwrap_or("mp4")
                        ));

                    let output_path = state.target_output_path.clone().unwrap_or(default_out);

                    state.export_status = ExportStatus::Processing {
                        progress: 0.0,
                        output_path: output_path.clone(),
                    };

                    let _ = state.cmd_tx.try_send(WorkerCommand::StartLosslessCut {
                        input_path,
                        output_path,
                        in_point: in_pt,
                        out_point: out_pt,
                    });
                }
            }
            Task::none()
        }

        Message::WorkerEventReceived(event) => {
            match event {
                WorkerEventWrapper::MetadataLoaded(meta) => {
                    // Update or add item to queue
                    if let Some(existing) = state.queue.iter_mut().find(|i| i.file_path == meta.file_path) {
                        *existing = meta;
                    } else {
                        state.queue.push(meta);
                        if state.active_index.is_none() {
                            state.active_index = Some(0);
                        }
                    }
                }
                WorkerEventWrapper::FrameDecoded {
                    timestamp,
                    width,
                    height,
                    rgba,
                } => {
                    state.current_timestamp = timestamp;
                    let handle = image::Handle::from_rgba(width, height, rgba);
                    state.current_frame = Some(handle);
                }
                WorkerEventWrapper::CutProgress(prog) => {
                    if let ExportStatus::Processing { output_path, .. } = &state.export_status {
                        state.export_status = ExportStatus::Processing {
                            progress: prog,
                            output_path: output_path.clone(),
                        };
                    }
                }
                WorkerEventWrapper::CutCompleted(result) => match result {
                    Ok(path) => {
                        state.export_status = ExportStatus::Completed { output_path: path };
                    }
                    Err(err) => {
                        state.export_status = ExportStatus::Failed(err);
                    }
                },
                WorkerEventWrapper::Error(err) => {
                    eprintln!("Worker error: {err}");
                }
            }
            Task::none()
        }

        Message::TickTelemetry => {
            state.refresh_memory();
            Task::none()
        }

        Message::TickPlayback => {
            if state.is_playing {
                let dur = state.active_duration();
                let next_sec = state.current_timestamp + 0.04;
                if next_sec >= dur {
                    state.current_timestamp = dur;
                    state.is_playing = false;
                } else {
                    state.current_timestamp = next_sec;
                }
                let _ = state.cmd_tx.try_send(WorkerCommand::Seek(state.current_timestamp));
            }
            Task::none()
        }
    }
}

fn subscription(state: &AppState) -> Subscription<Message> {
    let telemetry_sub = iced::time::every(Duration::from_secs(1)).map(|_| Message::TickTelemetry);

    if state.is_playing {
        let playback_sub =
            iced::time::every(Duration::from_millis(40)).map(|_| Message::TickPlayback);
        Subscription::batch([telemetry_sub, playback_sub])
    } else {
        telemetry_sub
    }
}

fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    layout::view(state)
}
