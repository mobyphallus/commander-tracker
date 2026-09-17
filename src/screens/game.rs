use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::Utc;
use iced::widget::{button, column, container, image, mouse_area, row, scrollable, stack, text};
use iced::{Border, Color, ContentFit, Element, Length};
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
    /// A life +/- button currently held down; a 3s hold applies +/-10 instead
    /// of the normal +/-1 on release.
    pub press_hold: Option<PressHold>,
    /// A swipe in progress over a poison or commander-damage counter.
    pub drag: Option<DragState>,
}

#[derive(Debug, Clone, Copy)]
pub struct PressHold {
    pub seat: usize,
    pub sign: i32,
    pub started_at: Instant,
    pub fired: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragTarget {
    Poison(usize),
    Damage(usize, usize),
}

#[derive(Debug, Clone, Copy)]
pub struct DragState {
    pub target: DragTarget,
    pub baseline_y: Option<f32>,
}

/// Pixels of vertical swipe needed to register one +/-1 step.
const SWIPE_STEP: f32 = 28.0;
/// How long a life button must be held before it jumps to +/-10.
const HOLD_THRESHOLD: Duration = Duration::from_secs(3);

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
            press_hold: None,
            drag: None,
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
    LifePressStart(usize, i32),
    LifePressEnd(usize, i32),
    HoldTick,
    DragStart(DragTarget),
    DragMove(DragTarget, f32),
    DragEnd,
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
        GameMessage::LifePressStart(seat, sign) => {
            state.press_hold = Some(PressHold {
                seat,
                sign,
                started_at: Instant::now(),
                fired: false,
            });
            (iced::Task::none(), None)
        }
        GameMessage::LifePressEnd(seat, sign) => {
            if let Some(hold) = state.press_hold.take() {
                if hold.seat == seat && hold.sign == sign && !hold.fired {
                    apply_life_delta(state, seat, sign);
                }
            }
            (iced::Task::none(), None)
        }
        GameMessage::HoldTick => {
            if let Some(hold) = state.press_hold {
                if !hold.fired && hold.started_at.elapsed() >= HOLD_THRESHOLD {
                    if let Some(h) = &mut state.press_hold {
                        h.fired = true;
                    }
                    apply_life_delta(state, hold.seat, hold.sign * 10);
                }
            }
            (iced::Task::none(), None)
        }
        GameMessage::DragStart(target) => {
            state.drag = Some(DragState {
                target,
                baseline_y: None,
            });
            (iced::Task::none(), None)
        }
        GameMessage::DragMove(target, y) => {
            let mut steps = 0;
            if let Some(drag) = &mut state.drag {
                if drag.target == target {
                    match drag.baseline_y {
                        None => drag.baseline_y = Some(y),
                        Some(baseline) => {
                            let delta = baseline - y;
                            if delta.abs() >= SWIPE_STEP {
                                steps = (delta / SWIPE_STEP).trunc() as i32;
                                drag.baseline_y = Some(y);
                            }
                        }
                    }
                }
            }
            if steps != 0 {
                apply_drag_steps(state, target, steps);
            }
            (iced::Task::none(), None)
        }
        GameMessage::DragEnd => {
            state.drag = None;
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

fn apply_life_delta(state: &mut GameState, seat: usize, delta: i32) {
    if let Some(s) = state.seats.get_mut(seat) {
        s.life += delta;
    }
    state.check_zero_life(seat);
}

fn apply_drag_steps(state: &mut GameState, target: DragTarget, steps: i32) {
    match target {
        DragTarget::Poison(seat) => {
            if let Some(s) = state.seats.get_mut(seat) {
                s.poison = (s.poison + steps).max(0);
            }
            state.check_hard_elimination(seat);
        }
        DragTarget::Damage(target_seat, source_seat) => {
            if let Some(seat) = state.seats.get_mut(target_seat) {
                let entry = seat.commander_damage_taken.entry(source_seat).or_insert(0);
                let before = *entry;
                let after = (before + steps).max(0);
                *entry = after;
                seat.life -= after - before;
            }
            state.check_hard_elimination(target_seat);
            state.check_zero_life(target_seat);
        }
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

    let bar_row = container(
        row![
            text(format!(
                "{}'s turn - {}",
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

    // A frosted-glass-style panel floating over the header, centered
    // regardless of how wide the surrounding controls are.
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
        background: Some(Color::from_rgba(1.0, 1.0, 1.0, 0.14).into()),
        text_color: Some(Color::WHITE),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.28),
            width: 1.0,
            radius: 20.0.into(),
        },
        ..container::Style::default()
    });

    let top_bar = stack![
        bar_row,
        container(timer_panel).width(Length::Fill).center_x(Length::Fill),
    ];

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
    button(text(label).size(16))
        .padding(12)
        .width(Length::Fill)
        .style(if selected { button::primary } else { button::secondary })
        .on_press(Message::Game(GameMessage::SwitchTab(seat, tab)))
        .into()
}

/// A big, thumb-friendly life +/- control. Tap for +/-1; hold for three
/// seconds to jump by +/-10 instead.
fn life_button(seat: usize, sign: i32, label: &str) -> Element<'_, Message> {
    mouse_area(
        container(text(label).size(30))
            .width(Length::Fixed(72.0))
            .height(Length::Fixed(72.0))
            .center_x(Length::Fixed(72.0))
            .center_y(Length::Fixed(72.0))
            .style(style::panel),
    )
    .on_press(Message::Game(GameMessage::LifePressStart(seat, sign)))
    .on_release(Message::Game(GameMessage::LifePressEnd(seat, sign)))
    .into()
}

fn life_tab(index: usize, seat: &Seat) -> Element<'_, Message> {
    row![
        life_button(index, -1, "-"),
        text(seat.life.to_string())
            .size(48)
            .width(Length::Fill)
            .align_x(iced::Alignment::Center),
        life_button(index, 1, "+"),
    ]
    .spacing(14)
    .align_y(iced::Alignment::Center)
    .into()
}

/// A large swipe zone: swipe up to add one, down to remove one.
fn swipe_zone<'a>(target: DragTarget, content: Element<'a, Message>) -> Element<'a, Message> {
    mouse_area(content)
        .on_press(Message::Game(GameMessage::DragStart(target)))
        .on_move(move |point| Message::Game(GameMessage::DragMove(target, point.y)))
        .on_release(Message::Game(GameMessage::DragEnd))
        .on_exit(Message::Game(GameMessage::DragEnd))
        .into()
}

fn poison_tab(index: usize, seat: &Seat) -> Element<'_, Message> {
    let content = container(
        column![
            text(seat.poison.to_string()).size(48),
            text("Swipe up: +1  \u{00b7}  down: -1").size(13),
            text(format!("Lethal at {LETHAL_POISON} poison")).size(11),
        ]
        .spacing(6)
        .align_x(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .style(style::panel);

    swipe_zone(DragTarget::Poison(index), content.into())
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
            let content = container(
                row![
                    text(format!("{} ({})", other.commander.name, other.player.name))
                        .size(14)
                        .width(Length::Fill),
                    text(amount.to_string()).size(26),
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            )
            .padding(12)
            .width(Length::Fill)
            .height(Length::Fixed(64.0))
            .style(style::panel);

            swipe_zone(DragTarget::Damage(index, j), content.into())
        })
        .collect();

    column![
        text(format!(
            "Lethal at {LETHAL_COMMANDER_DAMAGE} from one commander \u{00b7} swipe up/down"
        ))
        .size(11),
        scrollable(column(rows).spacing(8)).height(Length::Fill),
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

    let controls = container(
        column![
            tabs,
            body,
            row![
                button(text(if seat.eliminated { "Back In" } else { "Mark Out" }).size(16))
                    .padding(14)
                    .width(Length::Fill)
                    .style(if seat.eliminated { button::secondary } else { button::danger })
                    .on_press(Message::Game(GameMessage::ToggleEliminated(index))),
                button(text("Commander Killed").size(16))
                    .padding(14)
                    .width(Length::Fill)
                    .on_press(Message::Game(GameMessage::MarkKilled(index))),
            ]
            .spacing(8),
            button(text("Declare Winner").size(18))
                .padding(16)
                .width(Length::Fill)
                .style(button::success)
                .on_press(Message::Game(GameMessage::StartDeclareWinner(index))),
        ]
        .spacing(8),
    )
    .padding(10)
    .width(Length::Fill)
    .style(|_theme: &iced::Theme| container::Style {
        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.62).into()),
        text_color: Some(Color::WHITE),
        ..container::Style::default()
    });

    // Art fills the entire tile; the name sits on top of it up high, and the
    // interactive controls float over the bottom on their own scrim.
    let overlay = column![caption, iced::widget::vertical_space(), controls]
        .width(Length::Fill)
        .height(Length::Fill);

    let card = stack![art, overlay];

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
