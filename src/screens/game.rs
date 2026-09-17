use std::collections::HashMap;

use chrono::Utc;
use iced::widget::{button, column, container, image, row, scrollable, text};
use iced::{Element, Length};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::model::{FinishedGame, Seat, WinReason};
use crate::screens::setup::Layout;

pub struct GameState {
    pub seats: Vec<Seat>,
    pub layout: Layout,
    pub started_at: chrono::DateTime<Utc>,
    pub active_seat: usize,
    pub turn_seconds: u64,
    pub game_seconds: u64,
    pub paused: bool,
    pub pending_winner: Option<usize>,
    pub pending_reason: Option<WinReason>,
}

impl GameState {
    pub fn new(seats: Vec<Seat>, layout: Layout) -> Self {
        Self {
            seats,
            layout,
            started_at: Utc::now(),
            active_seat: 0,
            turn_seconds: 0,
            game_seconds: 0,
            paused: false,
            pending_winner: None,
            pending_reason: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum GameMessage {
    Tick,
    TogglePause,
    NextTurn,
    LifeDelta(usize, i32),
    PoisonDelta(usize, i32),
    CommanderDamageDelta(usize, usize, i32),
    StartDeclareWinner(usize),
    CancelDeclareWinner,
    PickWinReason(WinReason),
    ConfirmEndGame,
    AbandonGame,
}

pub enum Action {
    Finished,
    Abandoned,
}

pub fn update(
    state: &mut GameState,
    conn: &mut Connection,
    message: GameMessage,
) -> (iced::Task<Message>, Option<Action>) {
    match message {
        GameMessage::Tick => {
            if !state.paused {
                state.turn_seconds += 1;
                state.game_seconds += 1;
            }
            (iced::Task::none(), None)
        }
        GameMessage::TogglePause => {
            state.paused = !state.paused;
            (iced::Task::none(), None)
        }
        GameMessage::NextTurn => {
            state.active_seat = (state.active_seat + 1) % state.seats.len();
            state.turn_seconds = 0;
            (iced::Task::none(), None)
        }
        GameMessage::LifeDelta(seat, delta) => {
            if let Some(s) = state.seats.get_mut(seat) {
                s.life += delta;
            }
            (iced::Task::none(), None)
        }
        GameMessage::PoisonDelta(seat, delta) => {
            if let Some(s) = state.seats.get_mut(seat) {
                s.poison = (s.poison + delta).max(0);
            }
            (iced::Task::none(), None)
        }
        GameMessage::CommanderDamageDelta(target, source, delta) => {
            if let Some(seat) = state.seats.get_mut(target) {
                let entry = seat.commander_damage_taken.entry(source).or_insert(0);
                let before = *entry;
                let after = (before + delta).max(0);
                *entry = after;
                seat.life -= after - before;
            }
            (iced::Task::none(), None)
        }
        GameMessage::StartDeclareWinner(seat) => {
            state.pending_winner = Some(seat);
            state.pending_reason = None;
            (iced::Task::none(), None)
        }
        GameMessage::CancelDeclareWinner => {
            state.pending_winner = None;
            state.pending_reason = None;
            (iced::Task::none(), None)
        }
        GameMessage::PickWinReason(reason) => {
            state.pending_reason = Some(reason);
            (iced::Task::none(), None)
        }
        GameMessage::ConfirmEndGame => {
            if let (Some(winner), Some(reason)) = (state.pending_winner, state.pending_reason) {
                let finished = FinishedGame {
                    seats: state.seats.clone(),
                    winner_seat: Some(winner),
                    win_reason: Some(reason),
                    started_at: state.started_at,
                    ended_at: Utc::now(),
                };
                match db::record_game(conn, &finished) {
                    Ok(()) => (iced::Task::none(), Some(Action::Finished)),
                    Err(_) => (iced::Task::none(), None),
                }
            } else {
                (iced::Task::none(), None)
            }
        }
        GameMessage::AbandonGame => (iced::Task::none(), Some(Action::Abandoned)),
    }
}

fn format_duration(total_seconds: u64) -> String {
    format!("{:02}:{:02}", total_seconds / 60, total_seconds % 60)
}

fn grid_columns(n: usize) -> usize {
    match n {
        0 | 1 | 2 => 2,
        3 => 3,
        4 => 2,
        5 | 6 => 3,
        _ => 4,
    }
}

pub fn view<'a>(
    state: &'a GameState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    if let Some(winner) = state.pending_winner {
        return declare_winner_view(state, winner);
    }

    let top_bar = row![
        text(format!(
            "{}'s turn - {}",
            state.seats[state.active_seat].player.name,
            format_duration(state.turn_seconds)
        ))
        .size(22),
        text(format!("Game time {}", format_duration(state.game_seconds))).size(18),
        button(text(if state.paused { "Resume" } else { "Pause" }).size(18))
            .padding(10)
            .on_press(Message::Game(GameMessage::TogglePause)),
        button(text("Next Turn").size(18))
            .padding(10)
            .on_press(Message::Game(GameMessage::NextTurn)),
        button(text("Abandon Game").size(18))
            .padding(10)
            .style(button::danger)
            .on_press(Message::Game(GameMessage::AbandonGame)),
    ]
    .spacing(16)
    .align_y(iced::Alignment::Center);

    let seat_count = state.seats.len();
    let board: Element<Message> = match state.layout {
        Layout::List => scrollable(
            column(
                (0..seat_count)
                    .map(|i| seat_panel(i, state, image_cache))
                    .collect::<Vec<Element<Message>>>(),
            )
            .spacing(12),
        )
        .height(Length::Fill)
        .into(),
        Layout::Grid => {
            let cols = grid_columns(seat_count);
            let mut rows_el = Vec::new();
            let mut i = 0;
            while i < seat_count {
                let end = (i + cols).min(seat_count);
                let row_panels: Vec<Element<Message>> = (i..end)
                    .map(|idx| seat_panel(idx, state, image_cache))
                    .collect();
                rows_el.push(row(row_panels).spacing(12).into());
                i = end;
            }
            scrollable(column(rows_el).spacing(12)).height(Length::Fill).into()
        }
    };

    container(column![top_bar, board].spacing(16).padding(16))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn seat_panel<'a>(
    index: usize,
    state: &'a GameState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let seat = &state.seats[index];
    let is_active = state.active_seat == index;

    let portrait: Element<Message> = match image_cache.get(&seat.commander.scryfall_id) {
        Some(handle) => image(handle.clone())
            .width(Length::Fill)
            .height(Length::Fixed(100.0))
            .into(),
        None => text(&seat.commander.name).size(16).into(),
    };

    let mut damage_rows = Vec::new();
    for (j, other) in state.seats.iter().enumerate() {
        if j == index {
            continue;
        }
        let amount = seat.damage_from(j);
        damage_rows.push(
            row![
                text(format!("{}", other.commander.name)).size(14).width(Length::Fill),
                button(text("-").size(16))
                    .padding(6)
                    .on_press(Message::Game(GameMessage::CommanderDamageDelta(index, j, -1))),
                text(amount.to_string()).size(16).width(Length::Fixed(30.0)),
                button(text("+").size(16))
                    .padding(6)
                    .on_press(Message::Game(GameMessage::CommanderDamageDelta(index, j, 1))),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into(),
        );
    }

    let card = column![
        portrait,
        text(seat.player.name.clone()).size(20),
        text(seat.commander.name.clone()).size(14),
        row![
            button(text("-5").size(18)).padding(8).on_press(Message::Game(GameMessage::LifeDelta(index, -5))),
            button(text("-1").size(18)).padding(8).on_press(Message::Game(GameMessage::LifeDelta(index, -1))),
            text(seat.life.to_string()).size(30).width(Length::Fixed(60.0)).align_x(iced::Alignment::Center),
            button(text("+1").size(18)).padding(8).on_press(Message::Game(GameMessage::LifeDelta(index, 1))),
            button(text("+5").size(18)).padding(8).on_press(Message::Game(GameMessage::LifeDelta(index, 5))),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
        row![
            text("Poison").size(14),
            button(text("-").size(16)).padding(6).on_press(Message::Game(GameMessage::PoisonDelta(index, -1))),
            text(seat.poison.to_string()).size(16).width(Length::Fixed(24.0)),
            button(text("+").size(16)).padding(6).on_press(Message::Game(GameMessage::PoisonDelta(index, 1))),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
        text("Commander damage taken:").size(13),
        column(damage_rows).spacing(4),
        button(text("Declare Winner").size(16))
            .padding(8)
            .style(button::success)
            .on_press(Message::Game(GameMessage::StartDeclareWinner(index))),
    ]
    .spacing(6);

    container(card)
        .padding(12)
        .width(Length::Fill)
        .style(move |theme: &iced::Theme| {
            let mut style = container::rounded_box(theme);
            if is_active {
                style.border.color = theme.palette().primary;
                style.border.width = 3.0;
            }
            style
        })
        .into()
}

fn declare_winner_view(state: &GameState, winner: usize) -> Element<Message> {
    let winner_seat = &state.seats[winner];
    let reasons = column(
        WinReason::ALL
            .iter()
            .map(|r| {
                let selected = state.pending_reason == Some(*r);
                button(text(r.label()).size(18))
                    .padding(12)
                    .width(Length::Fill)
                    .style(if selected {
                        button::primary
                    } else {
                        button::secondary
                    })
                    .on_press(Message::Game(GameMessage::PickWinReason(*r)))
                    .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(8);

    let mut confirm = button(text("Confirm & Save Game").size(20)).padding(16);
    if state.pending_reason.is_some() {
        confirm = confirm
            .style(button::success)
            .on_press(Message::Game(GameMessage::ConfirmEndGame));
    }

    container(
        column![
            text(format!(
                "{} wins with {}!",
                winner_seat.player.name, winner_seat.commander.name
            ))
            .size(26),
            text("How did they win?").size(18),
            scrollable(reasons).height(Length::Fixed(320.0)),
            row![
                confirm,
                button(text("Cancel").size(18))
                    .padding(16)
                    .on_press(Message::Game(GameMessage::CancelDeclareWinner)),
            ]
            .spacing(12),
        ]
        .spacing(16)
        .padding(24),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
