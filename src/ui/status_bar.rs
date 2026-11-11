use crate::state::app_state::{AppState, ExportStatus};
use crate::state::messages::Message;
use iced::widget::{container, row, space, text};
use iced::{Alignment, Element, Length};

pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    let status_text = match &state.export_status {
        ExportStatus::Idle => "Status: Ready",
        ExportStatus::Processing { .. } => "Status: Cutting media...",
        ExportStatus::Completed { .. } => "Status: Completed",
        ExportStatus::Failed(_err) => "Status: Cut Error",
    };

    let ram_text = format!("RAM: {:.1} MB", state.ram_usage_mb);
    let hw_text = format!("Hardware: {}", state.detected_hw);

    let content = row![
        text(status_text).size(11),
        space().width(Length::Fill),
        text(hw_text).size(11),
        space().width(Length::Fill),
        text(ram_text).size(11),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    container(content)
        .padding(6)
        .width(Length::Fill)
        .into()
}
