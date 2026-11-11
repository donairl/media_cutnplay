use crate::state::app_state::{AppState, ExportStatus};
use crate::state::messages::Message;
use iced::widget::{button, column, container, progress_bar, space, text};
use iced::{Element, Length};

pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    let header = text("Cut & Export").size(16);

    let info_section: Element<'a, Message> = if let Some(item) = state.active_item() {
        let video_codec = item
            .video
            .as_ref()
            .map(|v| v.codec.as_str())
            .unwrap_or("None");
        let audio_codec = item
            .audio
            .as_ref()
            .map(|a| a.codec.as_str())
            .unwrap_or("None");

        column![
            text("Source Specs").size(13),
            text(format!("Format: {}", item.format_name)).size(11),
            text(format!("Video: {video_codec}")).size(11),
            text(format!("Audio: {audio_codec}")).size(11),
            text(format!("Bitrate: {} kbps", item.bitrate / 1000)).size(11),
        ]
        .spacing(4)
        .into()
    } else {
        text("No media selected").size(12).into()
    };

    let cut_mode_info = column![
        text("Cut Strategy").size(13),
        text("Mode: Lossless Cut (-c copy)").size(11),
        text("Aligns instantly to keyframes without re-encoding quality loss.").size(10),
    ]
    .spacing(3);

    let cut_duration = match (state.in_point, state.out_point) {
        (Some(i), Some(o)) if o > i => format_time(o - i),
        _ => "--:--:--".into(),
    };

    let cut_points_section = column![
        text("Cut Range").size(13),
        text(format!("Duration: {cut_duration}")).size(12),
    ]
    .spacing(4);

    let output_str = state
        .target_output_path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("Default: <input>_cut.<ext>");

    let output_section = column![
        text("Destination").size(13),
        text(output_str).size(11),
        button("Choose File...").on_press(Message::SelectOutputPath),
    ]
    .spacing(5);

    let action_section: Element<'a, Message> = match &state.export_status {
        ExportStatus::Idle => {
            let can_cut = state.active_item().is_some()
                && state.in_point.is_some()
                && state.out_point.is_some()
                && state.out_point.unwrap() > state.in_point.unwrap();

            let mut btn = button(text("▶ START LOSSLESS CUT").size(13))
                .width(Length::Fill);

            if can_cut {
                btn = btn.on_press(Message::StartLosslessCut);
            }

            column![btn].into()
        }
        ExportStatus::Processing { progress, .. } => column![
            text(format!("Cutting... {:.0}%", progress * 100.0)).size(12),
            progress_bar(0.0..=1.0, *progress).length(Length::Fill),
        ]
        .spacing(6)
        .into(),
        ExportStatus::Completed { output_path } => {
            let filename = output_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("File");
            column![
                text(format!("✓ Cut Complete: {filename}")).size(12),
                button("Cut Again").on_press(Message::StartLosslessCut).width(Length::Fill),
            ]
            .spacing(6)
            .into()
        }
        ExportStatus::Failed(err) => column![
            text(format!("✕ Error: {err}")).size(11),
            button("Retry").on_press(Message::StartLosslessCut).width(Length::Fill),
        ]
        .spacing(6)
        .into(),
    };

    container(
        column![
            header,
            info_section,
            cut_mode_info,
            cut_points_section,
            output_section,
            space().height(Length::Fill),
            action_section,
        ]
        .spacing(14),
    )
    .padding(12)
    .width(Length::Fixed(280.0))
    .height(Length::Fill)
    .into()
}

fn format_time(sec: f64) -> String {
    let total_secs = sec.floor() as u64;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    let millis = ((sec - sec.floor()) * 1000.0) as u64;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
    } else {
        format!("{minutes:02}:{seconds:02}.{millis:03}")
    }
}
