use crate::state::app_state::AppState;
use crate::state::messages::Message;
use crate::ui::video_view::VideoProgram;
use iced::widget::{button, column, container, row, shader, slider, space, text};
use iced::{Alignment, Color, Element, Length};

pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    let duration = state.active_duration();

    // 1. Preview Viewport
    let preview_element: Element<'a, Message> = if let Some(frame) = &state.current_frame {
        container(
            shader(VideoProgram {
                frame: frame.clone(),
            })
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::Style::default().background(Color::BLACK))
        .into()
    } else {
        container(
            column![
                text("No Video Loaded").size(18),
                text("Add and select a media file to begin previewing and cutting").size(13),
            ]
            .spacing(8)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    };

    // 2. Timecode Readout
    let timecode_str = format!(
        "{} / {}",
        format_time(state.current_timestamp),
        format_time(duration)
    );

    // 3. Transport Toolbar
    let play_label = if state.is_playing { "⏸ Pause" } else { "▶ Play" };
    let fs_label = if state.is_fullscreen { "⛶ Exit Fullscreen" } else { "⛶ Fullscreen" };

    let transport_bar = row![
        button("⏮ -1s").on_press(Message::StepFrame(-25)),
        button("◀ Prev").on_press(Message::StepFrame(-1)),
        button(play_label).on_press(Message::TogglePlay),
        button("Next ▶").on_press(Message::StepFrame(1)),
        button("⏭ +1s").on_press(Message::StepFrame(25)),
        space().width(Length::Fill),
        text(timecode_str).size(13),
        space().width(Length::Fill),
        button(fs_label).on_press(Message::ToggleFullscreen),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // 4. Scrubber & Cut Markers
    let scrubber = slider(
        0.0..=duration.max(0.001),
        state.current_timestamp,
        Message::Seek,
    )
    .step(0.01)
    .width(Length::Fill);

    let in_str = state
        .in_point
        .map(format_time)
        .unwrap_or_else(|| "--:--:--".into());
    let out_str = state
        .out_point
        .map(format_time)
        .unwrap_or_else(|| "--:--:--".into());

    let markers_bar = row![
        button("Set [ In ]").on_press(Message::SetInPoint),
        text(format!("In: {in_str}")).size(12),
        space().width(Length::Fixed(20.0)),
        button("Set [ Out ]").on_press(Message::SetOutPoint),
        text(format!("Out: {out_str}")).size(12),
        space().width(Length::Fill),
        button("Clear Points").on_press(Message::ClearPoints),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    container(
        column![
            preview_element,
            transport_bar,
            scrubber,
            markers_bar,
        ]
        .spacing(10),
    )
    .padding(12)
    .width(Length::Fill)
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
