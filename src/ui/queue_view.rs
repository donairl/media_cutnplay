use crate::state::app_state::AppState;
use crate::state::messages::Message;
use crate::ui::{icons, theme};
use iced::widget::{button, column, container, row, scrollable, space, text, tooltip};
use iced::{Alignment, Element, Length};

pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    if !state.show_queue_panel {
        return theme::panel_rail(theme::panel_toggle(
            icons::LEFT_PANEL_OPEN,
            "Show media queue",
            tooltip::Position::Right,
            Message::ToggleQueuePanel,
        ));
    }

    let clear_btn = theme::with_tooltip(
        button(icons::icon(icons::DELETE, 20))
            .padding(8)
            .style(theme::icon_button)
            .on_press_maybe((!state.queue.is_empty()).then_some(Message::ClearQueue)),
        "Clear queue",
        tooltip::Position::Bottom,
    );

    let header = row![
        theme::panel_toggle(
            icons::LEFT_PANEL_CLOSE,
            "Hide media queue",
            tooltip::Position::Bottom,
            Message::ToggleQueuePanel,
        ),
        theme::title("Media Queue"),
        space().width(Length::Fill),
        clear_btn,
        button(theme::icon_label(icons::ADD, "Add"))
            .padding([8.0, 16.0])
            .style(theme::tonal)
            .on_press(Message::OpenFilePicker),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let list_content: Element<'a, Message> = if state.queue.is_empty() {
        container(
            column![
                theme::body("No media yet"),
                theme::supporting("Use Add to select files"),
            ]
            .spacing(4)
            .align_x(Alignment::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
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

                // Headline inherits the list item's content color;
                // the supporting line stays in the variant color.
                let item_btn = button(
                    column![
                        text(name).size(14).wrapping(text::Wrapping::WordOrGlyph),
                        theme::supporting(format!(
                            "{resolution_text} • {dur} • {size_mb:.1} MB"
                        )),
                    ]
                    .spacing(2),
                )
                .width(Length::Fill)
                .padding([10.0, 16.0])
                .style(move |_theme, status| theme::list_item(is_selected, status))
                .on_press(Message::SelectQueueItem(idx));

                let remove_btn = button(icons::icon(icons::CLOSE, 18))
                    .padding(8)
                    .style(theme::icon_button)
                    .on_press(Message::RemoveQueueItem(idx));

                row![item_btn, remove_btn]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .into()
            })
            .collect();

        scrollable(column(items).spacing(4))
            .height(Length::Fill)
            .spacing(4)
            .style(theme::scroll)
            .into()
    };

    container(column![header, list_content].spacing(12))
        .padding(16)
        .width(Length::Fixed(300.0))
        .height(Length::Fill)
        .style(theme::card)
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
