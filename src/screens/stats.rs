use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::model::{MatchupStat, PlayerStat};
use crate::style;

pub struct StatsState {
    pub matchups: Vec<MatchupStat>,
    pub players: Vec<PlayerStat>,
}

impl StatsState {
    pub fn load(conn: &Connection) -> Self {
        Self {
            matchups: db::commander_matchup_stats(conn).unwrap_or_default(),
            players: db::player_stats(conn).unwrap_or_default(),
        }
    }
}

pub fn view(state: &StatsState) -> Element<'_, Message> {
    let player_rows = column(
        state
            .players
            .iter()
            .map(|p| {
                let pct = if p.games > 0 {
                    (p.wins as f64 / p.games as f64) * 100.0
                } else {
                    0.0
                };
                container(
                    row![
                        text(p.player_name.clone()).size(26).width(Length::Fill),
                        text(format!("{} games", p.games)).size(22).width(Length::Fixed(160.0)),
                        text(format!("{} wins", p.wins)).size(22).width(Length::Fixed(160.0)),
                        text(format!("{:.0}%", pct)).size(26).width(Length::Fixed(100.0)),
                    ]
                    .spacing(16)
                    .align_y(iced::Alignment::Center),
                )
                .padding([0, 20])
                .height(Length::Fixed(style::TOUCH_H))
                .center_y(Length::Fixed(style::TOUCH_H))
                .width(Length::Fill)
                .style(style::panel)
                .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(10);

    let matchup_rows = column(
        state
            .matchups
            .iter()
            .map(|m| {
                container(
                    row![
                        text(format!("{} vs {}", m.commander_a, m.commander_b))
                            .size(22)
                            .width(Length::Fill),
                        text(format!("{}-{}", m.a_wins, m.b_wins))
                            .size(24)
                            .width(Length::Fixed(120.0)),
                        text(format!("{} games", m.games)).size(20).width(Length::Fixed(160.0)),
                    ]
                    .spacing(16)
                    .align_y(iced::Alignment::Center),
                )
                .padding([0, 20])
                .height(Length::Fixed(style::TOUCH_H))
                .center_y(Length::Fixed(style::TOUCH_H))
                .width(Length::Fill)
                .style(style::panel)
                .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(10);

    let header = container(
        row![
            text("Stats").size(38),
            iced::widget::horizontal_space(),
            style::touch_button("Back", 20)
                .width(Length::Fixed(200.0))
                .style(button::secondary)
                .on_press(Message::GoHome),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(16)
    .width(Length::Fill)
    .style(style::header);

    container(
        column![
            header,
            text("Player win rates").size(28),
            scrollable(player_rows).height(Length::FillPortion(2)),
            text("Commander matchups").size(28),
            scrollable(matchup_rows).height(Length::FillPortion(3)),
        ]
        .spacing(style::GAP)
        .padding(style::GAP),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
