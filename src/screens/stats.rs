use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::model::{MatchupStat, PlayerStat};

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
                row![
                    text(p.player_name.clone()).size(18).width(Length::Fixed(160.0)),
                    text(format!("{} games", p.games)).size(16).width(Length::Fixed(100.0)),
                    text(format!("{} wins", p.wins)).size(16).width(Length::Fixed(100.0)),
                    text(format!("{:.0}%", pct)).size(16),
                ]
                .spacing(12)
                .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(8);

    let matchup_rows = column(
        state
            .matchups
            .iter()
            .map(|m| {
                row![
                    text(format!("{} vs {}", m.commander_a, m.commander_b))
                        .size(16)
                        .width(Length::Fixed(300.0)),
                    text(format!("{}-{}", m.a_wins, m.b_wins))
                        .size(16)
                        .width(Length::Fixed(80.0)),
                    text(format!("{} games", m.games)).size(16),
                ]
                .spacing(12)
                .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(8);

    container(
        column![
            row![
                text("Stats").size(30),
                button(text("Back").size(18))
                    .padding(10)
                    .on_press(Message::GoHome),
            ]
            .spacing(16)
            .align_y(iced::Alignment::Center),
            text("Player win rates").size(22),
            scrollable(player_rows).height(Length::Fixed(240.0)),
            text("Commander matchups").size(22),
            scrollable(matchup_rows).height(Length::Fill),
        ]
        .spacing(16)
        .padding(20),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
