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
    /// A counter zone currently held down; holding it applies +/-10 every
    /// couple of seconds instead of the normal +/-1 on release.
    pub press_hold: Option<PressHold>,
    /// An upward swipe in progress on a seat tile, tracked until it crosses
    /// the threshold that opens that seat's action menu.
    pub swipe: Option<SwipeState>,
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
    pub target: CounterTarget,
    pub sign: i32,
    pub started_at: Instant,
    /// None until the first +/-10 fires; then tracks the last time it fired
    /// so continuing to hold repeats it every `HOLD_THRESHOLD`.
    pub last_fired_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy)]
pub struct SwipeState {
    pub seat: usize,
    pub baseline_y: Option<f32>,
}

/// Pixels of upward swipe needed to open a seat's action menu.
const SWIPE_OPEN_THRESHOLD: f32 = 55.0;
/// How long a zone must be held before it jumps by +/-10, and how often it
/// repeats while still held.
const HOLD_THRESHOLD: Duration = Duration::from_secs(2);

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
            swipe: None,
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
    CounterPressStart(CounterTarget, i32),
    CounterPressEnd(CounterTarget, i32),
    HoldTick,
    SwipeStart(usize),
    SwipeMove(usize, f32),
    SwipeEnd,
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
            (iced::Task::none(), None)
        }
        GameMessage::SwitchTab(seat, tab) => {
            if let Some(t) = state.seat_tab.get_mut(seat) {
                *t = tab;
            }
            state.action_menu_for = None;
            (iced::Task::none(), None)
        }
        GameMessage::CounterPressStart(target, sign) => {
            state.press_hold = Some(PressHold {
                target,
                sign,
                started_at: Instant::now(),
                last_fired_at: None,
            });
            (iced::Task::none(), None)
        }
        GameMessage::CounterPressEnd(target, sign) => {
            if let Some(hold) = state.press_hold.take() {
                if hold.target == target && hold.sign == sign && hold.last_fired_at.is_none() {
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
        GameMessage::SwipeStart(seat) => {
            state.swipe = Some(SwipeState {
                seat,
                baseline_y: None,
            });
            (iced::Task::none(), None)
        }
        GameMessage::SwipeMove(seat, y) => {
            let mut opened = false;
            if let Some(swipe) = &mut state.swipe {
                if swipe.seat == seat {
                    match swipe.baseline_y {
                        None => swipe.baseline_y = Some(y),
                        Some(baseline) => {
                            if baseline - y >= SWIPE_OPEN_THRESHOLD {
                                opened = true;
                            }
                        }
                    }
                }
            }
            if opened {
                state.action_menu_for = Some(seat);
                state.swipe = None;
            }
            (iced::Task::none(), None)
        }
        GameMessage::SwipeEnd => {
            state.swipe = None;
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
fn split_counter<'a>(value: i32, target: CounterTarget, height: f32) -> Element<'a, Message> {
    let zone = |sign: i32, glyph: &'static str, align: iced::alignment::Horizontal| {
        mouse_area(
            container(text(glyph).size(20))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(align)
                .align_y(iced::alignment::Vertical::Center)
                .padding(18),
        )
        .on_press(Message::Game(GameMessage::CounterPressStart(target, sign)))
        .on_release(Message::Game(GameMessage::CounterPressEnd(target, sign)))
    };

    let zones = container(
        row![
            zone(-1, "\u{2212}", iced::alignment::Horizontal::Left),
            zone(1, "+", iced::alignment::Horizontal::Right),
        ]
        .spacing(0),
    )
    .width(Length::Fill)
    .height(Length::Fixed(height))
    .style(style::panel);

    let number = container(text(value.to_string()).size(52))
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .center_x(Length::Fill)
        .center_y(Length::Fixed(height));

    stack![zones, number].into()
}

fn life_tab(index: usize, seat: &Seat) -> Element<'_, Message> {
    split_counter(seat.life, CounterTarget::Life(index), 90.0)
}

fn poison_tab(index: usize, seat: &Seat) -> Element<'_, Message> {
    column![
        split_counter(seat.poison, CounterTarget::Poison(index), 80.0),
        text(format!("Lethal at {LETHAL_POISON} poison")).size(11),
    ]
    .spacing(6)
    .align_x(iced::Alignment::Center)
    .into()
}

/// Shown on an opponent's own tile (in place of their life total) while
/// someone else is in commander-damage-logging mode: a quick +/- on the
/// damage *this* seat's commander has dealt to the seat being focused.
fn damage_focus_tab(source: usize, target: usize, state: &GameState) -> Element<'_, Message> {
    let amount = state.seats[target].damage_from(source);
    let target_name = state.seats[target].player.name.clone();
    column![
        text(format!("Damage dealt to {target_name}")).size(13),
        split_counter(amount, CounterTarget::Damage(target, source), 80.0),
        text(format!("Lethal at {LETHAL_COMMANDER_DAMAGE}")).size(11),
    ]
    .spacing(8)
    .align_x(iced::Alignment::Center)
    .into()
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

    let body = match state.damage_focus {
        Some(focus) if focus != index => damage_focus_tab(index, focus, state),
        _ => match state.seat_tab[index] {
            SeatTab::Life => life_tab(index, seat),
            SeatTab::Poison => poison_tab(index, seat),
        },
    };

    let controls = container(
        column![
            body,
            button(text("Commander Killed").size(16))
                .padding(14)
                .width(Length::Fill)
                .on_press(Message::Game(GameMessage::MarkKilled(index))),
        ]
        .spacing(10),
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

    let card: Element<Message> = if state.action_menu_for == Some(index) {
        stack![art, action_menu(index, seat)].into()
    } else {
        stack![art, overlay].into()
    };

    let style_fn: fn(&iced::Theme) -> container::Style = if is_active {
        style::panel_active
    } else {
        style::panel
    };

    let swipeable = mouse_area(card)
        .on_press(Message::Game(GameMessage::SwipeStart(index)))
        .on_move(move |point| Message::Game(GameMessage::SwipeMove(index, point.y)))
        .on_release(Message::Game(GameMessage::SwipeEnd))
        .on_exit(Message::Game(GameMessage::SwipeEnd));

    container(swipeable)
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
