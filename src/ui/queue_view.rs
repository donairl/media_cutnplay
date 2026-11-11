use crate::state::app_state::AppState;
use crate::state::messages::Message;
use iced::widget::{button, column, container, row, scrollable, space, text};
use iced::{Alignment, Element, Length};

pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    let header = row![
        text("Media Queue").size(16),
        space().width(Length::Fill),
        button("+ Add").on_press(Message::OpenFilePicker),
        button("Clear").on_press(Message::ClearQueue),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let list_content: Element<'a, Message> = if state.queue.is_empty() {
        container(
            text("No media files added.\nClick '+ Add' to select files.")
                .size(13),
        )
        .padding(16)
        .center_x(Length::Fill)
        .into()
    } else {
        let items: Vec<Element<'a, Message>> = state
            .queue
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let is_selected = state.active_index == Some(idx);
                let name = item
                    .file_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Unknown");

                let resolution_text = item
                    .video
                    .as_ref()
                    .map(|v| format!("{}x{}", v.width, v.height))
                    .unwrap_or_else(|| "Audio Only".into());

                let size_mb = item.file_size_bytes as f64 / (1024.0 * 1024.0);
                let dur = format_time(item.duration_secs);

                let item_btn = button(
                    column![
                        text(name).size(13),
                        row![
                            text(resolution_text).size(11),
                            text("•").size(11),
                            text(dur).size(11),
                            text("•").size(11),
                            text(format!("{:.1} MB", size_mb)).size(11),
                        ]
                        .spacing(4),
                    ]
                    .spacing(3),
                )
                .width(Length::Fill)
                .on_press(Message::SelectQueueItem(idx));

                let remove_btn = button("✕")
                    .on_press(Message::RemoveQueueItem(idx));

                let row_content = row![item_btn, remove_btn]
                    .spacing(6)
                    .align_y(Alignment::Center);

                if is_selected {
                    container(row_content)
                        .padding(2)
                        .into()
                } else {
                    container(row_content)
                        .padding(2)
                        .into()
                }
            })
            .collect();

        scrollable(column(items).spacing(6))
            .height(Length::Fill)
            .into()
    };

    container(column![header, list_content].spacing(10))
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
