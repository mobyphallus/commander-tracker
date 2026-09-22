use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::model::{GameDetail, GameSummary};
use crate::style;

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
                button(
                    column![
                        text(winner).size(style::T_SUBHEAD),
                        text(format!(
                            "{} \u{00b7} {reason} \u{00b7} turn {} \u{00b7} {} players",
                            g.started_at.format("%Y-%m-%d %H:%M"),
                            g.ending_turn,
                            g.pod_size
                        ))
                        .size(style::T_CAPTION),
                    ]
                    .spacing(6),
                )
                .padding(20)
                .width(Length::Fill)
                .style(style::secondary)
                .on_press(Message::History(HistoryMessage::ViewGame(g.id)))
                .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(12);

    let header = container(
        row![
            text("Game History").size(style::T_TITLE),
            iced::widget::horizontal_space(),
            style::touch_button("Back", style::T_LABEL)
                .width(Length::Fixed(200.0))
                .style(style::secondary)
                .on_press(Message::GoHome),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(16)
    .width(Length::Fill)
    .style(style::header);

    container(
        column![header, scrollable(rows).height(Length::Fill)]
            .spacing(style::GAP)
            .padding(style::GAP),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn detail_view(detail: &GameDetail) -> Element<'_, Message> {
    let kill_lines: Vec<Element<Message>> = detail
        .kills
        .iter()
        .map(|k| {
            let killer = k.killer.clone().unwrap_or_else(|| "someone unknown".to_string());
            text(format!(
                "\u{2023} {} {} {}",
                k.victim,
                k.kind.past_tense(),
                killer
            ))
            .size(style::T_BODY)
            .into()
        })
        .collect();

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
                container(
                    column![
                        text(format!(
                            "{}{} - {}",
                            if s.won { "Winner \u{00b7} " } else { "" },
                            s.player_name,
                            s.commander_name
                        ))
                        .size(style::T_SUBHEAD),
                        text(format!("Life: {}   Poison: {}", s.final_life, s.final_poison))
                            .size(style::T_BODY),
                        text(if dmg.is_empty() {
                            "No commander damage taken".to_string()
                        } else {
                            format!("Damage taken: {dmg}")
                        })
                        .size(style::T_BODY),
                        text(match &s.out {
                            Some(out) => match &out.killer_name {
                                Some(killer) => format!(
                                    "{} on turn {}, to {}",
                                    out.cause.past_tense(),
                                    out.turn,
                                    killer
                                ),
                                None => format!(
                                    "{} on turn {}, nobody credited",
                                    out.cause.past_tense(),
                                    out.turn
                                ),
                            },
                            None if s.won => "Survived".to_string(),
                            None => "Still standing at the end".to_string(),
                        })
                        .size(style::T_BODY),
                    ]
                    .spacing(6),
                )
                .padding(18)
                .width(Length::Fill)
                .style(style::panel)
                .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(14);

    let header = container(
        row![
            text(format!(
                "Game on {}",
                detail.started_at.format("%Y-%m-%d %H:%M")
            ))
            .size(style::T_HEADING),
            iced::widget::horizontal_space(),
            style::touch_button("Back to list", style::T_LABEL)
                .width(Length::Fixed(240.0))
                .style(style::secondary)
                .on_press(Message::History(HistoryMessage::Back)),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(16)
    .width(Length::Fill)
    .style(style::header);

    container(
        column![
            header,
            text(format!(
                "Win condition: {} - ended on turn {}",
                detail.win_reason.map(|r| r.label()).unwrap_or("-"),
                detail.ending_turn
            ))
            .size(style::T_ACTION),
            column(kill_lines).spacing(6),
            scrollable(seat_rows).height(Length::Fill),
        ]
        .spacing(style::GAP)
        .padding(style::GAP),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
