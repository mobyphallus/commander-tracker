use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::Utc;
use iced::widget::{button, column, container, image, mouse_area, row, scrollable, stack, text};
use iced::{Border, Color, ContentFit, Element, Length};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::layout::{self, TableLayout};
use crate::model::{FinishedGame, KillEvent, Seat, LETHAL_COMMANDER_DAMAGE, LETHAL_POISON, WinReason};
use crate::style;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeatTab {
    Life,
    Poison,
}

pub struct GameState {
    pub seats: Vec<Seat>,
    pub table_layout: TableLayout,
    pub started_at: chrono::DateTime<Utc>,
    pub active_seat: usize,
    /// The 1-indexed count of individual turns taken so far this game,
    /// including the one in progress.
    pub turn_number: u32,
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
    /// A counter zone currently held down; holding it applies +/-10 every
    /// couple of seconds instead of the normal +/-1 on release, and an
    /// upward drag past the swipe threshold turns it into opening the
    /// action menu instead.
    pub press_hold: Option<PressHold>,
    /// Which seat's action menu (Commander Damage / Poison / Mark Out /
    /// Declare Winner) is currently open, if any.
    pub action_menu_for: Option<usize>,
    /// While set, every OTHER seat's tile swaps its life display for a
    /// quick +/- on the commander damage *that seat* has dealt to this one.
    pub damage_focus: Option<usize>,
}

/// Any counter that can be adjusted with the hold-to-repeat left/right zones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterTarget {
    Life(usize),
    Poison(usize),
    /// (seat whose damage total this is, seat whose commander dealt it)
    Damage(usize, usize),
}

#[derive(Debug, Clone, Copy)]
pub struct PressHold {
    pub seat: usize,
    pub target: CounterTarget,
    pub sign: i32,
    pub started_at: Instant,
    /// None until the first +/-10 fires; then tracks the last time it fired
    /// so continuing to hold repeats it every `HOLD_THRESHOLD`.
    pub last_fired_at: Option<Instant>,
    /// Since the +/- zones now cover the whole tile, an upward drag past
    /// the swipe threshold cancels the tap/hold and opens the action menu
    /// instead - tracked here rather than as a separate gesture.
    pub swipe_baseline_y: Option<f32>,
    pub became_swipe: bool,
}

/// Pixels of upward swipe needed to open a seat's action menu.
const SWIPE_OPEN_THRESHOLD: f32 = 55.0;
/// How long a zone must be held before it jumps by +/-10, and how often it
/// repeats while still held.
const HOLD_THRESHOLD: Duration = Duration::from_secs(2);

impl GameState {
    pub fn new(seats: Vec<Seat>, table_layout: TableLayout) -> Self {
        let seat_tab = vec![SeatTab::Life; seats.len()];
        let zero_life_prompt_dismissed = vec![false; seats.len()];
        Self {
            seats,
            table_layout,
            started_at: Utc::now(),
            active_seat: 0,
            turn_number: 1,
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
            press_hold: None,
            action_menu_for: None,
            damage_focus: None,
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
    CounterPressStart(usize, CounterTarget, i32),
    CounterPressMove(CounterTarget, i32, f32),
    CounterPressEnd(CounterTarget, i32),
    HoldTick,
    CloseActionMenu,
    StartDamageFocus(usize),
    EndDamageFocus,
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
            state.turn_number += 1;
            (iced::Task::none(), None)
        }
        GameMessage::SwitchTab(seat, tab) => {
            if let Some(t) = state.seat_tab.get_mut(seat) {
                *t = tab;
            }
            state.action_menu_for = None;
            (iced::Task::none(), None)
        }
        GameMessage::CounterPressStart(seat, target, sign) => {
            state.press_hold = Some(PressHold {
                seat,
                target,
                sign,
                started_at: Instant::now(),
                last_fired_at: None,
                swipe_baseline_y: None,
                became_swipe: false,
            });
            (iced::Task::none(), None)
        }
        GameMessage::CounterPressMove(target, sign, y) => {
            let mut open_seat = None;
            if let Some(hold) = &mut state.press_hold {
                if hold.target == target && hold.sign == sign && !hold.became_swipe {
                    match hold.swipe_baseline_y {
                        None => hold.swipe_baseline_y = Some(y),
                        Some(baseline) => {
                            if baseline - y >= SWIPE_OPEN_THRESHOLD {
                                hold.became_swipe = true;
                                open_seat = Some(hold.seat);
                            }
                        }
                    }
                }
            }
            if let Some(seat) = open_seat {
                state.action_menu_for = Some(seat);
                state.press_hold = None;
            }
            (iced::Task::none(), None)
        }
        GameMessage::CounterPressEnd(target, sign) => {
            if let Some(hold) = state.press_hold.take() {
                if hold.target == target
                    && hold.sign == sign
                    && hold.last_fired_at.is_none()
                    && !hold.became_swipe
                {
                    apply_counter_delta(state, target, sign);
                }
            }
            (iced::Task::none(), None)
        }
        GameMessage::HoldTick => {
            let mut fire = None;
            if let Some(hold) = &mut state.press_hold {
                let now = Instant::now();
                let should_fire = match hold.last_fired_at {
                    None => now.duration_since(hold.started_at) >= HOLD_THRESHOLD,
                    Some(last) => now.duration_since(last) >= HOLD_THRESHOLD,
                };
                if should_fire {
                    hold.last_fired_at = Some(now);
                    fire = Some((hold.target, hold.sign));
                }
            }
            if let Some((target, sign)) = fire {
                apply_counter_delta(state, target, sign * 10);
            }
            (iced::Task::none(), None)
        }
        GameMessage::CloseActionMenu => {
            state.action_menu_for = None;
            (iced::Task::none(), None)
        }
        GameMessage::StartDamageFocus(seat) => {
            state.damage_focus = Some(seat);
            state.action_menu_for = None;
            (iced::Task::none(), None)
        }
        GameMessage::EndDamageFocus => {
            state.damage_focus = None;
            (iced::Task::none(), None)
        }
        GameMessage::ToggleEliminated(seat) => {
            if let Some(s) = state.seats.get_mut(seat) {
                s.eliminated = !s.eliminated;
            }
            state.action_menu_for = None;
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
            state.action_menu_for = None;
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
                    ending_turn: state.turn_number,
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

fn apply_life_delta(state: &mut GameState, seat: usize, delta: i32) {
    if let Some(s) = state.seats.get_mut(seat) {
        s.life += delta;
    }
    state.check_zero_life(seat);
}

fn apply_poison_delta(state: &mut GameState, seat: usize, delta: i32) {
    if let Some(s) = state.seats.get_mut(seat) {
        s.poison = (s.poison + delta).max(0);
    }
    state.check_hard_elimination(seat);
}

fn apply_damage_delta(state: &mut GameState, target: usize, source: usize, delta: i32) {
    if let Some(seat) = state.seats.get_mut(target) {
        let entry = seat.commander_damage_taken.entry(source).or_insert(0);
        let before = *entry;
        let after = (before + delta).max(0);
        *entry = after;
        seat.life -= after - before;
    }
    state.check_hard_elimination(target);
    state.check_zero_life(target);
}

fn apply_counter_delta(state: &mut GameState, target: CounterTarget, delta: i32) {
    match target {
        CounterTarget::Life(seat) => apply_life_delta(state, seat, delta),
        CounterTarget::Poison(seat) => apply_poison_delta(state, seat, delta),
        CounterTarget::Damage(target_seat, source_seat) => {
            apply_damage_delta(state, target_seat, source_seat, delta)
        }
    }
}

fn format_duration(total_seconds: u64) -> String {
    format!("{:02}:{:02}", total_seconds / 60, total_seconds % 60)
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
                "Turn {} - {}'s turn - {}",
                state.turn_number,
                state.seats[state.active_seat].player.name,
                format_duration(state.turn_seconds)
            ))
            .size(18),
            iced::widget::horizontal_space(),
            button(text(if state.paused { "Resume" } else { "Pause" }).size(18))
                .padding(16)
                .on_press(Message::Game(GameMessage::TogglePause)),
            button(text("Next Turn").size(18))
                .padding(16)
                .style(button::primary)
                .on_press(Message::Game(GameMessage::NextTurn)),
            button(text("Abandon Game").size(18))
                .padding(16)
                .style(button::danger)
                .on_press(Message::Game(GameMessage::AbandonGame)),
        ]
        .spacing(14)
        .align_y(iced::Alignment::Center),
    )
    .padding(14)
    .width(Length::Fill)
    .style(style::header);

    let damage_focus_banner: Option<Element<Message>> = state.damage_focus.map(|focus| {
        container(
            row![
                text(format!(
                    "Logging commander damage dealt to {} - tap an opponent's tile to add it",
                    state.seats[focus].player.name
                ))
                .size(15),
                iced::widget::horizontal_space(),
                button(text("Done").size(16))
                    .padding(10)
                    .style(button::primary)
                    .on_press(Message::Game(GameMessage::EndDamageFocus)),
            ]
            .spacing(14)
            .align_y(iced::Alignment::Center),
        )
        .padding(12)
        .width(Length::Fill)
        .style(style::panel_active)
        .into()
    });

    // A frosted-glass-style panel floating dead center of the seat grid, so
    // it sits in the middle of the table regardless of pod size.
    let timer_panel = container(
        column![
            text(format_duration(state.game_seconds)).size(30),
            text("GAME TIME").size(11),
        ]
        .spacing(2)
        .align_x(iced::Alignment::Center),
    )
    .padding([10.0, 26.0])
    .style(|_theme: &iced::Theme| container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
        text_color: Some(Color::WHITE),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.28),
            width: 1.0,
            radius: 20.0.into(),
        },
        ..container::Style::default()
    });

    let board = layout::render_table(&state.table_layout, |idx| seat_panel(idx, state, image_cache));

    let board_with_timer = stack![
        board,
        container(timer_panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    ];

    let mut layout = column![top_bar].spacing(12);
    if let Some(banner) = damage_focus_banner {
        layout = layout.push(banner);
    }
    layout = layout.push(board_with_timer);

    container(layout.padding(16))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// A big number with the whole left half acting as a "-" zone and the whole
/// right half as a "+" zone, so you don't have to hit a small button. Tap
/// for +/-1; hold for +/-10, repeating every couple of seconds while held.
/// A huge centered number over full-tile-height, mostly-invisible left/right
/// tap zones - the left half subtracts, the right half adds. Tap for +/-1;
/// hold for +/-10, repeating while held; drag up past the threshold cancels
/// the tap and opens the seat's action menu instead.
fn split_counter<'a>(seat: usize, value: i32, target: CounterTarget) -> Element<'a, Message> {
    let zone = |sign: i32, glyph: &'static str| {
        mouse_area(
            container(
                text(glyph)
                    .size(34)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.45)),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .on_press(Message::Game(GameMessage::CounterPressStart(seat, target, sign)))
        .on_move(move |point| Message::Game(GameMessage::CounterPressMove(target, sign, point.y)))
        .on_release(Message::Game(GameMessage::CounterPressEnd(target, sign)))
    };

    let zones = row![zone(-1, "\u{2212}"), zone(1, "+")]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill);

    let number = container(
        text(value.to_string())
            .size(76)
            .color(Color::WHITE),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill);

    stack![zones, number].width(Length::Fill).height(Length::Fill).into()
}

/// What a seat's counter currently shows: the value, what it edits, and a
/// short subtitle for the caption (commander name / "Poison" / who the
/// damage is being logged against).
fn active_counter(index: usize, state: &GameState) -> (i32, CounterTarget, String) {
    if let Some(focus) = state.damage_focus {
        if focus != index {
            let amount = state.seats[focus].damage_from(index);
            let target_name = state.seats[focus].player.name.clone();
            return (
                amount,
                CounterTarget::Damage(focus, index),
                format!("Damage to {target_name} (lethal at {LETHAL_COMMANDER_DAMAGE})"),
            );
        }
    }
    let seat = &state.seats[index];
    match state.seat_tab[index] {
        SeatTab::Life => (seat.life, CounterTarget::Life(index), seat.commander.name.clone()),
        SeatTab::Poison => (
            seat.poison,
            CounterTarget::Poison(index),
            format!("Poison (lethal at {LETHAL_POISON})"),
        ),
    }
}

fn action_menu_item(label: &str, message: Message) -> Element<'_, Message> {
    button(text(label).size(17))
        .padding(14)
        .width(Length::Fill)
        .on_press(message)
        .into()
}

/// The swipe-up reveal: pick a view (Life / Commander Damage / Poison) or
/// fire an action (Mark Out, Declare Winner) for this seat.
fn action_menu(index: usize, seat: &Seat) -> Element<'_, Message> {
    container(
        column![
            text("Actions").size(14),
            action_menu_item("Life", Message::Game(GameMessage::SwitchTab(index, SeatTab::Life))),
            action_menu_item(
                "Commander Damage",
                Message::Game(GameMessage::StartDamageFocus(index)),
            ),
            action_menu_item("Poison", Message::Game(GameMessage::SwitchTab(index, SeatTab::Poison))),
            action_menu_item(
                if seat.eliminated { "Back In" } else { "Mark Out" },
                Message::Game(GameMessage::ToggleEliminated(index)),
            ),
            action_menu_item(
                "Declare Winner",
                Message::Game(GameMessage::StartDeclareWinner(index)),
            ),
            action_menu_item("Cancel", Message::Game(GameMessage::CloseActionMenu)),
        ]
        .spacing(8),
    )
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_theme: &iced::Theme| container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.85).into()),
        text_color: Some(Color::WHITE),
        ..container::Style::default()
    })
    .into()
}

fn seat_panel<'a>(
    index: usize,
    state: &'a GameState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let seat = &state.seats[index];
    let is_active = state.active_seat == index;

    // `eliminated` alone gates this, not a live re-check of the stats:
    // otherwise tapping "Back In" while poison/damage is still at a lethal
    // number would immediately re-flag them as out on the very next render.
    if seat.eliminated {
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

    let (value, target, subtitle) = active_counter(index, state);

    let caption = container(
        column![
            text(seat.player.name.clone()).size(20),
            text(subtitle).size(12),
        ]
        .spacing(2),
    )
    .padding(10)
    .width(Length::Fill)
    .style(|_theme: &iced::Theme| container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()),
        text_color: Some(Color::WHITE),
        ..container::Style::default()
    });

    let kill_button = container(
        button(text("Commander Killed").size(14))
            .padding(10)
            .on_press(Message::Game(GameMessage::MarkKilled(index))),
    )
    .width(Length::Fill)
    .padding(10)
    .center_x(Length::Fill);

    // Top caption and bottom kill button float above everything else in the
    // stack, so they still capture their own taps; the vertical space
    // between them is empty and lets taps fall through to the counter zones.
    let chrome = column![caption, iced::widget::vertical_space(), kill_button]
        .width(Length::Fill)
        .height(Length::Fill);

    let card: Element<Message> = if state.action_menu_for == Some(index) {
        stack![art, action_menu(index, seat)].into()
    } else {
        stack![art, split_counter(index, value, target), chrome].into()
    };

    let style_fn: fn(&iced::Theme) -> container::Style = if is_active {
        style::panel_active
    } else {
        style::panel
    };

    container(card)
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
