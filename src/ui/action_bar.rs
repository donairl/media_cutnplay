use crate::state::app_state::{AdjustKind, AppState};
use crate::state::messages::Message;
use crate::ui::{icons, theme};
use iced::widget::{button, column, container, space, text, tooltip, vertical_slider};
use iced::{Alignment, Element, Length};

const ADJUST_SLIDER_HEIGHT: f32 = 220.0;

/// Vertical bar on the left edge holding the side-panel toggles and the
/// preview picture settings.
pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    let adjust_toggle = |glyph: char, hint: &'a str, kind: AdjustKind| {
        toggle(
            glyph,
            hint,
            state.adjust_panel == Some(kind),
            Message::ToggleAdjustPanel(kind),
        )
    };

    container(
        column![
            toggle(
                icons::VIDEO_LIBRARY,
                "Media Queue",
                state.show_queue_panel,
                Message::ToggleQueuePanel,
            ),
            toggle(
                icons::MOVIE_EDIT,
                "Cut & Export",
                state.show_export_panel,
                Message::ToggleExportPanel,
            ),
            toggle(
                icons::TRANSFORM,
                "Transcode",
                state.show_transcode_panel,
                Message::ToggleTranscodePanel,
            ),
            space().height(Length::Fixed(8.0)),
            adjust_toggle(icons::BRIGHTNESS, "Brightness", AdjustKind::Brightness),
            adjust_toggle(icons::CONTRAST, "Contrast", AdjustKind::Contrast),
            toggle(
                icons::FILTER_B_AND_W,
                "Monochrome",
                state.adjust.monochrome,
                Message::ToggleMonochrome,
            ),
        ]
        .spacing(8),
    )
    .padding(8)
    .height(Length::Fill)
    .style(theme::card)
    .into()
}

/// Flyout beside the action bar: a slider for one picture setting.
pub fn adjust_panel<'a>(state: &'a AppState, kind: AdjustKind) -> Element<'a, Message> {
    let (glyph, hint) = match kind {
        AdjustKind::Brightness => (icons::BRIGHTNESS, "Reset brightness"),
        AdjustKind::Contrast => (icons::CONTRAST, "Reset contrast"),
    };
    let value = state.adjust.get(kind);
    let percent = (value * 100.0).round() as i32;

    container(
        column![
            icons::icon(glyph, 20).color(theme::ON_SURFACE_VARIANT),
            vertical_slider(-1.0..=1.0, value, move |v| Message::SetAdjust(kind, v))
                .step(0.01)
                .width(theme::SLIDER_HEIGHT)
                .height(Length::Fixed(ADJUST_SLIDER_HEIGHT))
                .style(theme::scrubber),
            text(format!("{percent:+}")).size(12).color(theme::ON_SURFACE),
            theme::with_tooltip(
                button(icons::icon(icons::RESTART_ALT, 20))
                    .padding(theme::ICON_PADDING)
                    .style(theme::icon_button)
                    .on_press_maybe((percent != 0).then_some(Message::ResetAdjust(kind))),
                hint,
                tooltip::Position::Right,
            ),
        ]
        .spacing(12)
        .align_x(Alignment::Center),
    )
    .padding([16.0, 8.0])
    .height(Length::Fill)
    .style(theme::card)
    .into()
}

/// Icon button that shows or hides a panel; highlighted while it is shown.
fn toggle<'a>(
    glyph: char,
    hint: &'a str,
    shown: bool,
    message: Message,
) -> Element<'a, Message> {
    theme::with_tooltip(
        button(icons::icon(glyph, 20))
            .padding(theme::ICON_PADDING)
            .style(move |_theme, status| theme::icon_toggle(shown, status))
            .on_press(message),
        hint,
        tooltip::Position::Right,
    )
}
