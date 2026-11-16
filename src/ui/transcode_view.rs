use crate::core::transcode::{
    AudioMode, Container, EncoderBackend, QualityPreset, RateMode, Resolution, TranscodeSettings,
    VideoCodec, BITRATE_RANGE, BITRATE_STEP, SATURATION_RANGE,
};
use crate::state::app_state::{AppState, ExportStatus};
use crate::state::messages::Message;
use crate::ui::{icons, theme};
use iced::widget::{button, column, container, progress_bar, row, scrollable, slider, text};
use iced::{Alignment, Color, Element, Length};

pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    let settings = state.transcode;
    let busy = matches!(state.transcode_status, ExportStatus::Processing { .. });
    // Each option is the current settings with one field swapped.
    let with = |change: fn(&mut TranscodeSettings)| {
        let mut next = settings;
        change(&mut next);
        (!busy).then_some(Message::SetTranscode(next))
    };

    let video_section = section_card(
        "Video",
        column![
            option_row(
                "Codec",
                vec![
                    (
                        "H.264",
                        settings.codec == VideoCodec::H264,
                        with(|s| *s = s.with_codec(VideoCodec::H264)),
                    ),
                    (
                        "HEVC",
                        settings.codec == VideoCodec::Hevc,
                        with(|s| *s = s.with_codec(VideoCodec::Hevc)),
                    ),
                    (
                        "AV1",
                        settings.codec == VideoCodec::Av1,
                        with(|s| *s = s.with_codec(VideoCodec::Av1)),
                    ),
                    (
                        "VP9",
                        settings.codec == VideoCodec::Vp9,
                        with(|s| *s = s.with_codec(VideoCodec::Vp9)),
                    ),
                ],
            ),
            option_row(
                "Encoder",
                vec![
                    (
                        "CPU",
                        settings.backend == EncoderBackend::Software,
                        with(|s| s.backend = EncoderBackend::Software),
                    ),
                    (
                        "NVENC",
                        settings.backend == EncoderBackend::Nvenc,
                        with(|s| s.backend = EncoderBackend::Nvenc)
                            .filter(|_| state.nvenc.is_some_and(|gpu| gpu.supports(settings.codec))),
                    ),
                ],
            ),
        ]
        .push(nvenc_note(state).map(theme::supporting))
        .push(
            option_row(
                "Rate control",
                vec![
                    (
                        "Quality",
                        settings.rate_mode == RateMode::Quality,
                        with(|s| s.rate_mode = RateMode::Quality),
                    ),
                    (
                        "Bitrate",
                        settings.rate_mode == RateMode::Bitrate,
                        with(|s| s.rate_mode = RateMode::Bitrate),
                    ),
                ],
            ),
        )
        .push(rate_slider(settings))
        .push((settings.rate_mode == RateMode::Quality).then(|| {
            option_row(
                "Preset",
                QualityPreset::ALL
                    .into_iter()
                    .map(|preset| {
                        let crf = preset.crf(settings.codec);
                        let label = match preset {
                            QualityPreset::High => "High",
                            QualityPreset::Medium => "Medium",
                            QualityPreset::Low => "Low",
                        };
                        let message = (!busy)
                            .then_some(Message::SetTranscode(TranscodeSettings { crf, ..settings }));
                        (label, settings.crf == crf, message)
                    })
                    .collect(),
            )
        }))
        .push(
            option_row(
                "Resolution",
                vec![
                    (
                        "Source",
                        settings.resolution == Resolution::Source,
                        with(|s| s.resolution = Resolution::Source),
                    ),
                    (
                        "1080p",
                        settings.resolution == Resolution::P1080,
                        with(|s| s.resolution = Resolution::P1080),
                    ),
                    (
                        "720p",
                        settings.resolution == Resolution::P720,
                        with(|s| s.resolution = Resolution::P720),
                    ),
                    (
                        "480p",
                        settings.resolution == Resolution::P480,
                        with(|s| s.resolution = Resolution::P480),
                    ),
                ],
            ),
        )
        .push(saturation_slider(settings, busy))
        .push(theme::supporting(output_summary(state)))
        .spacing(10)
        .into(),
    );

    let output_section = section_card(
        "Audio & container",
        column![
            option_row(
                "Audio",
                vec![
                    ("AAC", settings.audio == AudioMode::Aac, with(|s| *s = s.with_audio(AudioMode::Aac))),
                    ("Opus", settings.audio == AudioMode::Opus, with(|s| *s = s.with_audio(AudioMode::Opus))),
                    ("Copy", settings.audio == AudioMode::Copy, with(|s| *s = s.with_audio(AudioMode::Copy))),
                ],
            ),
            audio_bitrate_row(settings, busy),
            option_row(
                "Container",
                vec![
                    (
                        "MP4",
                        settings.container == Container::Mp4,
                        with(|s| s.container = Container::Mp4),
                    ),
                    (
                        "MKV",
                        settings.container == Container::Mkv,
                        with(|s| s.container = Container::Mkv),
                    ),
                ],
            ),
        ]
        .spacing(10)
        .into(),
    );

    let queued_videos = state.queue.iter().filter(|item| item.video.is_some()).count();
    let default_name = format!("Default: <input>_transcoded.{}", settings.extension());
    let output_name = state
        .transcode_output_path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or(&default_name)
        .to_string();

    let destination_section = section_card(
        "Destination",
        column![
            theme::supporting(output_name).wrapping(text::Wrapping::WordOrGlyph),
            button(theme::icon_label(icons::FOLDER_OPEN, "Choose file..."))
                .padding([8.0, 16.0])
                .style(theme::outlined)
                .on_press_maybe((!busy).then_some(Message::SelectTranscodeOutput)),
        ]
        .push((queued_videos > 1).then(|| {
            theme::supporting("Transcode all saves each file next to its source.")
        }))
        .spacing(10)
        .into(),
    );

    let has_video = state.active_item().is_some_and(|item| item.video.is_some());
    // Primary action, plus "all" once there is more than one video to do.
    let start = |label: &'a str, style| -> Element<'a, Message> {
        let one = action_button(icons::TRANSFORM, label, style)
            .on_press_maybe(has_video.then_some(Message::StartTranscode));
        if queued_videos > 1 {
            let all = action_button(icons::TRANSFORM, "Transcode all", theme::outlined)
                .on_press(Message::StartTranscodeAll);
            column![one, all, theme::supporting(format!("All {queued_videos} videos in the queue."))]
                .spacing(8)
                .into()
        } else {
            one.into()
        }
    };
    let batch = state.transcode_total > 1;

    let action_section: Element<'a, Message> = match &state.transcode_status {
        ExportStatus::Idle => start("Start transcode", theme::filled),
        ExportStatus::Processing { progress, output_path } => {
            let label = if batch {
                format!(
                    "Transcoding {}/{}... {:.0}%",
                    state.transcode_job_number(),
                    state.transcode_total,
                    progress * 100.0
                )
            } else {
                format!("Transcoding... {:.0}%", progress * 100.0)
            };
            column![
                theme::body(label),
                progress_bar(0.0..=1.0, *progress)
                    .length(Length::Fill)
                    .girth(theme::PROGRESS_GIRTH)
                    .style(theme::progress),
            ]
            .push(batch.then(|| {
                theme::supporting(file_name(output_path).to_string()).wrapping(text::Wrapping::WordOrGlyph)
            }))
            .push(
                action_button(icons::CANCEL, if batch { "Cancel all" } else { "Cancel" }, theme::outlined)
                    .on_press(Message::CancelTranscode),
            )
            .spacing(10)
            .into()
        }
        ExportStatus::Completed { outputs } => {
            let message = match outputs.as_slice() {
                [one] => format!("Transcode complete: {}", file_name(one)),
                many => format!("Transcoded {} files", many.len()),
            };
            column![
                status_line(icons::CHECK_CIRCLE, message, theme::SUCCESS),
                start("Transcode again", theme::tonal),
            ]
            .spacing(10)
            .into()
        }
        ExportStatus::Failed(err) => column![
            status_line(icons::ERROR, format!("Error: {err}"), theme::ERROR),
            start("Retry", theme::tonal),
        ]
        .spacing(10)
        .into(),
    };

    container(
        column![
            theme::title("Transcode"),
            scrollable(column![video_section, output_section, destination_section].spacing(12))
                .height(Length::Fill)
                .spacing(4)
                .style(theme::scroll),
            action_section,
        ]
        .spacing(12),
    )
    .padding(16)
    .width(Length::Fixed(300.0))
    .height(Length::Fill)
    .style(theme::card)
    .into()
}

fn file_name(path: &std::path::Path) -> &str {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("File")
}

/// What the active file would come out as, e.g. "1920x1080 -> 1280x720".
fn output_summary(state: &AppState) -> String {
    match state.active_item().and_then(|item| item.video.as_ref()) {
        Some(video) => {
            let (width, height) = state.transcode.resolution.output_size(video.width, video.height);
            if (width, height) == (video.width, video.height) {
                format!("Output {width}x{height}, whole file.")
            } else {
                format!("{}x{} -> {width}x{height}, whole file.", video.width, video.height)
            }
        }
        None => "Select a video to transcode.".to_string(),
    }
}

/// Why the NVENC choice is off for the chosen codec, if it is.
fn nvenc_note(state: &AppState) -> Option<&'static str> {
    let codec = state.transcode.codec;
    match state.nvenc {
        None => Some("Checking the GPU encoder..."),
        Some(gpu) if gpu.supports(codec) => None,
        Some(_) if !codec.has_nvenc() => Some("NVENC has no encoder for this codec."),
        Some(gpu) if gpu == Default::default() => Some("No NVENC encoder found on this machine."),
        Some(_) => Some("This GPU cannot encode this codec."),
    }
}

/// The slider for the active rate mode: quality level or target bitrate.
fn rate_slider<'a>(settings: TranscodeSettings) -> Element<'a, Message> {
    let (value, hint, control): (String, &str, Element<'a, Message>) = match settings.rate_mode {
        RateMode::Quality => (
            format!("CRF {}", settings.crf),
            "Lower is better quality and a bigger file. Bitrate follows the content.",
            slider(settings.codec.crf_range(), settings.crf, move |crf| {
                Message::SetTranscode(TranscodeSettings { crf, ..settings })
            })
            .height(theme::SLIDER_HEIGHT)
            .style(theme::scrubber)
            .into(),
        ),
        RateMode::Bitrate => (
            format!("{:.2} Mbps", f64::from(settings.bitrate_kbps) / 1000.0),
            "Average video bitrate. File size is predictable; quality follows the content.",
            slider(BITRATE_RANGE, settings.bitrate_kbps, move |bitrate_kbps| {
                Message::SetTranscode(TranscodeSettings { bitrate_kbps, ..settings })
            })
            .step(BITRATE_STEP)
            .height(theme::SLIDER_HEIGHT)
            .style(theme::scrubber)
            .into(),
        ),
    };

    column![
        text(value).size(12).color(theme::ON_SURFACE),
        control,
        theme::supporting(hint),
    ]
    .spacing(4)
    .into()
}

/// Colour saturation, 0% (greyscale) to 200%.
fn saturation_slider<'a>(settings: TranscodeSettings, busy: bool) -> Element<'a, Message> {
    let reset = button(text("Reset").size(12))
        .padding([2.0, 8.0])
        .style(theme::outlined)
        .on_press_maybe((!busy && settings.saturation != 100).then_some(Message::SetTranscode(
            TranscodeSettings { saturation: 100, ..settings },
        )));
    column![
        row![
            text(format!("Saturation {}%", settings.saturation))
                .size(12)
                .color(theme::ON_SURFACE)
                .width(Length::Fill),
            reset,
        ]
        .align_y(Alignment::Center),
        slider(SATURATION_RANGE, settings.saturation, move |saturation| {
            Message::SetTranscode(TranscodeSettings { saturation, ..settings })
        })
        .step(5u8)
        .height(theme::SLIDER_HEIGHT)
        .style(theme::scrubber),
        theme::supporting("0% is black and white; 100% keeps the source colours."),
    ]
    .spacing(4)
    .into()
}

/// Bitrate choices for the chosen audio codec, shown only while audio is
/// re-encoded.
fn audio_bitrate_row<'a>(settings: TranscodeSettings, busy: bool) -> Element<'a, Message> {
    let title = match settings.audio {
        AudioMode::Aac => "AAC bitrate (per stereo pair)",
        AudioMode::Opus => "Opus bitrate (per stereo pair)",
        AudioMode::Copy => return column![].into(),
    };
    option_row(
        title,
        settings
            .audio
            .bitrates()
            .iter()
            .map(|&audio_kbps| {
                let message = (!busy)
                    .then_some(Message::SetTranscode(TranscodeSettings { audio_kbps, ..settings }));
                (format!("{audio_kbps}k"), settings.audio_kbps == audio_kbps, message)
            })
            .collect(),
    )
}

/// A titled card grouping related settings.
fn section_card<'a>(title: &'a str, content: Element<'a, Message>) -> Element<'a, Message> {
    container(column![theme::section(title), content].spacing(8))
        .padding(16)
        .width(Length::Fill)
        .style(theme::inner_card)
        .into()
}

/// A label over a row of mutually exclusive choices; the selected one is
/// filled. A choice without a message is disabled.
fn option_row<'a, L: text::IntoFragment<'a>>(
    label: &'a str,
    options: Vec<(L, bool, Option<Message>)>,
) -> Element<'a, Message> {
    let buttons = options.into_iter().map(|(name, selected, message)| {
        let style: fn(&iced::Theme, button::Status) -> button::Style =
            if selected { theme::tonal } else { theme::outlined };
        button(container(text(name).size(12)).center_x(Length::Fill))
            .width(Length::Fill)
            .padding([6.0, 2.0])
            .style(style)
            .on_press_maybe(message)
            .into()
    });

    column![theme::supporting(label), row(buttons).spacing(4)]
        .spacing(4)
        .into()
}

/// Result message with a leading status icon.
fn status_line<'a>(glyph: char, message: String, color: Color) -> Element<'a, Message> {
    row![
        icons::icon(glyph, 18).color(color),
        text(message)
            .size(12)
            .color(color)
            .width(Length::Fill)
            .wrapping(text::Wrapping::WordOrGlyph),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// Full-width primary action at the bottom of the pane.
fn action_button<'a>(
    glyph: char,
    label: &'a str,
    style: fn(&iced::Theme, button::Status) -> button::Style,
) -> button::Button<'a, Message> {
    button(container(theme::icon_label(glyph, label)).center_x(Length::Fill))
        .width(Length::Fill)
        .padding(theme::BUTTON_PADDING)
        .style(style)
}
