use crate::state::app_state::{AppState, ExportStatus};
use crate::state::messages::Message;
use crate::ui::theme;
use iced::widget::{container, row, space, text};
use iced::{Alignment, Element, Length};

pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    // A running transcode outlasts any cut, so it takes the status slot.
    let (status_text, status_color) = match (&state.transcode_status, &state.export_status) {
        (ExportStatus::Processing { .. }, _) => ("Transcoding...", theme::PRIMARY),
        (_, status) => cut_status(status),
    };


    let ram_text = format!("RAM {:.1} MB", state.ram_usage_mb);
    let hw_text = state.detected_hw.as_str();

    let content = row![
        text(status_text).size(12).color(status_color),
        space().width(Length::Fill),
        theme::supporting(hw_text),
        space().width(Length::Fill),
        theme::supporting(ram_text),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    container(content)
        .padding([6.0, 16.0])
        .width(Length::Fill)
        .style(theme::status_bar)
        .into()
}

fn cut_status(status: &ExportStatus) -> (&'static str, iced::Color) {
    match status {
        ExportStatus::Idle => ("Ready", theme::ON_SURFACE_VARIANT),
        ExportStatus::Processing { .. } => ("Cutting media...", theme::PRIMARY),
        ExportStatus::Completed { .. } => ("Completed", theme::SUCCESS),
        ExportStatus::Failed(_err) => ("Cut error", theme::ERROR),
    }
}
