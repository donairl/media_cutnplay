use crate::state::app_state::AppState;
use crate::state::messages::Message;
use crate::ui::{controls_view, player_view, queue_view, status_bar};
use iced::widget::{column, container, row};
use iced::{Element, Length};

pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    if state.is_fullscreen {
        container(player_view::view(state))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        let main_columns = row![
            queue_view::view(state),
            player_view::view(state),
            controls_view::view(state),
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        let layout = column![
            main_columns,
            status_bar::view(state),
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
