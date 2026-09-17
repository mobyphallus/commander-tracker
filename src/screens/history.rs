use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::model::{GameDetail, GameSummary};

pub struct HistoryState {
    pub games: Vec<GameSummary>,
    pub selected: Option<GameDetail>,
}

impl HistoryState {
    pub fn load(conn: &Connection) -> Self {
        Self {
            games: db::list_games(conn).unwrap_or_default(),
            selected: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum HistoryMessage {
    ViewGame(i64),
    Back,
}

pub fn update(state: &mut HistoryState, conn: &Connection, message: HistoryMessage) {
    match message {
        HistoryMessage::ViewGame(id) => {
            state.selected = db::game_detail(conn, id).ok();
        }
        HistoryMessage::Back => {
            state.selected = None;
        }
    }
}

pub fn view(state: &HistoryState) -> Element<'_, Message> {
    if let Some(detail) = &state.selected {
        return detail_view(detail);
    }

    let rows = column(
        state
            .games
            .iter()
            .map(|g| {
                let winner = match (&g.winner_name, &g.winner_commander) {
                    (Some(n), Some(c)) => format!("{n} won with {c}"),
                    _ => "No winner recorded".to_string(),
                };
                let reason = g.win_reason.map(|r| r.label()).unwrap_or("-");
                button(column![
                    text(g.started_at.format("%Y-%m-%d %H:%M").to_string()).size(14),
                    text(format!("{winner} ({reason}) - {} players", g.pod_size)).size(16),
                ])
                .padding(12)
                .width(Length::Fill)
                .on_press(Message::History(HistoryMessage::ViewGame(g.id)))
                .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(8);

    container(
        column![
            row![
                text("Game History").size(30),
                button(text("Back").size(18))
                    .padding(10)
                    .on_press(Message::BackToSetup),
            ]
            .spacing(16)
            .align_y(iced::Alignment::Center),
            scrollable(rows).height(Length::Fill),
        ]
        .spacing(16)
        .padding(20),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn detail_view(detail: &GameDetail) -> Element<'_, Message> {
    let seat_rows = column(
        detail
            .seats
            .iter()
            .map(|s| {
                let dmg = s
                    .damage_taken
                    .iter()
                    .map(|(name, amt)| format!("{name}: {amt}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                column![
                    text(format!(
                        "{}{} - {}",
                        if s.won { "\u{1F451} " } else { "" },
                        s.player_name,
                        s.commander_name
                    ))
                    .size(18),
                    text(format!("Life: {}  Poison: {}", s.final_life, s.final_poison)).size(14),
                    text(if dmg.is_empty() {
                        "No commander damage taken".to_string()
                    } else {
                        format!("Damage taken: {dmg}")
                    })
                    .size(14),
                ]
                .spacing(4)
                .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(16);

    container(
        column![
            row![
                text(format!(
                    "Game on {}",
                    detail.started_at.format("%Y-%m-%d %H:%M")
                ))
                .size(24),
                button(text("Back to list").size(18))
                    .padding(10)
                    .on_press(Message::History(HistoryMessage::Back)),
            ]
            .spacing(16)
            .align_y(iced::Alignment::Center),
            text(format!(
                "Win condition: {}",
                detail.win_reason.map(|r| r.label()).unwrap_or("-")
            ))
            .size(18),
            scrollable(seat_rows).height(Length::Fill),
        ]
        .spacing(16)
        .padding(20),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
