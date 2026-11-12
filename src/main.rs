use iced::futures::SinkExt;
use iced::{event, keyboard, mouse, window, Element, Event, Subscription, Task};
use media_cutnplay::state::app_state::{AppState, ExportStatus};
use media_cutnplay::player::{AudioBackend, Player, PlayerEvent};
use media_cutnplay::state::messages::{Message, WorkerEventWrapper};
use media_cutnplay::ui::{icons, layout, player_view, theme};
use media_cutnplay::worker::actor::run_worker;
use media_cutnplay::worker::commands::{WorkerCommand, WorkerEvent};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

fn main() -> iced::Result {
    ffmpeg_next::init().expect("Failed to initialize FFmpeg native libraries");

    iced::application(boot, update, view)
        .title("Media CutNPlay - Offline Bulk Media Transcoder & Smart Cutter")
        .subscription(subscription)
        .font(icons::FONT_BYTES)
        .theme(|_state: &AppState| theme::app_theme())
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

    let (player_tx, mut player_rx) = mpsc::unbounded_channel();
    let player = Player::spawn(AudioBackend::Device, move |event| {
        let _ = player_tx.send(event);
    });

    let state = AppState::new(cmd_tx, player);

    let worker_events = Task::stream(iced::stream::channel(64, async move |mut output| {
        while let Some(event) = event_rx.recv().await {
            let wrapper = match event {
                WorkerEvent::MetadataLoaded(m) => WorkerEventWrapper::MetadataLoaded(m),
                WorkerEvent::CutProgress(p) => WorkerEventWrapper::CutProgress(p),
                WorkerEvent::CutCompleted(res) => {
                    WorkerEventWrapper::CutCompleted(res.map_err(|e| e.to_string()))
                }
                WorkerEvent::Error(err) => WorkerEventWrapper::Error(err.to_string()),
            };
            let _ = output.send(Message::WorkerEventReceived(wrapper)).await;
        }
    }));

    let player_events = Task::stream(iced::stream::channel(64, async move |mut output| {
        while let Some(event) = player_rx.recv().await {
            let _ = output.send(Message::PlayerEvent(event)).await;
        }
    }));

    // Files passed on the command line are queued at startup.
    let cli_files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let open_cli_files = if cli_files.is_empty() {
        Task::none()
    } else {
        Task::done(Message::FilesAdded(cli_files))
    };

    (
        state,
        Task::batch([worker_events, player_events, open_cli_files]),
    )
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
                open_in_player(state, first_path);
            }

            Task::none()
        }

        Message::SelectQueueItem(idx) => {
            if idx < state.queue.len() {
                state.active_index = Some(idx);
                state.export_status = ExportStatus::Idle;
                let path = state.queue[idx].file_path.clone();
                open_in_player(state, path);
            }
            Task::none()
        }

        Message::RemoveQueueItem(idx) => {
            if idx < state.queue.len() {
                state.queue.remove(idx);
                match state.active_index {
                    _ if state.queue.is_empty() => close_player(state),
                    Some(active) if idx < active => state.active_index = Some(active - 1),
                    Some(active) if idx == active => {
                        let new_active = active.min(state.queue.len() - 1);
                        state.active_index = Some(new_active);
                        let path = state.queue[new_active].file_path.clone();
                        open_in_player(state, path);
                    }
                    _ => {}
                }
            }
            Task::none()
        }

        Message::ClearQueue => {
            state.queue.clear();
            close_player(state);
            state.export_status = ExportStatus::Idle;
            Task::none()
        }

        Message::TogglePlay => {
            if state.active_item().is_some() {
                if state.is_playing {
                    state.player.pause();
                    state.is_playing = false;
                    // Park on the frame that is actually on screen.
                    if let Some(frame) = &state.current_frame {
                        state.current_timestamp = frame.pts;
                    }
                } else {
                    // Restart from the beginning when sitting at the end.
                    let dur = state.active_duration();
                    if state.player.has_ended() || (dur > 0.0 && state.current_timestamp >= dur - 0.05) {
                        state.current_timestamp = 0.0;
                        state.player.seek(0.0);
                    }
                    state.player.play();
                    state.is_playing = true;
                }
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
                state.player.seek(new_sec);
            }
            Task::none()
        }

        Message::Seek(sec) => {
            state.current_timestamp = sec.clamp(0.0, state.active_duration());
            state.player.seek(state.current_timestamp);
            Task::none()
        }

        Message::PlayerEvent(event) => {
            match event {
                PlayerEvent::FrameReady => present(state),
                PlayerEvent::Ended => {
                    present(state);
                    state.is_playing = false;
                }
                PlayerEvent::Error(err) => eprintln!("Player error: {err}"),
            }
            Task::none()
        }

        Message::FrameTick => {
            present(state);
            animate_controls(state);
            Task::none()
        }

        Message::ToggleFullscreen => {
            state.is_fullscreen = !state.is_fullscreen;
            // Always enter (and leave) with a clean screen.
            state.controls_visible = false;
            state.controls_reveal = 0.0;
            state.controls_anim_last = None;
            state.cursor_y = None;
            state.pointer_pressed = false;
            let mode = if state.is_fullscreen {
                window::Mode::Fullscreen
            } else {
                window::Mode::Windowed
            };
            window::latest().and_then(move |id| {
                window::set_mode(id, mode)
            })
        }

        Message::WindowResized(height) => {
            state.window_height = height;
            refresh_controls(state);
            Task::none()
        }

        Message::CursorMoved(y) => {
            state.cursor_y = Some(y);
            refresh_controls(state);
            Task::none()
        }

        Message::CursorLeft => {
            state.cursor_y = None;
            refresh_controls(state);
            Task::none()
        }

        Message::PointerPressed(pressed) => {
            state.pointer_pressed = pressed;
            refresh_controls(state);
            Task::none()
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
    }
}

/// Puts the frame that is due on screen and syncs the timecode to the clock.
fn present(state: &mut AppState) {
    if let Some(frame) = state.player.poll_frame() {
        state.current_frame = Some(frame);
    }
    if state.is_playing {
        let dur = state.active_duration();
        let pos = state.player.position();
        state.current_timestamp = if dur > 0.0 { pos.min(dur) } else { pos };
    }
}

/// Decides whether the fullscreen controls should be up: summoned only at the
/// very bottom edge, then kept while the cursor stays over them. A drag in
/// progress (e.g. scrubbing) never dismisses them.
fn refresh_controls(state: &mut AppState) {
    let zone = if state.controls_visible {
        player_view::FULLSCREEN_KEEP_ZONE
    } else {
        player_view::FULLSCREEN_REVEAL_ZONE
    };
    let near_bottom = state
        .cursor_y
        .is_some_and(|y| y >= state.window_height - zone);

    if near_bottom {
        state.controls_visible = true;
    } else if !state.pointer_pressed {
        state.controls_visible = false;
    }
}

/// Advances the slide of the fullscreen controls toward its target.
fn animate_controls(state: &mut AppState) {
    const SLIDE_SECS: f32 = 0.18;

    if !state.controls_animating() {
        state.controls_anim_last = None;
        return;
    }
    let now = Instant::now();
    let dt = state
        .controls_anim_last
        .map_or(1.0 / 60.0, |last| (now - last).as_secs_f32().min(0.05));
    let step = dt / SLIDE_SECS;
    state.controls_reveal = if state.controls_visible {
        (state.controls_reveal + step).min(1.0)
    } else {
        (state.controls_reveal - step).max(0.0)
    };
    state.controls_anim_last = Some(now);
}

fn reset_transport(state: &mut AppState) {
    state.is_playing = false;
    state.current_timestamp = 0.0;
    state.in_point = None;
    state.out_point = None;
    state.current_frame = None;
}

fn open_in_player(state: &mut AppState, path: PathBuf) {
    reset_transport(state);
    state.player.load(path);
}

fn close_player(state: &mut AppState) {
    reset_transport(state);
    state.active_index = None;
    state.player.close();
}

fn subscription(state: &AppState) -> Subscription<Message> {
    let mut subs = vec![
        iced::time::every(Duration::from_secs(1)).map(|_| Message::TickTelemetry),
        iced::event::listen_with(window_events),
    ];

    if state.is_fullscreen {
        subs.push(iced::event::listen_with(fullscreen_events));
    }

    if state.is_playing || state.controls_animating() {
        // One tick per display refresh; the audio clock decides which frame.
        subs.push(window::frames().map(|_| Message::FrameTick));
    }

    Subscription::batch(subs)
}

fn window_events(event: Event, _status: event::Status, _window: window::Id) -> Option<Message> {
    match event {
        Event::Window(window::Event::Resized(size)) => Some(Message::WindowResized(size.height)),
        _ => None,
    }
}

/// Pointer and keyboard input that matters only in fullscreen. Captured
/// events count too: the cursor is usually over a widget when it matters.
fn fullscreen_events(event: Event, _status: event::Status, _window: window::Id) -> Option<Message> {
    match event {
        Event::Mouse(mouse::Event::CursorMoved { position }) => {
            Some(Message::CursorMoved(position.y))
        }
        Event::Mouse(mouse::Event::CursorLeft) => Some(Message::CursorLeft),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
            Some(Message::PointerPressed(true))
        }
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::PointerPressed(false))
        }
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        }) => Some(Message::ToggleFullscreen),
        _ => None,
    }
}

fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    layout::view(state)
}
