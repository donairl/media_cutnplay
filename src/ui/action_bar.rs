use crate::state::app_state::AppState;
use crate::state::messages::Message;
use crate::ui::{icons, theme};
use iced::widget::{button, column, container, tooltip};
use iced::{Element, Length};

/// Vertical bar on the left edge holding the side-panel toggles.
pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
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
        ]
        .spacing(8),
    )
    .padding(8)
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
