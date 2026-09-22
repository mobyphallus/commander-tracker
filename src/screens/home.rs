use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length};

use crate::app::Message;
use crate::style;

#[derive(Debug, Clone)]
pub enum HomeMessage {
    StartGame,
    ViewStats,
    ViewHistory,
    ManagePlayers,
}

/// The one thing this screen exists to do. It gets the accent, the top of the
/// page and the biggest type on it; nothing else on the home screen is
/// allowed to compete.
fn primary_tile<'a>(label: &'a str, sub: &'a str, message: Message) -> Element<'a, Message> {
    button(
        container(
            column![
                text(label).size(style::T_DISPLAY),
                text(sub)
                    .size(style::T_LABEL)
                    .color(style::TEXT_ON_ACCENT_MUTED),
            ]
            .spacing(style::GAP_XS)
            .align_x(Alignment::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .padding(0)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(style::primary)
    .on_press(message)
    .into()
}

/// One of the places you can go that isn't starting a game. Deliberately
/// quiet: same shape as the primary tile, a third of its visual weight.
fn tile<'a>(label: &'a str, sub: &'a str, message: Message) -> Element<'a, Message> {
    button(
        container(
            column![
                text(label).size(style::T_HEADING),
                text(sub).size(style::T_CAPTION).color(style::TEXT_MUTED),
            ]
            .spacing(style::GAP_XS)
            .align_x(Alignment::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill),
    )
    .padding(0)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(style::secondary)
    .on_press(message)
    .into()
}

pub fn view<'a>() -> Element<'a, Message> {
    let header = container(
        column![
            text("Commander Pod").size(style::T_DISPLAY),
            text("Life, damage and win tracking for your playgroup")
                .size(style::T_CAPTION)
                .color(style::TEXT_MUTED),
        ]
        .spacing(style::GAP_SM)
        .align_x(Alignment::Center),
    )
    .padding([style::GAP, style::GAP * 2])
    .width(Length::Fill)
    .center_x(Length::Fill)
    .style(style::header);

    let start = primary_tile(
        "Start Game",
        "Pick players and commanders",
        Message::Home(HomeMessage::StartGame),
    );

    let elsewhere = row![
        tile(
            "Game History",
            "Review past games",
            Message::Home(HomeMessage::ViewHistory),
        ),
        tile(
            "Stats",
            "Win rates and matchups",
            Message::Home(HomeMessage::ViewStats),
        ),
        tile(
            "Players",
            "Add or rename people",
            Message::Home(HomeMessage::ManagePlayers),
        ),
    ]
    .spacing(style::GAP);

    container(
        column![
            header,
            container(start).height(Length::FillPortion(5)),
            container(elsewhere).height(Length::FillPortion(4)),
        ]
        .spacing(style::GAP)
        .padding(style::GAP)
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
