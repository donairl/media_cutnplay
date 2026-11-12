use crate::state::app_state::AppState;
use crate::state::messages::Message;
use crate::ui::{icons, theme};
use crate::ui::video_view::VideoProgram;
use iced::widget::{
    button, column, container, float, row, shader, slider, space, stack, text, tooltip,
};
use iced::{Alignment, Element, Length, Vector};

/// Distance from the bottom edge that summons the fullscreen controls.
pub const FULLSCREEN_REVEAL_ZONE: f32 = 6.0;
/// While shown, the controls stay as long as the cursor is within this band.
pub const FULLSCREEN_KEEP_ZONE: f32 = 190.0;
const FULLSCREEN_BAR_MARGIN: f32 = 24.0;

pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    container(
        column![
            preview(state),
            scrubber(state),
            transport_bar(state),
            markers_bar(state),
        ]
        .spacing(12),
    )
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(theme::card)
    .into()
}

/// Fullscreen: nothing but the picture. The transport floats up from the
/// bottom edge while `controls_reveal` is above zero.
pub fn fullscreen_view<'a>(state: &'a AppState) -> Element<'a, Message> {
    let video = preview(state);
    if state.controls_reveal <= 0.0 {
        return video;
    }

    let bar = container(column![scrubber(state), transport_bar(state)].spacing(8))
        .padding([12.0, 20.0])
        .max_width(960.0)
        .style(theme::floating_bar);

    // Ease-out cubic; the bar travels its own height plus the margin.
    let hidden = (1.0 - state.controls_reveal).powi(3);
    let sliding = float(bar).translate(move |bounds, _viewport| {
        Vector::new(0.0, hidden * (bounds.height + FULLSCREEN_BAR_MARGIN))
    });

    stack![
        video,
        container(sliding)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::End)
            .padding(FULLSCREEN_BAR_MARGIN),
    ]
    .into()
}

fn preview<'a>(state: &'a AppState) -> Element<'a, Message> {
    if let Some(frame) = &state.current_frame {
        container(
            shader(VideoProgram {
                frame: frame.clone(),
            })
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::video_backdrop)
        .into()
    } else {
        let style = if state.is_fullscreen {
            theme::video_backdrop
        } else {
            theme::placeholder
        };
        container(
            column![
                theme::title("No video loaded"),
                theme::supporting("Add and select a media file to begin previewing and cutting"),
            ]
            .spacing(8)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(style)
        .into()
    }
}

fn scrubber<'a>(state: &'a AppState) -> Element<'a, Message> {
    slider(
        0.0..=state.active_duration().max(0.001),
        state.current_timestamp,
        Message::Seek,
    )
    .step(0.01)
    .width(Length::Fill)
    .height(theme::SLIDER_HEIGHT)
    .style(theme::scrubber)
    .into()
}

fn transport_bar<'a>(state: &'a AppState) -> Element<'a, Message> {
    let has_media = state.active_item().is_some();

    let timecode = row![
        text(format_time(state.current_timestamp))
            .size(14)
            .color(theme::ON_SURFACE),
        theme::supporting(format!("/ {}", format_time(state.active_duration()))),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let step_button = |glyph: char, hint: &'static str, steps: i32| {
        with_tooltip(
            button(icons::icon(glyph, 20))
                .padding(theme::ICON_PADDING)
                .style(theme::icon_button)
                .on_press_maybe(has_media.then_some(Message::StepFrame(steps))),
            hint,
        )
    };

    let (play_icon, play_label) = if state.is_playing {
        (icons::PAUSE, "Pause")
    } else {
        (icons::PLAY_ARROW, "Play")
    };
    let play_button = button(theme::icon_label(play_icon, play_label))
        .padding([14.0, 20.0])
        .style(theme::fab)
        .on_press_maybe(has_media.then_some(Message::TogglePlay));

    let (fs_icon, fs_hint) = if state.is_fullscreen {
        (icons::FULLSCREEN_EXIT, "Exit fullscreen")
    } else {
        (icons::FULLSCREEN, "Fullscreen")
    };

    row![
        container(timecode).width(Length::FillPortion(1)),
        row![
            step_button(icons::FAST_REWIND, "Back 1 s", -25),
            step_button(icons::SKIP_PREVIOUS, "Previous frame", -1),
            play_button,
            step_button(icons::SKIP_NEXT, "Next frame", 1),
            step_button(icons::FAST_FORWARD, "Forward 1 s", 25),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
        container(with_tooltip(
            button(icons::icon(fs_icon, 20))
                .padding(theme::ICON_PADDING)
                .style(theme::icon_button)
                .on_press(Message::ToggleFullscreen),
            fs_hint,
        ))
        .width(Length::FillPortion(1))
        .align_x(Alignment::End),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn markers_bar<'a>(state: &'a AppState) -> Element<'a, Message> {
    let has_media = state.active_item().is_some();
    let in_str = state
        .in_point
        .map(format_time)
        .unwrap_or_else(|| "--:--:--".into());
    let out_str = state
        .out_point
        .map(format_time)
        .unwrap_or_else(|| "--:--:--".into());

    let marker_button = |label: &'static str, message: Message| {
        button(theme::label(label))
            .padding([8.0, 16.0])
            .style(theme::tonal)
            .on_press_maybe(has_media.then_some(message))
    };

    let marker_chip = |value: String| {
        container(text(value).size(12))
            .padding([6.0, 12.0])
            .style(theme::chip)
    };

    let has_points = state.in_point.is_some() || state.out_point.is_some();

    row![
        marker_button("Set In", Message::SetInPoint),
        marker_chip(in_str),
        space().width(Length::Fixed(12.0)),
        marker_button("Set Out", Message::SetOutPoint),
        marker_chip(out_str),
        space().width(Length::Fill),
        button(theme::label("Clear points"))
            .padding(theme::COMPACT_PADDING)
            .style(theme::text_button)
            .on_press_maybe(has_points.then_some(Message::ClearPoints)),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn with_tooltip<'a>(
    content: impl Into<Element<'a, Message>>,
    hint: &'a str,
) -> Element<'a, Message> {
    theme::with_tooltip(content, hint, tooltip::Position::Top)
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
