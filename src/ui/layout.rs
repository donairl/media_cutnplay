use crate::state::app_state::AppState;
use crate::state::messages::Message;
use crate::ui::{controls_view, player_view, queue_view, status_bar, theme};
use iced::widget::{column, container, row};
use iced::{Element, Length};

pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    if state.is_fullscreen {
        return player_view::fullscreen_view(state);
    }

    let content: Element<'a, Message> = {
        let panes = row![
            queue_view::view(state),
            player_view::view(state),
            controls_view::view(state),
        ]
        .spacing(theme::GUTTER)
        .width(Length::Fill)
        .height(Length::Fill);

        column![panes, status_bar::view(state)]
            .spacing(theme::GUTTER)
            .padding(theme::GUTTER)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    };

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::surface)
        .into()
}
