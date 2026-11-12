use crate::state::app_state::{AppState, ExportStatus};
use crate::state::messages::Message;
use crate::ui::{icons, theme};
use iced::widget::{button, column, container, progress_bar, row, space, text, tooltip};
use iced::{Alignment, Color, Element, Length};

pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    if !state.show_export_panel {
        return theme::panel_rail(theme::panel_toggle(
            icons::RIGHT_PANEL_OPEN,
            "Show Cut & Export",
            tooltip::Position::Left,
            Message::ToggleExportPanel,
        ));
    }

    let header = row![
        theme::title("Cut & Export"),
        space().width(Length::Fill),
        theme::panel_toggle(
            icons::RIGHT_PANEL_CLOSE,
            "Hide Cut & Export",
            tooltip::Position::Bottom,
            Message::ToggleExportPanel,
        ),
    ]
    .align_y(Alignment::Center);

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

    let cut_duration = match (state.in_point, state.out_point) {
        (Some(i), Some(o)) if o > i => format_time(o - i),
        _ => "--:--:--".into(),
    };

    let cut_section = section_card(
        "Cut",
        column![
            spec_row("Mode", "Lossless (-c copy)".to_string()),
            spec_row("Duration", cut_duration),
            theme::supporting("Aligns to keyframes instantly, without re-encoding quality loss."),
        ]
        .spacing(6)
        .into(),
    );

    let output_str = state
        .target_output_path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("Default: <input>_cut.<ext>");

    let output_section = section_card(
        "Destination",
        column![
            theme::supporting(output_str).wrapping(text::Wrapping::WordOrGlyph),
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
            let can_cut = state.active_item().is_some()
                && matches!((state.in_point, state.out_point), (Some(i), Some(o)) if o > i);

            action_button(icons::CONTENT_CUT, "Start lossless cut", theme::filled)
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
        ExportStatus::Completed { output_path } => {
            let filename = output_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("File");
            column![
                status_line(
                    icons::CHECK_CIRCLE,
                    format!("Cut complete: {filename}"),
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
            info_section,
            cut_section,
            output_section,
            space().height(Length::Fill),
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
