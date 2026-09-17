use std::collections::HashMap;

use chrono::Utc;
use iced::widget::{button, column, container, image, row, scrollable, stack, text};
use iced::{Color, ContentFit, Element, Length};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::model::{FinishedGame, KillEvent, Seat, LETHAL_COMMANDER_DAMAGE, LETHAL_POISON, WinReason};
use crate::style;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeatTab {
    Life,
    Damage,
    Poison,
}

pub struct GameState {
    pub seats: Vec<Seat>,
    pub started_at: chrono::DateTime<Utc>,
    pub active_seat: usize,
    pub turn_seconds: u64,
    pub game_seconds: u64,
    pub paused: bool,
    pub seat_tab: Vec<SeatTab>,
    pub kills: Vec<KillEvent>,
    pub marking_kill_for: Option<usize>,
    /// A seat that just hit 0 life and hasn't been asked "are they out?" yet
    /// (or was asked and said no, until they drop to 0 again).
    pub pending_life_check: Option<usize>,
    pub zero_life_prompt_dismissed: Vec<bool>,
    pub pending_winner: Option<usize>,
    pub pending_reason: Option<WinReason>,
}

impl GameState {
    pub fn new(seats: Vec<Seat>) -> Self {
        let seat_tab = vec![SeatTab::Life; seats.len()];
        let zero_life_prompt_dismissed = vec![false; seats.len()];
        Self {
            seats,
            started_at: Utc::now(),
            active_seat: 0,
            turn_seconds: 0,
            game_seconds: 0,
            paused: false,
            seat_tab,
            kills: Vec::new(),
            marking_kill_for: None,
            pending_life_check: None,
            zero_life_prompt_dismissed,
            pending_winner: None,
            pending_reason: None,
        }
    }

    /// Auto-eliminates on lethal poison or lethal commander damage from a
    /// single opponent, per the rules - no confirmation needed for these.
    fn check_hard_elimination(&mut self, seat: usize) {
        let s = &mut self.seats[seat];
        if s.eliminated {
            return;
        }
        let lethal_damage = s.commander_damage_taken.values().any(|&d| d >= LETHAL_COMMANDER_DAMAGE);
        if s.poison >= LETHAL_POISON || lethal_damage {
            s.eliminated = true;
        }
    }

    /// Life 0 or below isn't automatically a loss - some cards keep a player
    /// alive there - so ask instead of assuming.
    fn check_zero_life(&mut self, seat: usize) {
        let s = &self.seats[seat];
        if s.eliminated {
            return;
        }
        if s.life > 0 {
            self.zero_life_prompt_dismissed[seat] = false;
            return;
        }
        if !self.zero_life_prompt_dismissed[seat] && self.pending_life_check.is_none() {
            self.pending_life_check = Some(seat);
        }
    }
}

#[derive(Debug, Clone)]
pub enum GameMessage {
    Tick,
    TogglePause,
    NextTurn,
    SwitchTab(usize, SeatTab),
    LifeDelta(usize, i32),
    PoisonDelta(usize, i32),
    CommanderDamageDelta(usize, usize, i32),
    ToggleEliminated(usize),
    AnswerZeroLifeCheck(bool),
    MarkKilled(usize),
    CancelMarkKilled,
    ConfirmKill(usize, Option<usize>),
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
            let n = state.seats.len();
            let mut next = state.active_seat;
            for _ in 0..n {
                next = (next + 1) % n;
                if !state.seats[next].eliminated {
                    break;
                }
            }
            state.active_seat = next;
            state.turn_seconds = 0;
            (iced::Task::none(), None)
        }
        GameMessage::SwitchTab(seat, tab) => {
            if let Some(t) = state.seat_tab.get_mut(seat) {
                *t = tab;
            }
            (iced::Task::none(), None)
        }
        GameMessage::LifeDelta(seat, delta) => {
            if let Some(s) = state.seats.get_mut(seat) {
                s.life += delta;
            }
            state.check_zero_life(seat);
            (iced::Task::none(), None)
        }
        GameMessage::PoisonDelta(seat, delta) => {
            if let Some(s) = state.seats.get_mut(seat) {
                s.poison = (s.poison + delta).max(0);
            }
            state.check_hard_elimination(seat);
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
            state.check_hard_elimination(target);
            state.check_zero_life(target);
            (iced::Task::none(), None)
        }
        GameMessage::ToggleEliminated(seat) => {
            if let Some(s) = state.seats.get_mut(seat) {
                s.eliminated = !s.eliminated;
            }
            (iced::Task::none(), None)
        }
        GameMessage::AnswerZeroLifeCheck(out) => {
            if let Some(seat) = state.pending_life_check.take() {
                if out {
                    state.seats[seat].eliminated = true;
                } else {
                    state.zero_life_prompt_dismissed[seat] = true;
                }
            }
            (iced::Task::none(), None)
        }
        GameMessage::MarkKilled(seat) => {
            state.marking_kill_for = Some(seat);
            (iced::Task::none(), None)
        }
        GameMessage::CancelMarkKilled => {
            state.marking_kill_for = None;
            (iced::Task::none(), None)
        }
        GameMessage::ConfirmKill(victim, killer) => {
            state.kills.push(KillEvent {
                victim_seat: victim,
                killer_seat: killer,
            });
            state.marking_kill_for = None;
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
                    kills: state.kills.clone(),
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
    if let Some(seat) = state.pending_life_check {
        return zero_life_check_view(state, seat);
    }
    if let Some(winner) = state.pending_winner {
        return declare_winner_view(state, winner);
    }
    if let Some(victim) = state.marking_kill_for {
        return mark_kill_view(state, victim);
    }

    let top_bar = container(
        row![
            text(format!(
                "{}'s turn - {}",
                state.seats[state.active_seat].player.name,
                format_duration(state.turn_seconds)
            ))
            .size(22),
            text(format!("Game time {}", format_duration(state.game_seconds))).size(16),
            iced::widget::horizontal_space(),
            button(text(if state.paused { "Resume" } else { "Pause" }).size(16))
                .padding(10)
                .on_press(Message::Game(GameMessage::TogglePause)),
            button(text("Next Turn").size(16))
                .padding(10)
                .style(button::primary)
                .on_press(Message::Game(GameMessage::NextTurn)),
            button(text("Abandon Game").size(16))
                .padding(10)
                .style(button::danger)
                .on_press(Message::Game(GameMessage::AbandonGame)),
        ]
        .spacing(14)
        .align_y(iced::Alignment::Center),
    )
    .padding(14)
    .width(Length::Fill)
    .style(style::header);

    let seat_count = state.seats.len();
    let cols = grid_columns(seat_count);
    let mut rows_el: Vec<Element<Message>> = Vec::new();
    let mut i = 0;
    while i < seat_count {
        let end = (i + cols).min(seat_count);
        let row_panels: Vec<Element<Message>> = (i..end)
            .map(|idx| seat_panel(idx, state, image_cache))
            .collect();
        rows_el.push(
            row(row_panels)
                .spacing(12)
                .height(Length::FillPortion(1))
                .into(),
        );
        i = end;
    }
    let board = column(rows_el).spacing(12).height(Length::Fill);

    container(column![top_bar, board].spacing(12).padding(16))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn tab_button<'a>(label: &'a str, tab: SeatTab, seat: usize, current: SeatTab) -> Element<'a, Message> {
    let selected = tab == current;
    button(text(label).size(13))
        .padding(6)
        .style(if selected { button::primary } else { button::secondary })
        .on_press(Message::Game(GameMessage::SwitchTab(seat, tab)))
        .into()
}

fn life_tab(index: usize, seat: &Seat) -> Element<'_, Message> {
    row![
        button(text("-5").size(16)).padding(8).on_press(Message::Game(GameMessage::LifeDelta(index, -5))),
        button(text("-1").size(16)).padding(8).on_press(Message::Game(GameMessage::LifeDelta(index, -1))),
        text(seat.life.to_string()).size(34).width(Length::Fixed(64.0)).align_x(iced::Alignment::Center),
        button(text("+1").size(16)).padding(8).on_press(Message::Game(GameMessage::LifeDelta(index, 1))),
        button(text("+5").size(16)).padding(8).on_press(Message::Game(GameMessage::LifeDelta(index, 5))),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .into()
}

fn poison_tab(index: usize, seat: &Seat) -> Element<'_, Message> {
    column![
        row![
            button(text("-1").size(16)).padding(8).on_press(Message::Game(GameMessage::PoisonDelta(index, -1))),
            text(seat.poison.to_string()).size(34).width(Length::Fixed(50.0)).align_x(iced::Alignment::Center),
            button(text("+1").size(16)).padding(8).on_press(Message::Game(GameMessage::PoisonDelta(index, 1))),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
        text(format!("Lethal at {LETHAL_POISON} poison")).size(11),
    ]
    .spacing(4)
    .into()
}

fn damage_tab<'a>(index: usize, state: &'a GameState) -> Element<'a, Message> {
    let seat = &state.seats[index];
    let rows: Vec<Element<Message>> = state
        .seats
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != index)
        .map(|(j, other)| {
            let amount = seat.damage_from(j);
            row![
                text(format!("{} ({})", other.commander.name, other.player.name))
                    .size(13)
                    .width(Length::Fill),
                button(text("-").size(16))
                    .padding(6)
                    .on_press(Message::Game(GameMessage::CommanderDamageDelta(index, j, -1))),
                text(amount.to_string()).size(18).width(Length::Fixed(30.0)).align_x(iced::Alignment::Center),
                button(text("+").size(16))
                    .padding(6)
                    .on_press(Message::Game(GameMessage::CommanderDamageDelta(index, j, 1))),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into()
        })
        .collect();

    column![
        text(format!("Lethal at {LETHAL_COMMANDER_DAMAGE} from one commander")).size(11),
        column(rows).spacing(6),
    ]
    .spacing(6)
    .into()
}

fn seat_panel<'a>(
    index: usize,
    state: &'a GameState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let seat = &state.seats[index];
    let is_active = state.active_seat == index;
    let lethal = seat.is_lethal();
    let out = seat.eliminated || lethal;

    if out {
        return eliminated_tile(index, seat, image_cache);
    }

    let art: Element<Message> = match seat.commander.portrait_url().and_then(|u| image_cache.get(u)) {
        Some(handle) => image(handle.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(text(seat.commander.name.clone()).size(16))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into(),
    };

    let caption = container(
        column![
            text(seat.player.name.clone()).size(20),
            text(seat.commander.name.clone()).size(13),
        ]
        .spacing(2),
    )
    .padding(10)
    .width(Length::Fill)
    .style(|_theme: &iced::Theme| container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
        text_color: Some(Color::WHITE),
        ..container::Style::default()
    });

    let portrait_area = container(stack![
        art,
        container(caption)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(iced::alignment::Vertical::Bottom),
    ])
    .width(Length::Fill)
    .height(Length::FillPortion(3));

    let tabs = row![
        tab_button("Life", SeatTab::Life, index, state.seat_tab[index]),
        tab_button("Damage", SeatTab::Damage, index, state.seat_tab[index]),
        tab_button("Poison", SeatTab::Poison, index, state.seat_tab[index]),
    ]
    .spacing(6);

    let body = match state.seat_tab[index] {
        SeatTab::Life => life_tab(index, seat),
        SeatTab::Damage => damage_tab(index, state),
        SeatTab::Poison => poison_tab(index, seat),
    };

    let controls = column![
        tabs,
        body,
        row![
            button(text(if seat.eliminated { "Back In" } else { "Mark Out" }).size(13))
                .padding(7)
                .style(if seat.eliminated { button::secondary } else { button::danger })
                .on_press(Message::Game(GameMessage::ToggleEliminated(index))),
            button(text("Commander Killed").size(13))
                .padding(7)
                .on_press(Message::Game(GameMessage::MarkKilled(index))),
        ]
        .spacing(6),
        button(text("Declare Winner").size(15))
            .padding(9)
            .style(button::success)
            .on_press(Message::Game(GameMessage::StartDeclareWinner(index))),
    ]
    .spacing(7)
    .height(Length::FillPortion(2));

    let card = column![portrait_area, controls].spacing(7).height(Length::Fill);

    let style_fn: fn(&iced::Theme) -> container::Style = if is_active {
        style::panel_active
    } else {
        style::panel
    };

    container(card)
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style_fn)
        .into()
}

/// A fully blacked-out tile for a seat that's out of the game, with just
/// enough left to see who it was and undo the call if it was a mistake.
fn eliminated_tile<'a>(
    index: usize,
    seat: &'a Seat,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let art: Element<Message> = match seat.commander.portrait_url().and_then(|u| image_cache.get(u)) {
        Some(handle) => image(handle.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(text("")).width(Length::Fill).height(Length::Fill).into(),
    };

    let scrim = container(
        column![
            text("ELIMINATED").size(24),
            text(seat.player.name.clone()).size(18),
            text(seat.commander.name.clone()).size(13),
            text(format!("Final: {} life, {} poison", seat.life, seat.poison)).size(13),
            button(text("Back In").size(14))
                .padding(10)
                .style(button::secondary)
                .on_press(Message::Game(GameMessage::ToggleEliminated(index))),
        ]
        .spacing(8)
        .align_x(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .style(|_theme: &iced::Theme| container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.85).into()),
        text_color: Some(Color::WHITE),
        ..container::Style::default()
    });

    container(stack![art, scrim])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::panel)
        .into()
}

fn zero_life_check_view(state: &GameState, seat: usize) -> Element<'_, Message> {
    let s = &state.seats[seat];
    container(
        column![
            text(format!("{} is at {} life. Are they out?", s.player.name, s.life)).size(26),
            text(
                "Some effects keep a player from losing at 0 or below - say no to keep them in."
            )
            .size(14),
            row![
                button(text("Yes, they're out").size(18))
                    .padding(16)
                    .style(button::danger)
                    .on_press(Message::Game(GameMessage::AnswerZeroLifeCheck(true))),
                button(text("No, keep playing").size(18))
                    .padding(16)
                    .style(button::secondary)
                    .on_press(Message::Game(GameMessage::AnswerZeroLifeCheck(false))),
            ]
            .spacing(12),
        ]
        .spacing(18)
        .padding(24),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn mark_kill_view(state: &GameState, victim: usize) -> Element<'_, Message> {
    let victim_seat = &state.seats[victim];

    let mut options: Vec<Element<Message>> = state
        .seats
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != victim)
        .map(|(j, other)| {
            button(
                text(format!("{} ({})", other.player.name, other.commander.name)).size(18),
            )
            .padding(14)
            .width(Length::Fill)
            .on_press(Message::Game(GameMessage::ConfirmKill(victim, Some(j))))
            .into()
        })
        .collect();
    options.push(
        button(text("Board wipe / unknown").size(18))
            .padding(14)
            .width(Length::Fill)
            .on_press(Message::Game(GameMessage::ConfirmKill(victim, None)))
            .into(),
    );

    container(
        column![
            text(format!(
                "{}'s {} was killed - by whom?",
                victim_seat.player.name, victim_seat.commander.name
            ))
            .size(24),
            scrollable(column(options).spacing(10)).height(Length::Fixed(320.0)),
            button(text("Cancel").size(16))
                .padding(12)
                .on_press(Message::Game(GameMessage::CancelMarkKilled)),
        ]
        .spacing(16)
        .padding(24),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn declare_winner_view(state: &GameState, winner: usize) -> Element<'_, Message> {
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
