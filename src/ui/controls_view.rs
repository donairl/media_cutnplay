use crate::core::cutter::CutMode;
use crate::state::app_state::{AppState, ExportStatus};
use crate::state::messages::Message;
use crate::ui::{icons, theme};
use iced::widget::{
    button, checkbox, column, container, progress_bar, row, scrollable, space, text,
};
use iced::{Alignment, Color, Element, Length};

pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    let header = theme::title("Cut & Export");

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

        section_card(
            "Source",
            column![
                spec_row("Format", item.format_name.clone()),
                spec_row("Video", video_codec.to_string()),
                spec_row("Audio", audio_codec.to_string()),
                spec_row("Bitrate", format!("{} kbps", item.bitrate / 1000)),
            ]
            .spacing(6)
            .into(),
        )
    } else {
        section_card("Source", theme::supporting("No media selected").into())
    };

    let export_segments = state.export_segments();
    let cut_duration = if export_segments.is_empty() {
        "--:--:--".into()
    } else {
        format_time(export_segments.iter().map(|s| s.duration()).sum())
    };

    let mut cut_rows = column![
        mode_picker(state.cut_mode),
        theme::supporting(match state.cut_mode {
            CutMode::Lossless => "Instant stream copy. Each segment starts on a keyframe.",
            CutMode::Smart => "Frame-accurate. Re-encodes only the frames at the cut points. H.264 only.",
        }),
        spec_row("Segments", export_segments.len().to_string()),
        spec_row("Duration", cut_duration),
    ]
    .spacing(6);

    if state.segments.is_empty() {
        cut_rows = cut_rows.push(theme::supporting(
            "Mark In and Out, then Add segment to cut several parts.",
        ));
    } else {
        for (index, segment) in state.segments.iter().enumerate() {
            cut_rows = cut_rows.push(segment_row(index, segment.start, segment.end));
        }
        cut_rows = cut_rows.push(
            row![
                checkbox(state.merge_segments)
                    .label("Merge into one file")
                    .size(16)
                    .text_size(12)
                    .on_toggle(Message::SetMergeSegments),
                space().width(Length::Fill),
                button(theme::label("Clear"))
                    .padding(theme::COMPACT_PADDING)
                    .style(theme::text_button)
                    .on_press(Message::ClearSegments),
            ]
            .align_y(Alignment::Center),
        );
    }

    let cut_section = section_card("Cut", cut_rows.into());

    let output_str = state
        .target_output_path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("Default: <input>_cut.<ext>");
    let split_output = !state.merge_segments && state.segments.len() > 1;

    let output_section = section_card(
        "Destination",
        column![
            theme::supporting(output_str).wrapping(text::Wrapping::WordOrGlyph),
            split_output.then(|| theme::supporting("One file per segment: _01, _02, ...")),
            button(theme::icon_label(icons::FOLDER_OPEN, "Choose file..."))
                .padding([8.0, 16.0])
                .style(theme::outlined)
                .on_press(Message::SelectOutputPath),
        ]
        .spacing(10)
        .into(),
    );

    let action_section: Element<'a, Message> = match &state.export_status {
        ExportStatus::Idle => {
            let can_cut = state.active_item().is_some() && !export_segments.is_empty();

            let label = match state.cut_mode {
                CutMode::Lossless => "Start lossless cut",
                CutMode::Smart => "Start smart cut",
            };
            action_button(icons::CONTENT_CUT, label, theme::filled)
                .on_press_maybe(can_cut.then_some(Message::StartLosslessCut))
                .into()
        }
        ExportStatus::Processing { progress, .. } => column![
            theme::body(format!("Cutting... {:.0}%", progress * 100.0)),
            progress_bar(0.0..=1.0, *progress)
                .length(Length::Fill)
                .girth(theme::PROGRESS_GIRTH)
                .style(theme::progress),
        ]
        .spacing(10)
        .into(),
        ExportStatus::Completed { outputs } => {
            let summary = match outputs.as_slice() {
                [single] => single
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("File")
                    .to_string(),
                many => format!("{} files", many.len()),
            };
            column![
                status_line(
                    icons::CHECK_CIRCLE,
                    format!("Cut complete: {summary}"),
                    theme::SUCCESS,
                ),
                action_button(icons::CONTENT_CUT, "Cut again", theme::tonal).on_press(Message::StartLosslessCut),
            ]
            .spacing(10)
            .into()
        }
        ExportStatus::Failed(err) => column![
            status_line(icons::ERROR, format!("Error: {err}"), theme::ERROR),
            action_button(icons::CONTENT_CUT, "Retry", theme::tonal).on_press(Message::StartLosslessCut),
        ]
        .spacing(10)
        .into(),
    };

    container(
        column![
            header,
            scrollable(column![info_section, cut_section, output_section].spacing(12))
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

/// A titled card grouping related settings.
fn section_card<'a>(title: &'a str, content: Element<'a, Message>) -> Element<'a, Message> {
    container(column![theme::section(title), content].spacing(8))
        .padding(16)
        .width(Length::Fill)
        .style(theme::inner_card)
        .into()
}

/// Label on the left, value on the right.
fn spec_row<'a>(label: &'a str, value: String) -> Element<'a, Message> {
    row![
        theme::supporting(label),
        space().width(Length::Fill),
        text(value).size(12).color(theme::ON_SURFACE),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

/// Two-way switch between the cut modes.
fn mode_picker<'a>(current: CutMode) -> Element<'a, Message> {
    let option = |label: &'a str, mode: CutMode| {
        let style: fn(&iced::Theme, button::Status) -> button::Style =
            if mode == current { theme::tonal } else { theme::outlined };
        button(container(theme::label(label)).center_x(Length::Fill))
            .width(Length::Fill)
            .padding(theme::COMPACT_PADDING)
            .style(style)
            .on_press(Message::SetCutMode(mode))
    };
    row![option("Lossless", CutMode::Lossless), option("Smart", CutMode::Smart)]
        .spacing(8)
        .into()
}

/// One committed segment: click to jump to its start, X to drop it.
fn segment_row<'a>(index: usize, start: f64, end: f64) -> Element<'a, Message> {
    let label = format!("{:02}  {} - {}", index + 1, format_time(start), format_time(end));
    row![
        button(text(label).size(12))
            .padding(theme::COMPACT_PADDING)
            .width(Length::Fill)
            .style(theme::text_button)
            .on_press(Message::Seek(start)),
        button(icons::icon(icons::CLOSE, 16))
            .padding(6)
            .style(theme::icon_button)
            .on_press(Message::RemoveSegment(index)),
    ]
    .spacing(4)
    .align_y(Alignment::Center)
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
