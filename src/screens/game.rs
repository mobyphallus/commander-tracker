#[path = "game_center.rs"]
pub(crate) mod center;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::Utc;
use iced::widget::{button, column, container, image, mouse_area, row, scrollable, stack, text};
use iced::{Element, Length};
use rusqlite::Connection;

use crate::app::Message;
use crate::art;
use crate::cards;
use crate::db;
use crate::icon::Glyph;
use crate::layout::{self, SeatOrientation, TableLayout};
use crate::model::{
    Elimination, FinishedGame, HateKind, KillEvent, OutCause, Seat, WinReason,
    LETHAL_COMMANDER_DAMAGE, LETHAL_POISON, PARTNER, PRIMARY,
};
use crate::rotated::{self, Line};
use crate::style;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SeatTab {
    Life,
    Poison,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct GameState {
    #[serde(skip)]
    pub feedback_panel: Option<crate::feedback::Panel>,
    #[serde(skip)]
    pub error: Option<String>,
    #[serde(skip)]
    pub result_save_failed: bool,
    #[serde(skip)]
    pub help_open: bool,
    #[serde(skip)]
    pub help_was_paused: bool,
    #[serde(skip)]
    pub undo_open: bool,
    #[serde(skip)]
    pub undo: Vec<crate::session::UndoEntry>,
    pub seats: Vec<Seat>,
    pub table_layout: TableLayout,
    pub started_at: chrono::DateTime<Utc>,
    /// Seat indices in the order the pod plays, first player first. Chosen
    /// during setup: this is the table's real seating ring, not seat-index
    /// order, so it already accounts for which way turns pass.
    pub turn_order: Vec<usize>,
    /// Where in `turn_order` the current turn is.
    pub turn_index: usize,
    pub active_seat: usize,
    /// The active player's turn count, including the turn in progress.
    /// Each player begins at one; ending a turn advances only the next player.
    pub turn_number: u32,
    turns_taken: Vec<u32>,
    pub turn_seconds: u64,
    pub game_seconds: u64,
    pub paused: bool,
    pub seat_tab: Vec<SeatTab>,
    pub kills: Vec<KillEvent>,
    /// The in-progress "commander hate" log: which seat it happened to, and
    /// what kind once picked (the next step asks who did it).
    pub hate_flow: Option<HateFlow>,
    /// A seat on its way out, and how far through the "why, then who"
    /// prompt chain it has got.
    pub out_flow: Option<OutFlow>,
    /// A seat that just hit 0 life and hasn't been asked "are they out?" yet
    /// (or was asked and said no, until they drop to 0 again).
    pub pending_life_check: Option<usize>,
    pub zero_life_prompt_dismissed: Vec<bool>,
    pub pending_winner: Option<usize>,
    pub pending_reason: Option<WinReason>,
    /// Set while the "are you sure" prompt for abandoning is up. Abandoning
    /// saves nothing, so it never happens on a single tap.
    pub pending_abandon: bool,
    /// Global controls are separate from routine seat interactions.
    pub game_menu_open: bool,
    /// A counter zone currently held down; holding it applies +/-10 every
    /// couple of seconds instead of the normal +/-1 on release, and an
    /// upward drag past the swipe threshold turns it into opening the
    /// action menu instead.
    #[serde(skip)]
    pub press_hold: Option<PressHold>,
    /// Which seat's action menu (Commander Damage / Poison / Mark Out /
    /// Declare Winner) is currently open, if any.
    pub action_menu_for: Option<usize>,
    /// While set, every OTHER seat's tile swaps its life display for a
    /// quick +/- on the commander damage *that seat* has dealt to this one.
    pub damage_focus: Option<usize>,
    /// Per seat, which of that seat's commanders the damage focus is
    /// currently logging. Only meaningful for seats running a partner pair;
    /// everyone else stays on PRIMARY.
    pub damage_slot: Vec<usize>,
}

/// Marking a seat out: why first, then who gets the kill. Some causes skip
/// straight past the first question, and one skips both.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct OutFlow {
    pub seat: usize,
    /// `None` while the cause is still being asked.
    pub cause: Option<OutCause>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct HateFlow {
    pub victim: usize,
    pub kind: Option<HateKind>,
}

/// Any counter that can be adjusted with the hold-to-repeat left/right zones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterTarget {
    Life(usize),
    Poison(usize),
    /// (seat taking the damage, seat that dealt it, which of that seat's
    /// commanders dealt it)
    Damage(usize, usize, usize),
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
    /// Since the +/- zones now cover the whole tile, a drag past the swipe
    /// threshold cancels the tap/hold and does something else instead: up
    /// opens the action menu, sideways opens commander damage. Tracked here
    /// rather than as a separate gesture.
    pub swipe_baseline: Option<iced::Point>,
    pub became_swipe: bool,
}

/// Pixels of upward swipe needed to open a seat's action menu.
const SWIPE_OPEN_THRESHOLD: f32 = 55.0;
/// Pixels of sideways swipe needed to start logging commander damage
/// against a seat. Higher than the upward threshold because a sideways
/// drift during a tap is far more common than a deliberate upward one.
const SWIPE_DAMAGE_THRESHOLD: f32 = 90.0;
/// How long a zone must be held before it jumps by +/-10, and how often it
/// repeats while still held.
const HOLD_THRESHOLD: Duration = Duration::from_secs(2);

impl GameState {
    pub(crate) fn valid_recovery_lengths(&self) -> bool {
        let n = self.seats.len();
        self.turns_taken.len() == n
            && self.seat_tab.len() == n
            && self.zero_life_prompt_dismissed.len() == n
            && self.damage_slot.len() == n
            && [
                self.pending_life_check,
                self.pending_winner,
                self.action_menu_for,
                self.damage_focus,
                self.out_flow.map(|f| f.seat),
                self.hate_flow.map(|f| f.victim),
            ]
            .into_iter()
            .flatten()
            .all(|i| i < n)
            && self
                .kills
                .iter()
                .all(|k| k.victim_seat < n && k.killer_seat.is_none_or(|i| i < n))
            && self.seats.iter().all(|s| {
                s.elimination
                    .is_none_or(|e| e.killer_seat.is_none_or(|i| i < n))
            })
    }

    /// Which of `seat`'s commanders the damage focus is logging, clamped
    /// to what that seat actually has so a stale PARTNER can never point at
    /// a commander that isn't there.
    pub fn damage_slot(&self, seat: usize) -> usize {
        let has_partner = self.seats.get(seat).is_some_and(|s| s.partner.is_some());
        match self.damage_slot.get(seat).copied().unwrap_or(PRIMARY) {
            PARTNER if has_partner => PARTNER,
            _ => PRIMARY,
        }
    }

    pub fn new(seats: Vec<Seat>, table_layout: TableLayout, turn_order: Vec<usize>) -> Self {
        let seat_count = seats.len();
        let seat_tab = vec![SeatTab::Life; seats.len()];
        let zero_life_prompt_dismissed = vec![false; seats.len()];
        // Setup always supplies a full order; fall back to seat order so a
        // malformed one can never leave the game with nobody to play.
        let turn_order = if turn_order.len() == seats.len() {
            turn_order
        } else {
            (0..seats.len()).collect()
        };
        let active_seat = turn_order.first().copied().unwrap_or(0);
        let mut turns_taken = vec![0; seat_count];
        if let Some(turns) = turns_taken.get_mut(active_seat) {
            *turns = 1;
        }
        Self {
            feedback_panel: None,
            error: None,
            result_save_failed: false,
            help_open: false,
            help_was_paused: false,
            undo_open: false,
            undo: Vec::new(),
            turns_taken,
            seats,
            table_layout,
            started_at: Utc::now(),
            turn_order,
            turn_index: 0,
            active_seat,
            turn_number: 1,
            turn_seconds: 0,
            game_seconds: 0,
            paused: false,
            seat_tab,
            kills: Vec::new(),
            hate_flow: None,
            out_flow: None,
            pending_life_check: None,
            zero_life_prompt_dismissed,
            pending_winner: None,
            pending_abandon: false,
            game_menu_open: false,
            pending_reason: None,
            press_hold: None,
            action_menu_for: None,
            damage_focus: None,
            damage_slot: vec![PRIMARY; seat_count],
        }
    }

    /// Which seat's commander has dealt this one a lethal 21, if any.
    fn lethal_commander_damage_source(&self, seat: usize) -> Option<usize> {
        self.seats[seat]
            .commander_damage_taken
            .iter()
            .find(|(_, &amount)| amount >= LETHAL_COMMANDER_DAMAGE)
            .map(|(&(source, _), _)| source)
    }

    /// Handles the two deaths the rules decide for us. Lethal commander
    /// damage names its own killer - the commander that dealt the 21st point
    /// is right there on the table - so it goes straight in the book. Poison
    /// is just as lethal but anyone at the table could have put those
    /// counters on, so that one has to be asked.
    fn check_hard_elimination(&mut self, seat: usize) {
        if self.seats[seat].eliminated || self.out_flow.is_some() {
            return;
        }
        if let Some(source) = self.lethal_commander_damage_source(seat) {
            let turn = self.turn_number;
            self.seats[seat].mark_out(Elimination {
                cause: OutCause::CommanderDamage,
                killer_seat: Some(source),
                turn,
            });
            return;
        }
        if self.seats[seat].poison >= LETHAL_POISON {
            self.out_flow = Some(OutFlow {
                seat,
                cause: Some(OutCause::Poison),
            });
        }
    }

    /// Once a prompt closes, pick up anyone else the board has already
    /// killed - one board wipe can put two seats out at the same moment,
    /// and only one of them can be asked about at a time.
    fn next_pending_out(&mut self) {
        for seat in 0..self.seats.len() {
            self.check_hard_elimination(seat);
            if self.out_flow.is_some() {
                return;
            }
        }
    }

    /// Put a seat out at the current turn.
    fn mark_out(&mut self, seat: usize, cause: OutCause, killer_seat: Option<usize>) {
        let turn = self.turn_number;
        self.seats[seat].mark_out(Elimination {
            cause,
            killer_seat,
            turn,
        });
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
    OpenFeedback(usize),
    CloseFeedback,
    Tick,
    Undo,
    OpenUndo,
    CloseUndo,
    ShowHelp,
    DismissHelp,
    RetrySave,
    DismissError,
    SaveAndHome,
    TogglePause,
    OpenGameMenu,
    CloseGameMenu,
    NextTurn,
    SwitchTab(usize, SeatTab),
    CounterPressStart(usize, CounterTarget, i32),
    /// (target, sign, cursor x, cursor y)
    CounterPressMove(CounterTarget, i32, f32, f32),
    CounterPressEnd(CounterTarget, i32),
    HoldTick,
    CloseActionMenu,
    StartDamageFocus(usize),
    /// (seat, which of that seat's commanders to log damage for)
    SelectDamageSlot(usize, usize),
    EndDamageFocus,
    ToggleEliminated(usize),
    /// Why a seat went out, when it had to be asked.
    PickOutCause(OutCause),
    /// Who gets the kill. `None` is a real answer, not a cancel.
    ConfirmOutKiller(Option<usize>),
    CancelOut,
    AnswerZeroLifeCheck(bool),
    StartHate(usize),
    PickHateKind(HateKind),
    CancelHate,
    ConfirmHate(Option<usize>),
    StartDeclareWinner(usize),
    CancelDeclareWinner,
    PickWinReason(WinReason),
    ConfirmEndGame,
    /// Puts the confirmation prompt up. `AbandonGame` is the answer to it,
    /// and the only thing that actually throws the game away.
    RequestAbandon,
    CancelAbandon,
    AbandonGame,
}

pub enum Action {
    Finished,
    Abandoned,
    Suspended,
}

pub fn update(
    state: &mut GameState,
    conn: &mut Connection,
    message: GameMessage,
) -> (iced::Task<Message>, Option<Action>) {
    crate::session::update(state, conn, message)
}

pub(crate) fn update_inner(
    state: &mut GameState,
    conn: &mut Connection,
    message: GameMessage,
) -> (iced::Task<Message>, Option<Action>) {
    match message {
        GameMessage::OpenFeedback(_) => (iced::Task::none(), None),
        GameMessage::CloseFeedback => {
            state.feedback_panel = None;
            (iced::Task::none(), None)
        }
        GameMessage::Undo => {
            crate::session::undo(state);
            (iced::Task::none(), None)
        }
        GameMessage::OpenUndo => {
            state.undo_open = true;
            state.game_menu_open = false;
            (iced::Task::none(), None)
        }
        GameMessage::CloseUndo => {
            state.undo_open = false;
            (iced::Task::none(), None)
        }
        GameMessage::ShowHelp => {
            state.help_was_paused = state.paused;
            state.paused = true;
            state.help_open = true;
            state.game_menu_open = false;
            (iced::Task::none(), None)
        }
        GameMessage::DismissHelp => {
            state.help_open = false;
            state.paused = state.help_was_paused;
            if let Err(e) = db::set_setting(conn, "gestures_seen", "yes") {
                state.error = Some(e.to_string());
            }
            (iced::Task::none(), None)
        }
        GameMessage::DismissError => {
            state.error = None;
            (iced::Task::none(), None)
        }
        GameMessage::RetrySave => {
            state.error = None;
            if std::mem::take(&mut state.result_save_failed) {
                return update_inner(state, conn, GameMessage::ConfirmEndGame);
            }
            if let Err(e) = crate::session::save(conn, state) {
                state.error = Some(e);
            }
            (iced::Task::none(), None)
        }
        GameMessage::SaveAndHome => {
            state.paused = true;
            match crate::session::save(conn, state) {
                Ok(()) => (iced::Task::none(), Some(Action::Suspended)),
                Err(e) => {
                    state.error = Some(e);
                    (iced::Task::none(), None)
                }
            }
        }
        GameMessage::Tick => {
            if !state.paused {
                state.turn_seconds += 1;
                state.game_seconds += 1;
            }
            (iced::Task::none(), None)
        }
        GameMessage::OpenGameMenu => {
            state.game_menu_open = true;
            state.press_hold = None;
            state.action_menu_for = None;
            (iced::Task::none(), None)
        }
        GameMessage::CloseGameMenu => {
            state.game_menu_open = false;
            (iced::Task::none(), None)
        }
        GameMessage::TogglePause => {
            state.paused = !state.paused;
            state.game_menu_open = false;
            (iced::Task::none(), None)
        }
        GameMessage::NextTurn => {
            let n = state.turn_order.len();
            if n == 0 || state.seats.iter().all(|seat| seat.eliminated) {
                return (iced::Task::none(), None);
            }
            // Walk the chosen order, skipping anyone already out. If
            // everyone else is eliminated this lands back on the current
            // seat, which is the right answer for a solo survivor.
            for _ in 0..n {
                state.turn_index = (state.turn_index + 1) % n;
                if !state.seats[state.turn_order[state.turn_index]].eliminated {
                    break;
                }
            }
            state.active_seat = state.turn_order[state.turn_index];
            state.turn_seconds = 0;
            state.turns_taken[state.active_seat] += 1;
            state.turn_number = state.turns_taken[state.active_seat];
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
                swipe_baseline: None,
                became_swipe: false,
            });
            (iced::Task::none(), None)
        }
        GameMessage::CounterPressMove(target, sign, x, y) => {
            let mut open_menu = None;
            let mut open_damage = None;
            if let Some(hold) = &mut state.press_hold {
                if hold.target == target && hold.sign == sign && !hold.became_swipe {
                    match hold.swipe_baseline {
                        None => hold.swipe_baseline = Some(iced::Point::new(x, y)),
                        Some(baseline) => {
                            // Judged in the player's own frame, not the
                            // screen's: someone sitting opposite swipes away
                            // from themselves by dragging DOWN the screen,
                            // and someone at a head of the table does it
                            // sideways. Screen axes would read those as the
                            // wrong gesture entirely.
                            let (up, across) = state
                                .table_layout
                                .seat_orientation(hold.seat)
                                .drag_in_player_frame(x - baseline.x, y - baseline.y);
                            // Whichever axis has travelled further decides
                            // the gesture, so a diagonal drag resolves to
                            // one thing rather than firing both.
                            if up >= SWIPE_OPEN_THRESHOLD && up >= across {
                                hold.became_swipe = true;
                                open_menu = Some(hold.seat);
                            } else if across >= SWIPE_DAMAGE_THRESHOLD && across > up.max(0.0) {
                                hold.became_swipe = true;
                                open_damage = Some(hold.seat);
                            }
                        }
                    }
                }
            }
            if let Some(seat) = open_menu {
                state.action_menu_for = Some(seat);
                state.press_hold = None;
            }
            if let Some(seat) = open_damage {
                state.press_hold = None;
                return update(state, conn, GameMessage::StartDamageFocus(seat));
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
            // Start each source seat back on its primary commander, so a
            // slot left selected from a previous focus can't quietly log
            // damage against the wrong half of a partner pair.
            state.damage_slot.iter_mut().for_each(|s| *s = PRIMARY);
            // The seat being swiped on shows its life while everyone else
            // logs damage into it, even if it was left showing poison.
            state.seat_tab.iter_mut().for_each(|t| *t = SeatTab::Life);
            (iced::Task::none(), None)
        }
        GameMessage::SelectDamageSlot(seat, slot) => {
            if let Some(current) = state.damage_slot.get_mut(seat) {
                *current = slot;
            }
            (iced::Task::none(), None)
        }
        GameMessage::EndDamageFocus => {
            state.damage_focus = None;
            // Back to life totals everywhere, even for a seat that was
            // showing poison before the damage focus started - life is what
            // you want to see the moment combat is done being logged.
            state.seat_tab.iter_mut().for_each(|t| *t = SeatTab::Life);
            (iced::Task::none(), None)
        }
        GameMessage::ToggleEliminated(seat) => {
            state.action_menu_for = None;
            match state.seats.get(seat).map(|s| s.eliminated) {
                // Back in: the old record goes with them. They did not die
                // on that turn after all.
                Some(true) => state.seats[seat].bring_back(),
                // Out by hand, so nothing on the board explains it - ask.
                Some(false) => state.out_flow = Some(OutFlow { seat, cause: None }),
                None => {}
            }
            (iced::Task::none(), None)
        }
        GameMessage::PickOutCause(cause) => {
            if let Some(flow) = state.out_flow {
                if cause.needs_killer_prompt() {
                    state.out_flow = Some(OutFlow {
                        cause: Some(cause),
                        ..flow
                    });
                } else {
                    // A concede has no killer by definition.
                    state.out_flow = None;
                    state.mark_out(flow.seat, cause, None);
                    state.next_pending_out();
                }
            }
            (iced::Task::none(), None)
        }
        GameMessage::ConfirmOutKiller(killer) => {
            if let Some(flow) = state.out_flow.take() {
                state.mark_out(flow.seat, flow.cause.unwrap_or(OutCause::Other), killer);
                state.next_pending_out();
            }
            (iced::Task::none(), None)
        }
        GameMessage::CancelOut => {
            if let Some(flow) = state.out_flow.take() {
                // Backing out leaves the seat in the game. If the board is
                // what called it, the next counter change asks again.
                state.zero_life_prompt_dismissed[flow.seat] = true;
            }
            (iced::Task::none(), None)
        }
        GameMessage::AnswerZeroLifeCheck(out) => {
            if let Some(seat) = state.pending_life_check.take() {
                if out {
                    // They're out - but 0 life doesn't say who did it.
                    state.out_flow = Some(OutFlow {
                        seat,
                        cause: Some(OutCause::LifeLoss),
                    });
                } else {
                    state.zero_life_prompt_dismissed[seat] = true;
                }
            }
            (iced::Task::none(), None)
        }
        GameMessage::StartHate(seat) => {
            state.hate_flow = Some(HateFlow {
                victim: seat,
                kind: None,
            });
            (iced::Task::none(), None)
        }
        GameMessage::PickHateKind(kind) => {
            if let Some(flow) = &mut state.hate_flow {
                flow.kind = Some(kind);
            }
            (iced::Task::none(), None)
        }
        GameMessage::CancelHate => {
            state.hate_flow = None;
            (iced::Task::none(), None)
        }
        GameMessage::ConfirmHate(culprit) => {
            if let Some(flow) = state.hate_flow.take() {
                if let Some(kind) = flow.kind {
                    state.kills.push(KillEvent {
                        victim_seat: flow.victim,
                        killer_seat: culprit,
                        kind,
                    });
                }
            }
            (iced::Task::none(), None)
        }
        GameMessage::StartDeclareWinner(seat) => {
            state.pending_winner = Some(seat);
            state.pending_reason = None;
            state.action_menu_for = None;
            (iced::Task::none(), None)
        }
        GameMessage::CancelDeclareWinner => {
            state.result_save_failed = false;
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
                    elapsed_seconds: state.game_seconds,
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
                    Err(e) => {
                        state.result_save_failed = true;
                        state.error = Some(format!(
                            "Couldn’t save this game: {e}. Your game is still open; try again."
                        ));
                        (iced::Task::none(), None)
                    }
                }
            } else {
                (iced::Task::none(), None)
            }
        }
        GameMessage::RequestAbandon => {
            state.game_menu_open = false;
            state.press_hold = None;
            state.pending_abandon = true;
            (iced::Task::none(), None)
        }
        GameMessage::CancelAbandon => {
            state.pending_abandon = false;
            (iced::Task::none(), None)
        }
        GameMessage::AbandonGame => {
            if state.pending_abandon {
                match crate::feedback::abandon(conn, state) {
                    Ok(()) => return (iced::Task::none(), Some(Action::Abandoned)),
                    Err(e) => state.error = Some(format!("Couldn’t discard the saved game: {e}")),
                }
            }
            (iced::Task::none(), None)
        }
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

fn apply_damage_delta(
    state: &mut GameState,
    target: usize,
    source: usize,
    slot: usize,
    delta: i32,
) {
    if let Some(seat) = state.seats.get_mut(target) {
        let entry = seat
            .commander_damage_taken
            .entry((source, slot))
            .or_insert(0);
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
        CounterTarget::Damage(target_seat, source_seat, slot) => {
            apply_damage_delta(state, target_seat, source_seat, slot, delta)
        }
    }
}

fn format_duration(total_seconds: u64) -> String {
    format!("{:02}:{:02}", total_seconds / 60, total_seconds % 60)
}

/// Global controls live away from the counters, behind the central timer.
fn game_menu_view(state: &GameState) -> Element<'_, Message> {
    dialog(
        column![
            crate::icon::view(crate::icon::Glyph::History, 32., style::ACCENT_BRIGHT),
            text("Game menu").size(style::T_TITLE),
            text(if state.paused {
                "Timers are paused."
            } else {
                "Timers are running."
            })
            .size(style::T_BODY)
            .color(style::TEXT_MUTED),
            style::icon_button(
                if state.paused {
                    crate::icon::Glyph::Play
                } else {
                    crate::icon::Glyph::Pause
                },
                if state.paused {
                    "Resume game"
                } else {
                    "Pause timers"
                },
                style::T_ACTION
            )
            .width(Length::Fill)
            .style(style::secondary)
            .on_press(Message::Game(GameMessage::TogglePause)),
            style::touch_button("Recent actions & undo", style::T_LABEL)
                .width(Length::Fill)
                .style(style::secondary)
                .on_press(Message::Game(GameMessage::OpenUndo)),
            style::touch_button("Touch controls", style::T_LABEL)
                .width(Length::Fill)
                .style(style::secondary)
                .on_press(Message::Game(GameMessage::ShowHelp)),
            style::touch_button("Save & return home", style::T_LABEL)
                .width(Length::Fill)
                .style(style::secondary)
                .on_press(Message::Game(GameMessage::SaveAndHome)),
            style::touch_button("Back to game", style::T_ACTION)
                .width(Length::Fill)
                .style(style::primary)
                .on_press(Message::Game(GameMessage::CloseGameMenu)),
            style::icon_button(crate::icon::Glyph::Close, "Abandon game", style::T_LABEL)
                .width(Length::Fill)
                .style(style::danger_ghost)
                .on_press(Message::Game(GameMessage::RequestAbandon)),
        ]
        .spacing(style::GAP),
    )
}

fn dialog<'a>(content: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    container(
        container(scrollable(content).height(Length::Shrink))
            .padding(32)
            .max_width(560)
            .style(style::panel),
    )
    .padding(style::GAP)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// Abandoning writes nothing; the primary escape keeps the current game.
fn abandon_confirm_view<'a>() -> Element<'a, Message> {
    dialog(column![
        text("Abandon this game?").size(style::T_TITLE),
        text("This game won't be saved to history. Its life totals, commander damage and turn count will be lost.")
            .size(style::T_BODY).color(style::TEXT_MUTED),
        style::touch_button("Keep playing", style::T_ACTION).width(Length::Fill)
            .style(style::primary).on_press(Message::Game(GameMessage::CancelAbandon)),
        style::touch_button("Abandon game", style::T_ACTION).width(Length::Fill)
            .style(style::danger).on_press(Message::Game(GameMessage::AbandonGame)),
    ].spacing(style::GAP))
}

pub fn view<'a>(
    state: &'a GameState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    if let Some(panel) = &state.feedback_panel {
        return crate::feedback::view(panel, Message::Game(GameMessage::CloseFeedback));
    }
    if let Some(error) = &state.error {
        return dialog(
            column![
                text("Your game is still here").size(style::T_TITLE),
                text(error),
                style::touch_button("Try saving again", style::T_ACTION)
                    .style(style::primary)
                    .on_press(Message::Game(GameMessage::RetrySave)),
                style::touch_button("Back to game", style::T_ACTION)
                    .style(style::secondary)
                    .on_press(Message::Game(GameMessage::DismissError))
            ]
            .spacing(style::GAP),
        );
    }
    if state.help_open {
        return dialog(
            column![
                text("Touch controls").size(style::T_TITLE),
                text("Each tile faces its player. These gestures follow your seat’s orientation.")
                    .size(style::T_BODY),
                text("Tap the left / right half to subtract / add 1.").size(style::T_LABEL),
                text("Hold for 2 seconds to change by 10; keep holding to repeat.")
                    .size(style::T_LABEL),
                text("Swipe away from yourself for poison and player actions.")
                    .size(style::T_LABEL),
                text("Swipe sideways to record commander damage.").size(style::T_LABEL),
                text("Tap the center timer for pause, undo, help, and saving.")
                    .size(style::T_LABEL),
                style::touch_button("Got it", style::T_ACTION)
                    .width(Length::Fill)
                    .style(style::primary)
                    .on_press(Message::Game(GameMessage::DismissHelp))
            ]
            .spacing(style::GAP),
        );
    }
    if state.undo_open {
        let mut actions = column![
            text("Recent actions").size(style::T_TITLE),
            text("Undo restores the most recent action, including any elimination it caused.")
                .size(style::T_BODY)
        ]
        .spacing(style::GAP);
        for entry in state.undo.iter().rev().take(10) {
            actions = actions.push(text(&entry.label).size(style::T_LABEL));
        }
        if state.undo.is_empty() {
            actions = actions.push(text("No actions to undo in this session."));
        }
        let mut undo = style::touch_button("Undo latest action", style::T_ACTION)
            .width(Length::Fill)
            .style(style::secondary);
        if !state.undo.is_empty() {
            undo = undo.on_press(Message::Game(GameMessage::Undo));
        }
        return dialog(
            actions.push(undo).push(
                style::touch_button("Back to game", style::T_ACTION)
                    .width(Length::Fill)
                    .style(style::primary)
                    .on_press(Message::Game(GameMessage::CloseUndo)),
            ),
        );
    }
    if let Some(seat) = state.pending_life_check {
        return zero_life_check_view(state, seat);
    }
    if let Some(winner) = state.pending_winner {
        return declare_winner_view(state, winner);
    }
    if let Some(flow) = state.out_flow {
        return out_view(state, flow);
    }
    if let Some(flow) = state.hate_flow {
        return hate_view(state, flow);
    }
    if state.pending_abandon {
        return abandon_confirm_view();
    }

    if state.game_menu_open {
        return game_menu_view(state);
    }

    iced::widget::responsive(move |size| board_view(state, image_cache, size)).into()
}

fn board_view<'a>(
    state: &'a GameState,
    image_cache: &'a HashMap<String, image::Handle>,
    size: iced::Size,
) -> Element<'a, Message> {
    let diameter = center::diameter(size);
    let control_size = iced::Size::new(diameter, diameter);
    let controls = center::view(state, diameter);
    let board_size = iced::Size::new((size.width - 32.0).max(0.0), (size.height - 32.0).max(0.0));
    let board = layout::render_table(&state.table_layout, |idx| {
        let tile = state.table_layout.seat_bounds(idx, board_size);
        // Caption canvases start inside the tile's 3px border padding.
        let avoid = iced::Rectangle {
            x: (board_size.width - control_size.width) / 2.0 - tile.x - 3.0,
            y: (board_size.height - control_size.height) / 2.0 - tile.y - 3.0,
            width: control_size.width,
            height: control_size.height,
        };
        seat_panel(idx, state, image_cache, avoid)
    });
    container(stack![
        board,
        container(controls)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
    ])
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// A big number with the whole left half acting as a "-" zone and the whole
/// right half as a "+" zone, so you don't have to hit a small button. Tap
/// for +/-1; hold for +/-10, repeating every couple of seconds while held.
/// A huge centered number over full-tile-height, mostly-invisible left/right
/// tap zones - the left half subtracts, the right half adds. Tap for +/-1;
/// hold for +/-10, repeating while held. Dragging past the threshold
/// cancels the tap: up opens the seat's action menu, sideways starts
/// logging commander damage against that seat.
fn split_counter<'a>(
    seat: usize,
    value: i32,
    target: CounterTarget,
    facing: SeatOrientation,
) -> Element<'a, Message> {
    let zone = |sign: i32, glyph: &'static str| {
        // The glyph sits on its own frosted chip so it reads over bright
        // art; the tap area is still the whole half of the tile.
        let chip = container(text(glyph).size(style::T_TITLE).color(style::TEXT))
            .width(Length::Fixed(84.0))
            .height(Length::Fixed(84.0))
            .center_x(Length::Fixed(84.0))
            .center_y(Length::Fixed(84.0))
            .style(style::glass_round);

        mouse_area(
            container(chip)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .on_press(Message::Game(GameMessage::CounterPressStart(
            seat, target, sign,
        )))
        .on_move(move |point| {
            Message::Game(GameMessage::CounterPressMove(
                target, sign, point.x, point.y,
            ))
        })
        .on_release(Message::Game(GameMessage::CounterPressEnd(target, sign)))
    };

    // Minus always sits at the player's own left hand, which is not the
    // screen's. Someone sitting opposite reaches for the screen's right when
    // they mean their left, so their halves swap; at the heads of the table
    // their left and right run up and down the screen, so the tile splits
    // top/bottom instead of left/right.
    let minus = "\u{2212}";
    let zones: Element<Message> = match facing {
        SeatOrientation::Upright => row![zone(-1, minus), zone(1, "+")]
            .spacing(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        SeatOrientation::UpsideDown => row![zone(1, "+"), zone(-1, minus)]
            .spacing(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        // Head seats read down the screen, so their left is the top.
        SeatOrientation::LeftHead => column![zone(-1, minus), zone(1, "+")]
            .spacing(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        // ...and the far head reads up it, so their left is the bottom.
        SeatOrientation::RightHead => column![zone(1, "+"), zone(-1, minus)]
            .spacing(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    };

    let number = rotated::strong_chip(
        vec![Line::new(value.to_string(), style::T_COUNTER as f32)],
        facing,
    );

    stack![zones, number]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// What a seat's counter currently shows: the value, what it edits, and a
/// optional context for special counters (poison or commander damage).
fn active_counter(index: usize, state: &GameState) -> (i32, CounterTarget, String) {
    if let Some(focus) = state.damage_focus {
        if focus != index {
            let slot = state.damage_slot(index);
            let amount = state.seats[focus].damage_from(index, slot);
            let target_name = state.seats[focus].player.name.clone();
            // Name the specific commander: with a partner pair, "damage to
            // Will" is ambiguous and each one has its own lethal 21.
            let source_name = state.seats[index].commander_in(slot).name.clone();
            return (
                amount,
                CounterTarget::Damage(focus, index, slot),
                format!(
                    "{source_name} \u{00b7} to {target_name} (lethal at {LETHAL_COMMANDER_DAMAGE})"
                ),
            );
        }
    }
    let seat = &state.seats[index];
    match state.seat_tab[index] {
        SeatTab::Life => (seat.life, CounterTarget::Life(index), String::new()),
        SeatTab::Poison => (
            seat.poison,
            CounterTarget::Poison(index),
            format!("Poison (lethal at {LETHAL_POISON})"),
        ),
    }
}

/// The swipe-up reveal: pick a view (Life / Poison) or fire an action (Mark
/// Out, Declare Winner) for this seat.
///
/// Drawn as a turned canvas rather than a column of buttons: it belongs to
/// one player, and a menu that reads upside down to them is no use at all.
fn action_menu(index: usize, seat: &Seat, facing: SeatOrientation) -> Element<'_, Message> {
    rotated::menu(
        vec![
            (
                Glyph::Heart,
                "Life".to_string(),
                Message::Game(GameMessage::SwitchTab(index, SeatTab::Life)),
            ),
            (
                Glyph::Poison,
                "Poison".to_string(),
                Message::Game(GameMessage::SwitchTab(index, SeatTab::Poison)),
            ),
            (
                if seat.eliminated {
                    Glyph::Play
                } else {
                    Glyph::Close
                },
                if seat.eliminated {
                    "Back In"
                } else {
                    "Mark Out"
                }
                .to_string(),
                Message::Game(GameMessage::ToggleEliminated(index)),
            ),
            (
                Glyph::Trophy,
                "Declare Winner".to_string(),
                Message::Game(GameMessage::StartDeclareWinner(index)),
            ),
            (
                Glyph::Back,
                "Back to game".to_string(),
                Message::Game(GameMessage::CloseActionMenu),
            ),
        ],
        facing,
    )
}

fn seat_panel<'a>(
    index: usize,
    state: &'a GameState,
    image_cache: &'a HashMap<String, image::Handle>,
    avoid: iced::Rectangle,
) -> Element<'a, Message> {
    let seat = &state.seats[index];
    let is_active = state.active_seat == index;

    // `eliminated` alone gates this, not a live re-check of the stats:
    // otherwise tapping "Back In" while poison/damage is still at a lethal
    // number would immediately re-flag them as out on the very next render.
    if seat.eliminated {
        // The tile is the only place the record is visible mid-game, so it
        // carries the whole thing: what happened, when, and to whom.
        let out_line = seat.elimination.map(|out| match out.killer_seat {
            Some(killer) => format!(
                "{} on turn {}, to {}",
                out.cause.past_tense(),
                out.turn,
                state.seats[killer].player.name
            ),
            None => format!("{} on turn {}", out.cause.past_tense(), out.turn),
        });
        return eliminated_tile(
            index,
            seat,
            state.table_layout.seat_orientation(index),
            out_line,
            image_cache,
        );
    }

    // Everything a player reads on their own tile is turned to face them,
    // since they're sitting round the table rather than behind the screen.
    let facing = state.table_layout.seat_orientation(index);
    let art = if state.damage_focus.is_some() {
        // Show the actual source commander while logging damage; partner
        // selector chips already identify both cards and need clear space.
        art::framed(
            seat.commander_in(state.damage_slot(index)),
            image_cache,
            18,
            facing.radians(),
        )
    } else {
        art::framed_pair(
            &seat.commander,
            seat.partner.as_ref(),
            image_cache,
            18,
            facing.radians(),
        )
    };

    let (value, target, subtitle) = active_counter(index, state);

    let mut identity = seat.commander.color_identity.clone();
    if let Some(partner) = &seat.partner {
        identity.push_str(&partner.color_identity);
    }
    let mut caption_lines = vec![Line::new(
        seat.player.name.clone(),
        style::T_PLAYER_NAME as f32,
    )];
    if !subtitle.is_empty() {
        caption_lines.push(Line::new(subtitle, style::T_CAPTION as f32).secondary());
    }
    caption_lines.push(Line::mana(&identity));
    let caption = rotated::identity_chip(caption_lines, facing, avoid);

    let hate_chip = rotated::action_button(
        Glyph::Shield,
        "Commander hate",
        "",
        facing,
        rotated::EdgeAlign::Start,
        false,
        Message::Game(GameMessage::StartHate(index)),
    );
    let end_turn_chip: Option<Element<Message>> = is_active.then(|| {
        rotated::action_button(
            Glyph::Next,
            "End turn",
            &format!("Turn {}", state.turn_number),
            facing,
            rotated::EdgeAlign::End,
            true,
            Message::Game(GameMessage::NextTurn),
        )
    });

    // While commander damage is being logged against someone else, a seat
    // running a partner pair has to say WHICH commander connected - each
    // tracks its own 21. Single-commander seats never see this.
    //
    // The two chips sit at either hand on the player's own edge, turned to
    // face them, for the same reason everything else on a tile does. They
    // used to be a plain row pinned to the bottom of the tile in *screen*
    // space, which for every seat along the top of the table put them
    // straight underneath the Done button floating dead centre of the
    // board. The player's own edge is the one part of a tile that can never
    // reach the middle of the table.
    let damage_source = state
        .damage_focus
        .filter(|focus| *focus != index && seat.partner.is_some());

    let partner_chips: Vec<Element<Message>> = match damage_source {
        Some(focus) => {
            let selected = state.damage_slot(index);
            [
                (PRIMARY, rotated::EdgeAlign::Start),
                (PARTNER, rotated::EdgeAlign::End),
            ]
            .into_iter()
            .map(|(slot, align)| {
                // The short name is what people say out loud, and two of
                // them on one edge is what makes swapping a glance rather
                // than a read. The running total under each says which of
                // the pair is actually close to lethal.
                let commander = seat.commander_in(slot);
                let dealt = state.seats[focus].damage_from(index, slot);
                let paint = if slot == selected {
                    style::accent_chip_paint()
                } else {
                    style::glass_paint()
                };
                rotated::edge_button(
                    vec![
                        Line::new(cards::first_word(&commander.name), style::T_LABEL as f32),
                        Line::new(format!("{dealt} dealt"), style::T_CAPTION as f32),
                    ],
                    facing,
                    align,
                    paint,
                    Message::Game(GameMessage::SelectDamageSlot(index, slot)),
                )
            })
            .collect()
        }
        None => Vec::new(),
    };

    let card: Element<Message> = if state.action_menu_for == Some(index) {
        // The scrim is a plain container and passes taps straight through;
        // the menu canvas above it is what actually swallows them.
        let cover = container(iced::widget::horizontal_space())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::scrim);
        stack![art, cover, action_menu(index, seat, facing)].into()
    } else {
        // The chips float above the counter zones so they capture their own
        // taps, and each one only claims the rectangle it actually draws -
        // everywhere else the tap falls straight through to the counter.
        let mut card = stack![art, split_counter(index, value, target, facing), caption];
        if partner_chips.is_empty() {
            card = card.push(hate_chip);
            if let Some(end_turn) = end_turn_chip {
                card = card.push(end_turn);
            }
        } else {
            // The pair takes both ends of the edge, so hate and end-turn
            // stand down for as long as the damage is being logged. Neither
            // is what you reached for mid-count, and three chips a side is
            // how a long commander name ends up under its neighbour.
            for chip in partner_chips {
                card = card.push(chip);
            }
        }
        card.into()
    };

    let style_fn: fn(&iced::Theme) -> container::Style = if is_active {
        style::panel_active
    } else {
        style::panel
    };

    container(card)
        .padding(3)
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
    facing: SeatOrientation,
    out_line: Option<String>,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let art = art::framed(&seat.commander, image_cache, 16, facing.radians());

    let scrim = container(
        scrollable(
            column![
                text("ELIMINATED").size(style::T_HEADING),
                text(seat.player.name.clone()).size(style::T_SUBHEAD),
                text(seat.commander.name.clone()).size(style::T_BODY),
                text(out_line.unwrap_or_else(|| "out".to_string())).size(style::T_BODY),
                text(format!("Final: {} life, {} poison", seat.life, seat.poison))
                    .size(style::T_CAPTION),
                style::touch_button("Rate this game", style::T_ACTION)
                    .width(Length::Fixed(220.0))
                    .style(style::primary)
                    .on_press(Message::Game(GameMessage::OpenFeedback(index))),
                style::touch_button("Back In", style::T_ACTION)
                    .width(Length::Fixed(220.0))
                    .style(style::secondary)
                    .on_press(Message::Game(GameMessage::ToggleEliminated(index))),
            ]
            .spacing(14)
            .align_x(iced::Alignment::Center),
        )
        .height(Length::Fill),
    )
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .style(style::scrim);

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
            text(format!("{} is at {} life.", s.player.name, s.life)).size(style::T_TITLE),
            text("Are they out?").size(style::T_HEADING),
            text("Some effects keep a player from losing at 0 or below - say no to keep them in.")
                .size(style::T_BODY),
            row![
                style::cta_button("Yes, they're out", style::T_SUBHEAD)
                    .width(Length::Fixed(380.0))
                    .style(style::danger)
                    .on_press(Message::Game(GameMessage::AnswerZeroLifeCheck(true))),
                style::cta_button("No, keep playing", style::T_SUBHEAD)
                    .width(Length::Fixed(380.0))
                    .style(style::secondary)
                    .on_press(Message::Game(GameMessage::AnswerZeroLifeCheck(false))),
            ]
            .spacing(20),
        ]
        .spacing(26)
        .align_x(iced::Alignment::Center)
        .padding(30),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// Shared touch card for a reason or player choice.
fn decision_option<'a>(
    glyph: Glyph,
    title: String,
    detail: String,
    selected: bool,
    message: GameMessage,
) -> Element<'a, Message> {
    button(
        row![
            crate::icon::view(
                glyph,
                32.0,
                if selected {
                    style::TEXT
                } else {
                    style::ACCENT_BRIGHT
                }
            ),
            column![
                text(title).size(style::T_ACTION),
                text(detail).size(style::T_CAPTION).color(if selected {
                    style::TEXT
                } else {
                    style::TEXT_MUTED
                })
            ]
            .spacing(6)
            .width(Length::Fill),
            crate::icon::view(
                if selected { Glyph::Check } else { Glyph::Next },
                24.0,
                if selected {
                    style::TEXT
                } else {
                    style::ACCENT_BRIGHT
                }
            ),
        ]
        .spacing(style::GAP)
        .align_y(iced::Alignment::Center),
    )
    .padding(24)
    .width(Length::Fill)
    .height(128)
    .style(if selected {
        style::primary
    } else {
        style::secondary
    })
    .on_press(Message::Game(message))
    .into()
}

/// Bounded two-column choices, with the footer kept outside the scroll area.
fn decision_page<'a>(
    glyph: Glyph,
    heading: String,
    subtitle: String,
    options: Vec<Element<'a, Message>>,
    cancel: GameMessage,
    confirm: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut iter = options.into_iter();
    let mut rows = Vec::new();
    while let Some(first) = iter.next() {
        let mut pair = row![first].spacing(style::GAP);
        pair = pair.push(
            iter.next()
                .unwrap_or_else(|| iced::widget::horizontal_space().into()),
        );
        rows.push(pair.into());
    }
    let footer = row![
        style::icon_button(Glyph::Back, "Back to game", style::T_ACTION)
            .width(220)
            .style(style::secondary)
            .on_press(Message::Game(cancel)),
        iced::widget::horizontal_space(),
        confirm.unwrap_or_else(|| iced::widget::Space::new(0, 0).into()),
    ]
    .align_y(iced::Alignment::Center);
    container(
        container(
            column![
                container(
                    row![
                        crate::icon::view(glyph, 40.0, style::ACCENT_BRIGHT),
                        column![
                            text(heading).size(style::T_TITLE),
                            text(subtitle).size(style::T_BODY).color(style::TEXT_MUTED)
                        ]
                        .spacing(8),
                    ]
                    .spacing(24)
                    .align_y(iced::Alignment::Center)
                )
                .padding(24)
                .width(Length::Fill)
                .style(style::panel),
                scrollable(column(rows).spacing(style::GAP)).height(Length::Fill),
                footer,
            ]
            .spacing(24),
        )
        .max_width(1180)
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .padding(32)
    .center_x(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn out_view(state: &GameState, flow: OutFlow) -> Element<'_, Message> {
    let seat = &state.seats[flow.seat];
    let (heading, subtitle, options) = match flow.cause {
        None => (
            format!("Mark {} out", seat.player.name),
            format!("Turn {} · Choose what happened", state.turn_number),
            OutCause::CHOOSABLE
                .iter()
                .map(|cause| {
                    decision_option(
                        if *cause == OutCause::Concede {
                            Glyph::Back
                        } else {
                            Glyph::Close
                        },
                        cause.label().to_string(),
                        if *cause == OutCause::Concede {
                            "This player chose to leave the game"
                        } else {
                            "An effect or another condition put them out"
                        }
                        .to_string(),
                        false,
                        GameMessage::PickOutCause(*cause),
                    )
                })
                .collect(),
        ),
        Some(cause) => {
            let mut options: Vec<_> = state
                .seats
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != flow.seat)
                .map(|(j, other)| {
                    decision_option(
                        Glyph::Players,
                        other.player.name.clone(),
                        other.deck_name(),
                        false,
                        GameMessage::ConfirmOutKiller(Some(j)),
                    )
                })
                .collect();
            options.push(decision_option(
                Glyph::Close,
                "Nobody".into(),
                "No player gets credit for this elimination".into(),
                false,
                GameMessage::ConfirmOutKiller(None),
            ));
            (
                format!("Who eliminated {}?", seat.player.name),
                format!("{} · Turn {}", cause.label(), state.turn_number),
                options,
            )
        }
    };
    decision_page(
        Glyph::Close,
        heading,
        subtitle,
        options,
        GameMessage::CancelOut,
        None,
    )
}

/// Two steps: what kind of hate landed on this seat, then who's responsible.
fn hate_view(state: &GameState, flow: HateFlow) -> Element<'_, Message> {
    let victim = &state.seats[flow.victim];
    let (heading, options) = match flow.kind {
        None => (
            format!("What happened to {}?", victim.player.name),
            HateKind::ALL
                .iter()
                .map(|kind| {
                    decision_option(
                        Glyph::Shield,
                        kind.label().into(),
                        "Log a commander event".into(),
                        false,
                        GameMessage::PickHateKind(*kind),
                    )
                })
                .collect(),
        ),
        Some(kind) => (
            format!("{} · Who did it?", kind.label()),
            state
                .seats
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != flow.victim)
                .map(|(j, other)| {
                    decision_option(
                        Glyph::Players,
                        other.player.name.clone(),
                        other.deck_name(),
                        false,
                        GameMessage::ConfirmHate(Some(j)),
                    )
                })
                .collect(),
        ),
    };
    decision_page(
        Glyph::Shield,
        heading,
        victim.deck_name(),
        options,
        GameMessage::CancelHate,
        None,
    )
}

fn declare_winner_view(state: &GameState, winner: usize) -> Element<'_, Message> {
    let seat = &state.seats[winner];
    let reasons = WinReason::ALL
        .iter()
        .map(|reason| {
            let (glyph, detail) = match reason {
                WinReason::CommanderDamage => (Glyph::Shield, "Lethal damage from a commander"),
                WinReason::Poison => (Glyph::Poison, "Opponents reached lethal poison"),
                WinReason::CombatDamage => (Glyph::Heart, "Combat damage closed out the game"),
                WinReason::InfiniteCombo => {
                    (Glyph::Rotate(true), "A repeating combo secured the win")
                }
                WinReason::Concede => (Glyph::Back, "The remaining opponents conceded"),
                WinReason::Other => (Glyph::Trophy, "Another effect or win condition"),
            };
            decision_option(
                glyph,
                reason.label().into(),
                detail.into(),
                state.pending_reason == Some(*reason),
                GameMessage::PickWinReason(*reason),
            )
        })
        .collect();
    let mut confirm = style::icon_button(Glyph::Check, "Confirm & save", style::T_ACTION)
        .width(260)
        .style(style::primary);
    if state.pending_reason.is_some() {
        confirm = confirm.on_press(Message::Game(GameMessage::ConfirmEndGame));
    }
    decision_page(
        Glyph::Trophy,
        format!("{} wins", seat.player.name),
        format!("{} · Choose how the game ended", seat.deck_name()),
        reasons,
        GameMessage::CancelDeclareWinner,
        Some(confirm.into()),
    )
}

#[cfg(test)]
mod elimination_tests {
    use super::*;
    use crate::model::{ArtFraming, Commander, Player};

    fn seat(id: i64, name: &str) -> Seat {
        Seat::new(
            Player {
                id,
                name: name.to_string(),
            },
            Commander {
                id,
                oracle_id: format!("oracle-{id}"),
                name: format!("{name}'s commander"),
                image_url: None,
                art_crop_url: None,
                color_identity: String::new(),
                framing: ArtFraming::default(),
            },
            40,
        )
    }

    /// These messages never touch the database, but `update` takes one, so
    /// the tests hand it a throwaway in-memory connection rather than
    /// carving out a separate code path for the tests to exercise.
    fn update_for_test(state: &mut GameState, message: GameMessage) {
        let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        db::init(&conn).unwrap();
        let _ = update(state, &mut conn, message);
    }

    fn game() -> GameState {
        let seats = vec![seat(1, "Ada"), seat(2, "Bo"), seat(3, "Cy"), seat(4, "Di")];
        let layout = layout::options_for(4).into_iter().next().unwrap();
        GameState::new(seats, layout, vec![0, 1, 2, 3])
    }

    #[test]
    fn turn_count_follows_each_player_and_timer_resets_each_handoff() {
        let base = game();
        let order = vec![2, 0, 3, 1];
        let mut state = GameState::new(base.seats, base.table_layout, order.clone());
        state.game_seconds = 500;
        for turn in 1..=3 {
            for &seat in &order {
                assert_eq!(state.active_seat, seat);
                assert_eq!(state.turn_number, turn);
                state.turn_seconds = 45;
                update_for_test(&mut state, GameMessage::NextTurn);
                assert_eq!(state.turn_seconds, 0);
                assert_eq!(state.game_seconds, 500);
            }
        }
        assert_eq!(state.turn_number, 4);
    }

    #[test]
    fn eliminating_the_first_player_does_not_stop_turn_counting() {
        let mut state = game();
        state.seats[0].eliminated = true;
        for turn in 1..=3 {
            for seat in 1..4 {
                update_for_test(&mut state, GameMessage::NextTurn);
                assert_eq!(state.active_seat, seat);
                assert_eq!(state.turn_number, turn);
            }
        }
        state.seats[0].eliminated = false;
        update_for_test(&mut state, GameMessage::NextTurn);
        assert_eq!(state.active_seat, 0);
        assert_eq!(
            state.turn_number, 2,
            "returning player resumes their own count"
        );
    }

    #[test]
    fn game_menu_and_pause_preserve_the_game() {
        let mut state = game();
        state.game_seconds = 123;
        update_for_test(&mut state, GameMessage::OpenGameMenu);
        assert!(state.game_menu_open);
        update_for_test(&mut state, GameMessage::TogglePause);
        assert!(state.paused);
        assert!(!state.game_menu_open);
        update_for_test(&mut state, GameMessage::Tick);
        assert_eq!(state.game_seconds, 123);
        update_for_test(&mut state, GameMessage::OpenGameMenu);
        update_for_test(&mut state, GameMessage::TogglePause);
        update_for_test(&mut state, GameMessage::Tick);
        assert_eq!(state.game_seconds, 124);
        assert_eq!(state.seats[0].life, 40);
    }

    #[test]
    fn abandoning_requires_confirmation_and_cancel_keeps_state() {
        let mut state = game();
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        db::init(&conn).unwrap();
        assert!(update(&mut state, &mut conn, GameMessage::AbandonGame)
            .1
            .is_none());
        update_for_test(&mut state, GameMessage::RequestAbandon);
        update_for_test(&mut state, GameMessage::CancelAbandon);
        assert!(!state.pending_abandon);
        assert!(update(&mut state, &mut conn, GameMessage::AbandonGame)
            .1
            .is_none());
        assert_eq!(state.seats.len(), 4);
        update_for_test(&mut state, GameMessage::RequestAbandon);
        assert!(matches!(
            update(&mut state, &mut conn, GameMessage::AbandonGame).1,
            Some(Action::Abandoned)
        ));
    }

    /// Lethal commander damage names its own killer: the commander that
    /// dealt the 21st point is right there on the table, so nobody is asked.
    #[test]
    fn lethal_commander_damage_credits_the_source() {
        let mut state = game();
        state.turn_number = 7;
        apply_damage_delta(&mut state, 0, 2, PRIMARY, LETHAL_COMMANDER_DAMAGE);

        let out = state.seats[0].elimination.expect("seat 0 should be out");
        assert_eq!(out.cause, OutCause::CommanderDamage);
        assert_eq!(out.killer_seat, Some(2));
        assert_eq!(out.turn, 7);
        assert!(state.out_flow.is_none(), "it should not have asked");
    }

    /// Poison is just as lethal, but any seat could have put those counters
    /// on - so it opens the prompt instead of guessing.
    #[test]
    fn lethal_poison_asks_who() {
        let mut state = game();
        state.turn_number = 3;
        apply_poison_delta(&mut state, 1, LETHAL_POISON);

        assert!(
            state.seats[1].elimination.is_none(),
            "nothing is recorded until the question is answered"
        );
        let flow = state.out_flow.expect("should be asking");
        assert_eq!(flow.seat, 1);
        assert_eq!(flow.cause, Some(OutCause::Poison));

        update_for_test(&mut state, GameMessage::ConfirmOutKiller(Some(3)));
        let out = state.seats[1].elimination.expect("seat 1 should be out");
        assert_eq!(out.cause, OutCause::Poison);
        assert_eq!(out.killer_seat, Some(3));
        assert_eq!(out.turn, 3);
    }

    /// Nobody is a real answer, not a way of cancelling the prompt.
    #[test]
    fn nobody_is_a_recorded_answer() {
        let mut state = game();
        state.turn_number = 9;
        state.out_flow = Some(OutFlow {
            seat: 2,
            cause: Some(OutCause::LifeLoss),
        });
        update_for_test(&mut state, GameMessage::ConfirmOutKiller(None));

        let out = state.seats[2].elimination.expect("seat 2 should be out");
        assert_eq!(out.cause, OutCause::LifeLoss);
        assert_eq!(out.killer_seat, None);
        assert_eq!(out.turn, 9);
    }

    /// A concede skips the killer question entirely.
    #[test]
    fn a_concede_has_no_killer() {
        let mut state = game();
        state.turn_number = 2;
        update_for_test(&mut state, GameMessage::ToggleEliminated(3));
        assert_eq!(
            state.out_flow.map(|f| f.cause),
            Some(None),
            "marking out by hand asks why first"
        );

        update_for_test(&mut state, GameMessage::PickOutCause(OutCause::Concede));
        assert!(state.out_flow.is_none(), "concede should not ask who");
        let out = state.seats[3].elimination.expect("seat 3 should be out");
        assert_eq!(out.cause, OutCause::Concede);
        assert_eq!(out.killer_seat, None);
        assert_eq!(out.turn, 2);
    }

    /// Picking "Other" is the one manual cause that does ask.
    #[test]
    fn other_asks_who() {
        let mut state = game();
        update_for_test(&mut state, GameMessage::ToggleEliminated(0));
        update_for_test(&mut state, GameMessage::PickOutCause(OutCause::Other));

        assert_eq!(
            state.out_flow.map(|f| f.cause),
            Some(Some(OutCause::Other)),
            "other should move on to the killer question"
        );
    }

    /// Bringing someone back in drops the record with them - they did not
    /// die on that turn after all.
    #[test]
    fn coming_back_in_clears_the_record() {
        let mut state = game();
        state.mark_out(1, OutCause::Concede, None);
        assert!(state.seats[1].elimination.is_some());

        update_for_test(&mut state, GameMessage::ToggleEliminated(1));
        assert!(!state.seats[1].eliminated);
        assert!(state.seats[1].elimination.is_none());
    }
}
