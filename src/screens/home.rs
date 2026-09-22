use iced::widget::{button, column, container, row, text};
use iced::{Element, Length};

use crate::app::Message;
use crate::style;

#[derive(Debug, Clone)]
pub enum HomeMessage {
    StartGame,
    ViewStats,
    ViewHistory,
    ManagePlayers,
}

fn tile<'a>(label: &'a str, sub: &'a str, message: Message, primary: bool) -> Element<'a, Message> {
    button(
        container(
            column![text(label).size(style::T_HEADING), text(sub).size(style::T_CAPTION)]
                .spacing(6)
                .align_x(iced::Alignment::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .padding(0)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(if primary {
        style::primary
    } else {
        style::secondary
    })
    .on_press(message)
    .into()
}

pub fn view<'a>() -> Element<'a, Message> {
    let header = container(
        column![
            text("Commander Pod").size(style::T_DISPLAY),
            text("Life, damage and win tracking for your playgroup").size(style::T_CAPTION),
        ]
        .spacing(6)
        .align_x(iced::Alignment::Center),
    )
    .padding(28)
    .width(Length::Fill)
    .center_x(Length::Fill)
    .style(style::header);

    let top_row = row![
        tile(
            "Start Game",
            "Pick players and commanders",
            Message::Home(HomeMessage::StartGame),
            true,
        ),
        tile(
            "Stats",
            "Win rates and matchups",
            Message::Home(HomeMessage::ViewStats),
            false,
        ),
    ]
    .spacing(style::GAP)
    .height(Length::FillPortion(1));

    let bottom_row = row![
        tile(
            "Game History",
            "Review past games",
            Message::Home(HomeMessage::ViewHistory),
            false,
        ),
        tile(
            "Players",
            "Add or rename people",
            Message::Home(HomeMessage::ManagePlayers),
            false,
        ),
    ]
    .spacing(style::GAP)
    .height(Length::FillPortion(1));

    container(
        column![header, top_row, bottom_row]
            .spacing(style::GAP)
            .padding(style::GAP)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
