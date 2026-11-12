use crate::state::app_state::AppState;
use crate::state::messages::Message;
use crate::ui::{action_bar, controls_view, player_view, queue_view, status_bar, theme};
use iced::widget::{column, container, row};
use iced::{Element, Length};

pub fn view<'a>(state: &'a AppState) -> Element<'a, Message> {
    if state.is_fullscreen {
        return player_view::fullscreen_view(state);
    }

    let content: Element<'a, Message> = {
        // Hidden side panels leave the row entirely; the player takes the space.
        let mut panes = row![action_bar::view(state)];
        if let Some(kind) = state.adjust_panel {
            panes = panes.push(action_bar::adjust_panel(state, kind));
        }
        if state.show_queue_panel {
            panes = panes.push(queue_view::view(state));
        }
        panes = panes.push(player_view::view(state));
        if state.show_export_panel {
            panes = panes.push(controls_view::view(state));
        }
        let panes = panes
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
