use iced::widget::{button, column, container, text};
use iced::{Element, Length};

use crate::app::Message;
use crate::style;

#[derive(Debug, Clone)]
pub enum HomeMessage {
    StartGame,
    ViewStats,
    ViewHistory,
}

fn big_button<'a>(label: &'a str, message: Message) -> Element<'a, Message> {
    button(
        container(text(label).size(28))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fixed(420.0))
    .height(Length::Fixed(120.0))
    .style(button::primary)
    .on_press(message)
    .into()
}

pub fn view<'a>() -> Element<'a, Message> {
    let header = container(text("Commander Pod").size(40))
        .padding(20)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .style(style::header);

    container(
        column![
            header,
            column![
                big_button("Start Game", Message::Home(HomeMessage::StartGame)),
                big_button("Stats", Message::Home(HomeMessage::ViewStats)),
                big_button("Game History", Message::Home(HomeMessage::ViewHistory)),
            ]
            .spacing(24)
            .align_x(iced::Alignment::Center),
        ]
        .spacing(60)
        .align_x(iced::Alignment::Center)
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_y(Length::Fill)
    .padding(20)
    .into()
}
