use std::collections::HashMap;

use iced::widget::{button, column, container, image, row, scrollable, stack, text, text_input};
use iced::{Alignment, ContentFit, Element, Length, Task};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::layout::{self, SeatOrientation, TableLayout, TurnDirection};
use crate::art;
use crate::model::{
    ArtFraming, Commander, Player, SavedDeck, Seat, PARTNER, PRIMARY, STARTING_LIFE,
};
use crate::panned_image;
use crate::scryfall::{self, Cooldown, ScryfallCard, ScryfallError};
use crate::style;

pub const MIN_POD: usize = 2;
pub const MAX_POD: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStage {
    ChoosePodSize,
    ChooseLayout,
    Grid,
    /// Last step before the game: who leads, and which way turns pass.
    TurnOrder,
}

/// The commander a player picked by name; art is chosen next, but this
/// identity (oracle id) is what stats will key on regardless of which
/// printing's art ends up chosen.
#[derive(Debug, Clone)]
pub struct ArtTarget {
    pub oracle_id: String,
    pub name: String,
    pub color_identity: String,
}

#[derive(Debug, Clone, Default)]
pub struct SeatSetup {
    pub player: Option<Player>,
    pub commander: Option<Commander>,
    /// The second commander of a partner pair. Optional and unvalidated -
    /// the table knows its own rules, so any commander can be paired.
    pub partner: Option<Commander>,
    /// Commanders this specific player has piloted before - personal to
    /// them, never shared with the rest of the pod.
    pub commander_history: Vec<SavedDeck>,
}

impl SeatSetup {
    pub fn commander_in(&self, slot: usize) -> Option<&Commander> {
        match slot {
            PARTNER => self.partner.as_ref(),
            _ => self.commander.as_ref(),
        }
    }

    pub fn set_commander_in(&mut self, slot: usize, commander: Option<Commander>) {
        match slot {
            PARTNER => self.partner = commander,
            _ => self.commander = commander,
        }
    }
}

pub struct SetupState {
    pub stage: SetupStage,
    pub pod_size: usize,
    pub table_layout: Option<TableLayout>,
    pub seats: Vec<SeatSetup>,
    pub editing_seat: Option<usize>,
    pub new_player_name: String,
    pub commander_query: String,
    pub commander_results: Vec<ScryfallCard>,
    pub searching: bool,
    pub art_target: Option<ArtTarget>,
    pub art_options: Vec<ScryfallCard>,
    pub loading_art_options: bool,
    /// Set while the player is framing a seat's art (zoom + anchor).
    pub framing: bool,
    /// Non-zero while Scryfall has us locked out for exceeding the rate
    /// limit; no search or art lookup is sent until it runs back down.
    pub cooldown: Cooldown,
    /// Which of the edited seat's commanders the picker is filling: PRIMARY
    /// normally, PARTNER while adding or changing a partner.
    pub editing_slot: usize,
    /// The seat that takes the first turn. None until someone is picked, so
    /// the game can't start on an arbitrary default.
    pub first_seat: Option<usize>,
    pub turn_direction: TurnDirection,
    pub error: Option<String>,
}

impl SetupState {
    pub fn new() -> Self {
        Self {
            stage: SetupStage::ChoosePodSize,
            pod_size: 0,
            table_layout: None,
            seats: Vec::new(),
            editing_seat: None,
            new_player_name: String::new(),
            commander_query: String::new(),
            commander_results: Vec::new(),
            searching: false,
            art_target: None,
            art_options: Vec::new(),
            loading_art_options: false,
            framing: false,
            cooldown: Cooldown::default(),
            editing_slot: PRIMARY,
            first_seat: None,
            turn_direction: TurnDirection::Clockwise,
            error: None,
        }
    }

    pub fn all_seats_ready(&self) -> bool {
        !self.seats.is_empty()
            && self
                .seats
                .iter()
                .all(|s| s.player.is_some() && s.commander.is_some())
    }

    fn players_taken_by_other_seats(&self) -> std::collections::HashSet<i64> {
        let editing = self.editing_seat;
        self.seats
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != editing)
            .filter_map(|(_, s)| s.player.as_ref().map(|p| p.id))
            .collect()
    }

    fn clear_editor_fields(&mut self) {
        self.new_player_name.clear();
        self.commander_query.clear();
        self.commander_results.clear();
        self.art_target = None;
        self.art_options.clear();
        self.framing = false;
    }
}

#[derive(Debug, Clone)]
pub enum SetupMessage {
    ChoosePodSize(usize),
    BackToPodSizeChoice,
    ChooseLayout(TableLayout),
    BackToLayoutChoice,
    EditSeat(usize),
    BackToGrid,
    NewPlayerNameChanged(String),
    CreatePlayer,
    PickExistingPlayer(Player),
    ClearSeatPlayer,
    CommanderQueryChanged(String),
    SearchCommanders,
    SearchResults(Result<Vec<ScryfallCard>, ScryfallError>),
    PickCommanderName(ScryfallCard),
    PickHistoryCommander(SavedDeck),
    ArtOptionsLoaded(Result<Vec<ScryfallCard>, ScryfallError>),
    PickArt(ScryfallCard),
    CancelArtPick,
    /// (which of the seat's commanders to restyle)
    ChangeArt(usize),
    StartFraming(usize),
    AddPartner,
    /// Fired continuously while the art is being dragged or pinched.
    FramingChanged(ArtFraming),
    DoneFraming,
    /// (which of the seat's commanders to clear)
    ClearSeatCommander(usize),
    CooldownTick,
    ReviewTurnOrder,
    BackToGridFromTurnOrder,
    ChooseFirstSeat(usize),
    RandomFirstSeat,
    SetTurnDirection(TurnDirection),
    StartGame,
}

pub enum Action {
    /// Seats, the table they're sitting at, and the seat indices in the
    /// order they'll take turns (first player first).
    StartGame(Vec<Seat>, TableLayout, Vec<usize>),
}

/// The order the pod will actually play in, or None until a first player is
/// chosen. Shared by the turn-order screen and the start handler so the
/// preview can never disagree with what the game uses.
fn planned_turn_order(state: &SetupState) -> Option<Vec<usize>> {
    let table = state.table_layout.as_ref()?;
    let first = state.first_seat?;
    Some(table.turn_order(first, state.turn_direction))
}

fn load_portrait_task(commander: &Commander) -> Task<Message> {
    match commander.portrait_url() {
        Some(url) => {
            let url = url.to_string();
            let key = url.clone();
            Task::perform(scryfall::fetch_image(url), move |res| {
                Message::ArtLoaded(key.clone(), res)
            })
        }
        None => Task::none(),
    }
}

/// Loads the framing this commander was last given in this exact tile, so
/// art placed into a seat already looks the way it was left rather than
/// reverting to a bare centre crop.
fn resolve_framing(
    state: &SetupState,
    conn: &Connection,
    seat: usize,
    mut commander: Commander,
) -> Commander {
    if let Some(layout) = &state.table_layout {
        commander.framing = db::load_framing(conn, commander.id, &layout.name, seat);
    }
    commander
}

fn current_commander_mut(state: &mut SetupState) -> Option<&mut Commander> {
    let seat = state.editing_seat?;
    let slot = state.editing_slot;
    let seat = state.seats.get_mut(seat)?;
    match slot {
        PARTNER => seat.partner.as_mut(),
        _ => seat.commander.as_mut(),
    }
}

fn load_prints_task(oracle_id: String) -> Task<Message> {
    Task::perform(scryfall::fetch_prints(oracle_id), |res| {
        Message::Setup(SetupMessage::ArtOptionsLoaded(res))
    })
}

pub fn update(
    state: &mut SetupState,
    conn: &Connection,
    message: SetupMessage,
) -> (Task<Message>, Option<Action>) {
    state.error = None;
    match message {
        SetupMessage::ChoosePodSize(n) => {
            state.pod_size = n;
            state.seats = vec![SeatSetup::default(); n];
            state.table_layout = None;
            // The seats are new, so a previously chosen leader would point
            // at someone who is no longer at the table.
            state.first_seat = None;
            state.stage = SetupStage::ChooseLayout;
            (Task::none(), None)
        }
        SetupMessage::BackToPodSizeChoice => {
            state.stage = SetupStage::ChoosePodSize;
            state.editing_seat = None;
            state.clear_editor_fields();
            (Task::none(), None)
        }
        SetupMessage::ChooseLayout(layout) => {
            state.table_layout = Some(layout);
            state.stage = SetupStage::Grid;
            // Tiles change shape with the layout, and framing is stored per
            // layout and seat, so every placed commander needs its framing
            // for the NEW tile rather than the one it was framed in.
            for seat_index in 0..state.seats.len() {
                for slot in [PRIMARY, PARTNER] {
                    if let Some(commander) = state.seats[seat_index].commander_in(slot).cloned() {
                        let reframed = resolve_framing(state, conn, seat_index, commander);
                        state.seats[seat_index].set_commander_in(slot, Some(reframed));
                    }
                }
            }
            (Task::none(), None)
        }
        SetupMessage::BackToLayoutChoice => {
            state.stage = SetupStage::ChooseLayout;
            state.editing_seat = None;
            state.clear_editor_fields();
            (Task::none(), None)
        }
        SetupMessage::EditSeat(i) => {
            state.editing_seat = Some(i);
            state.editing_slot = PRIMARY;
            state.clear_editor_fields();
            (Task::none(), None)
        }
        SetupMessage::BackToGrid => {
            state.editing_seat = None;
            state.editing_slot = PRIMARY;
            state.clear_editor_fields();
            (Task::none(), None)
        }
        SetupMessage::NewPlayerNameChanged(s) => {
            state.new_player_name = s;
            (Task::none(), None)
        }
        SetupMessage::CreatePlayer => {
            let name = state.new_player_name.trim().to_string();
            if name.is_empty() {
                return (Task::none(), None);
            }
            let Some(seat) = state.editing_seat else {
                return (Task::none(), None);
            };
            match db::create_player(conn, &name) {
                Ok(player) => {
                    state.seats[seat].player = Some(player);
                    state.seats[seat].commander_history = Vec::new();
                    state.new_player_name.clear();
                }
                Err(e) => state.error = Some(format!("Couldn't create player: {e}")),
            }
            (Task::none(), None)
        }
        SetupMessage::PickExistingPlayer(player) => {
            let Some(seat) = state.editing_seat else {
                return (Task::none(), None);
            };
            if state.players_taken_by_other_seats().contains(&player.id) {
                state.error = Some(format!("{} is already seated at this table.", player.name));
                return (Task::none(), None);
            }
            let history = db::player_commander_history(conn, player.id).unwrap_or_default();
            state.seats[seat].player = Some(player);
            state.seats[seat].commander_history = history;
            (Task::none(), None)
        }
        SetupMessage::ClearSeatPlayer => {
            if let Some(seat) = state.editing_seat {
                state.seats[seat].player = None;
                state.seats[seat].commander_history.clear();
            }
            (Task::none(), None)
        }
        SetupMessage::CommanderQueryChanged(s) => {
            state.commander_query = s;
            (Task::none(), None)
        }
        SetupMessage::SearchCommanders => {
            let query = state.commander_query.clone();
            if query.trim().is_empty() || state.cooldown.active() {
                return (Task::none(), None);
            }
            state.searching = true;
            (
                Task::perform(scryfall::search_commanders(query), |res| {
                    Message::Setup(SetupMessage::SearchResults(res))
                }),
                None,
            )
        }
        SetupMessage::SearchResults(res) => {
            state.searching = false;
            match res {
                Ok(list) => {
                    state.commander_results = list;
                    state.error = None;
                }
                Err(e) => {
                    state.cooldown.absorb(&e);
                    state.error = Some(e.to_string());
                }
            }
            (Task::none(), None)
        }
        SetupMessage::PickCommanderName(card) => {
            state.commander_results.clear();
            state.commander_query.clear();
            state.loading_art_options = true;
            let default_thumb_task = match card.small_url.clone() {
                Some(url) => {
                    let key = url.clone();
                    Task::perform(scryfall::fetch_image(url), move |res| {
                        Message::ArtLoaded(key.clone(), res)
                    })
                }
                None => Task::none(),
            };
            let prints_task = load_prints_task(card.oracle_id.clone());
            state.art_target = Some(ArtTarget {
                oracle_id: card.oracle_id.clone(),
                name: card.name.clone(),
                color_identity: card.color_identity.clone(),
            });
            state.art_options = vec![card];
            (Task::batch([prints_task, default_thumb_task]), None)
        }
        SetupMessage::PickHistoryCommander(deck) => {
            let Some(seat) = state.editing_seat else {
                return (Task::none(), None);
            };
            if let Some(player) = &state.seats[seat].player {
                let _ = db::record_player_commander_use(conn, player.id, deck.commander.id);
                if let Some(p) = &deck.partner {
                    let _ = db::record_player_commander_use(conn, player.id, p.id);
                }
            }
            let mut tasks = vec![load_portrait_task(&deck.commander)];
            let slot = state.editing_slot;
            let commander = resolve_framing(state, conn, seat, deck.commander);
            state.seats[seat].set_commander_in(slot, Some(commander));
            // A saved pair comes as one deck: taking half of it without the
            // other half would silently drop the partner.
            if slot == PRIMARY {
                if let Some(partner) = deck.partner {
                    tasks.push(load_portrait_task(&partner));
                    let partner = resolve_framing(state, conn, seat, partner);
                    state.seats[seat].partner = Some(partner);
                }
            }
            state.editing_seat = None;
            state.editing_slot = PRIMARY;
            state.clear_editor_fields();
            (Task::batch(tasks), None)
        }
        SetupMessage::ArtOptionsLoaded(res) => {
            state.loading_art_options = false;
            match res {
                Ok(list) => {
                    let thumb_tasks: Vec<Task<Message>> = list
                        .iter()
                        .filter_map(|c| c.small_url.clone())
                        .take(20)
                        .map(|url| {
                            let key = url.clone();
                            Task::perform(scryfall::fetch_image(url), move |res| {
                                Message::ArtLoaded(key.clone(), res)
                            })
                        })
                        .collect();
                    state.art_options = list;
                    (Task::batch(thumb_tasks), None)
                }
                Err(e) => {
                    state.cooldown.absorb(&e);
                    state.error = Some(e.to_string());
                    (Task::none(), None)
                }
            }
        }
        SetupMessage::PickArt(card) => {
            let Some(target) = state.art_target.take() else {
                return (Task::none(), None);
            };
            let Some(seat) = state.editing_seat else {
                return (Task::none(), None);
            };
            state.art_options.clear();
            match db::upsert_commander(
                conn,
                &target.oracle_id,
                &target.name,
                card.image_url.as_deref(),
                card.art_crop_url.as_deref(),
                &target.color_identity,
            ) {
                Ok(commander) => {
                    if let Some(player) = &state.seats[seat].player {
                        let _ = db::record_player_commander_use(conn, player.id, commander.id);
                    }
                    let mut tasks = vec![load_portrait_task(&commander)];
                    let slot = state.editing_slot;

                    // If this player has already saved that commander as
                    // half of a pair, the other half comes with it.
                    let saved_partner = (slot == PRIMARY)
                        .then(|| state.seats[seat].player.as_ref())
                        .flatten()
                        .and_then(|player| db::saved_partner(conn, player.id, commander.id));

                    let commander = resolve_framing(state, conn, seat, commander);
                    state.seats[seat].set_commander_in(slot, Some(commander));
                    if let Some(partner) = saved_partner {
                        tasks.push(load_portrait_task(&partner));
                        let partner = resolve_framing(state, conn, seat, partner);
                        state.seats[seat].partner = Some(partner);
                    }
                    state.clear_editor_fields();
                    // Straight into framing: this is the moment the art is
                    // chosen, which is the only point framing happens.
                    state.editing_slot = slot;
                    state.framing = true;
                    (Task::batch(tasks), None)
                }
                Err(e) => {
                    state.error = Some(format!("Couldn't save commander: {e}"));
                    (Task::none(), None)
                }
            }
        }
        SetupMessage::CancelArtPick => {
            state.art_target = None;
            state.art_options.clear();
            (Task::none(), None)
        }
        SetupMessage::AddPartner => {
            state.editing_slot = PARTNER;
            state.clear_editor_fields();
            (Task::none(), None)
        }
        SetupMessage::ChangeArt(slot) => {
            state.editing_slot = slot;
            let Some(seat) = state.editing_seat else {
                return (Task::none(), None);
            };
            let Some(commander) = state.seats[seat].commander_in(slot).cloned() else {
                return (Task::none(), None);
            };
            state.art_target = Some(ArtTarget {
                oracle_id: commander.oracle_id.clone(),
                name: commander.name.clone(),
                color_identity: commander.color_identity.clone(),
            });
            state.art_options.clear();
            state.loading_art_options = true;
            (load_prints_task(commander.oracle_id), None)
        }
        SetupMessage::StartFraming(slot) => {
            state.editing_slot = slot;
            state.framing = true;
            (Task::none(), None)
        }
        SetupMessage::FramingChanged(framing) => {
            // Kept in memory while the gesture runs; written once on Done so
            // a drag isn't a few hundred database writes.
            if let Some(commander) = current_commander_mut(state) {
                commander.framing = framing.clamped();
            }
            (Task::none(), None)
        }
        SetupMessage::DoneFraming => {
            state.framing = false;
            if let (Some(seat_index), Some(layout)) = (state.editing_seat, &state.table_layout) {
                let layout_name = layout.name.clone();
                let slot = state.editing_slot;
                if let Some(commander) = state.seats[seat_index].commander_in(slot) {
                    let _ = db::save_framing(
                        conn,
                        commander.id,
                        &layout_name,
                        seat_index,
                        commander.framing,
                    );
                }
            }
            (Task::none(), None)
        }
        SetupMessage::ClearSeatCommander(slot) => {
            state.editing_slot = slot;
            if let Some(seat) = state.editing_seat {
                state.seats[seat].set_commander_in(slot, None);
                // Dropping the primary drops the partner with it: a partner
                // on its own isn't a deck, and the picker would otherwise
                // reopen on a seat that still looks half-filled.
                if slot == PRIMARY {
                    state.seats[seat].partner = None;
                }
            }
            state.art_target = None;
            state.art_options.clear();
            (Task::none(), None)
        }
        SetupMessage::CooldownTick => {
            state.cooldown.tick();
            if !state.cooldown.active() {
                state.error = None;
            }
            (Task::none(), None)
        }
        SetupMessage::ReviewTurnOrder => {
            if !state.all_seats_ready() {
                state.error = Some("Every seat needs a player and a commander.".into());
                return (Task::none(), None);
            }
            state.error = None;
            state.stage = SetupStage::TurnOrder;
            (Task::none(), None)
        }
        SetupMessage::BackToGridFromTurnOrder => {
            state.stage = SetupStage::Grid;
            state.error = None;
            (Task::none(), None)
        }
        SetupMessage::ChooseFirstSeat(seat) => {
            state.first_seat = Some(seat);
            state.error = None;
            (Task::none(), None)
        }
        SetupMessage::RandomFirstSeat => {
            // A die roll's worth of randomness for picking who leads; not
            // worth a dependency, and nothing here is adversarial.
            let n = state.seats.len();
            if n > 0 {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos() as usize)
                    .unwrap_or(0);
                state.first_seat = Some(nanos % n);
                state.error = None;
            }
            (Task::none(), None)
        }
        SetupMessage::SetTurnDirection(dir) => {
            state.turn_direction = dir;
            (Task::none(), None)
        }
        SetupMessage::StartGame => {
            let Some(layout) = state.table_layout.clone() else {
                state.error = Some("Pick a table layout first.".into());
                return (Task::none(), None);
            };
            let Some(turn_order) = planned_turn_order(state) else {
                state.error = Some("Pick who takes the first turn.".into());
                return (Task::none(), None);
            };
            if state.all_seats_ready() {
                let seats: Vec<Seat> = state
                    .seats
                    .iter()
                    .map(|s| {
                        Seat::new(
                            s.player.clone().unwrap(),
                            s.commander.clone().unwrap(),
                            STARTING_LIFE,
                        )
                        .with_partner(s.partner.clone())
                    })
                    .collect();
                (Task::none(), Some(Action::StartGame(seats, layout, turn_order)))
            } else {
                state.error = Some("Every seat needs a player and a commander.".into());
                (Task::none(), None)
            }
        }
    }
}

pub fn view<'a>(
    state: &'a SetupState,
    players_cache: &'a [Player],
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    match state.stage {
        SetupStage::ChoosePodSize => pod_size_view(state),
        SetupStage::ChooseLayout => layout_choice_view(state),
        SetupStage::TurnOrder => turn_order_view(state, image_cache),
        SetupStage::Grid => {
            if state.editing_seat.is_some() {
                editor_overlay(state, players_cache, image_cache)
            } else {
                grid_view(state, image_cache)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Wizard chrome
//
// Setup is one four-step wizard, so every step is built out of the same three
// bands: a header that says which step this is, a body that fills whatever is
// left, and a footer whose back button and primary action never move. A thumb
// that learns the footer on step one is still right on step four.
// ---------------------------------------------------------------------------

/// Pod size, table layout, seats, turn order.
const STEP_COUNT: usize = 4;

/// The page rhythm, in the pixel units padding wants. Everything on this
/// screen is one of these four numbers - the shared gap, its two sub-units
/// and double it - so there are no stray 7s and 13s.
const PAD: f32 = style::GAP as f32;
const PAD_HALF: f32 = style::GAP_SM as f32;
const PAD_TIGHT: f32 = style::GAP_XS as f32;
const PAD_DOUBLE: f32 = PAD * 2.0;

/// The vertical padding that brings a [`style::T_SUBHEAD`] text field up to
/// [`style::TOUCH_H`], so typed fields are as tappable as the buttons next to
/// them. Derived rather than guessed: iced lays text out at 1.3x its size.

/// Fixed widths, so the same kind of control is the same size everywhere:
/// the way back, the one action that moves you on, a header utility, a card
/// in a picker, and a piece of commander art.
const BACK_W: f32 = 260.0;
const CTA_W: f32 = 460.0;
const UTILITY_W: f32 = 220.0;
const CARD_W: f32 = 320.0;
const ART_W: f32 = 300.0;
const ART_H: f32 = 220.0;
/// The table diagram on the layout step, at roughly the screen's own shape
/// so the preview is a scale model rather than a squashed one.
const PREVIEW_W: f32 = 300.0;
const PREVIEW_H: f32 = 190.0;

/// "Step 2 of 4", plus a dot per step so the distance left to travel is
/// readable without counting words.
fn step_eyebrow<'a>(step: usize) -> Element<'a, Message> {
    let dots = row((1..=STEP_COUNT)
        .map(|n| {
            text("\u{2022}")
                .size(style::T_HEADING)
                .color(if n <= step { style::ACCENT_BRIGHT } else { style::SURFACE_3 })
                .into()
        })
        .collect::<Vec<Element<Message>>>())
    .spacing(PAD_HALF)
    .align_y(Alignment::Center);

    row![
        text(format!("STEP {step} OF {STEP_COUNT}"))
            .size(style::T_CAPTION)
            .color(style::ACCENT_BRIGHT),
        dots,
    ]
    .spacing(PAD)
    .align_y(Alignment::Center)
    .into()
}

/// A label in the same slot as the step counter, for the seat editor, which
/// is a detour inside step three rather than a step of its own.
fn pane_eyebrow<'a>(label: String) -> Element<'a, Message> {
    text(label)
        .size(style::T_CAPTION)
        .color(style::ACCENT_BRIGHT)
        .into()
}

/// The top band of every screen in setup: where you are, what this step is
/// called, one line on what to do, and an optional side-trip on the right.
fn step_header<'a>(
    eyebrow: Element<'a, Message>,
    title: String,
    instruction: String,
    trailing: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let titles = column![
        eyebrow,
        text(title).size(style::T_TITLE).color(style::TEXT),
        text(instruction).size(style::T_LABEL).color(style::TEXT_MUTED),
    ]
    .spacing(PAD_TIGHT);

    let mut bar = row![titles, iced::widget::horizontal_space()]
        .spacing(PAD)
        .align_y(Alignment::Center);
    for control in trailing {
        bar = bar.push(control);
    }

    container(bar)
        .padding([PAD, PAD_DOUBLE])
        .width(Length::Fill)
        .style(style::header)
        .into()
}

/// The bottom band: the way back on the left, the way on on the right, in a
/// strip as tall as the primary action so nothing shifts between steps.
fn step_footer<'a>(
    back_label: &'a str,
    back: Message,
    forward: Element<'a, Message>,
) -> Element<'a, Message> {
    row![
        style::touch_button(back_label, style::T_ACTION)
            .width(Length::Fixed(BACK_W))
            .style(style::secondary)
            .on_press(back),
        iced::widget::horizontal_space(),
        forward,
    ]
    .spacing(PAD)
    .height(Length::Fixed(style::TOUCH_H_LG))
    .align_y(Alignment::Center)
    .into()
}

/// The one primary-styled button on a step. Passing `None` leaves it visibly
/// disabled and puts the reason beside it, so a step never stalls silently.
fn forward_action(label: &str, message: Option<Message>, blocked: &str) -> Element<'static, Message> {
    let enabled = message.is_some();
    let mut cta = style::cta_button(label.to_string(), style::T_LEAD)
        .width(Length::Fixed(CTA_W))
        .style(style::primary);
    if let Some(message) = message {
        cta = cta.on_press(message);
    }

    let mut bar = row![].spacing(PAD).align_y(Alignment::Center);
    if !enabled && !blocked.is_empty() {
        bar = bar.push(
            text(blocked.to_string())
                .size(style::T_LABEL)
                .color(style::TEXT_MUTED),
        );
    }
    bar.push(cta).into()
}

/// What sits in the footer's action slot on the steps you leave by tapping
/// the content itself. The slot stays occupied so the footer keeps its shape.
fn footer_hint(message: &str) -> Element<'static, Message> {
    text(message.to_string())
        .size(style::T_LABEL)
        .color(style::TEXT_MUTED)
        .into()
}

/// Header, body, any error, footer - stacked the same way on every step.
fn step_page<'a>(
    header: Element<'a, Message>,
    body: Element<'a, Message>,
    footer: Element<'a, Message>,
    error: Option<&str>,
) -> Element<'a, Message> {
    let mut content = column![
        header,
        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    ]
    .spacing(PAD);

    if let Some(message) = error {
        content = content.push(error_banner(message));
    }

    container(content.push(footer).padding(PAD))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn error_banner(message: &str) -> Element<'static, Message> {
    container(
        text(message.to_string())
            .size(style::T_LABEL)
            .color(style::TEXT),
    )
    .padding(PAD)
    .width(Length::Fill)
    .style(style::panel_danger)
    .into()
}

/// A screen with nothing on it yet still says what it is waiting for.
fn empty_state(headline: &str, detail: &str) -> Element<'static, Message> {
    container(
        column![
            text(headline.to_string())
                .size(style::T_LEAD)
                .color(style::TEXT),
            text(detail.to_string())
                .size(style::T_BODY)
                .color(style::TEXT_MUTED),
        ]
        .spacing(PAD_HALF)
        .align_x(Alignment::Center),
    )
    .padding(PAD_DOUBLE)
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// The selected treatment for anything you pick from a set: the accent ring
/// and glow of [`style::panel_active`], which is the one thing on this screen
/// meant to be readable from the far side of a table. Unselected options get
/// the same footprint, so making a choice never shifts the layout.
fn selection_ring<'a>(
    selected: bool,
    choice: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let slot = container(choice.into()).padding(PAD_HALF);
    if selected {
        slot.style(style::panel_selected).into()
    } else {
        slot.into()
    }
}

/// The word under a choice that names its state, in the accent when it is
/// the one that has been picked.
fn choice_caption<'a>(selected: bool, chosen: &'a str, idle: &'a str) -> Element<'a, Message> {
    text(if selected { chosen } else { idle })
        .size(style::T_CAPTION)
        .color(if selected { style::ACCENT_BRIGHT } else { style::TEXT_MUTED })
        .into()
}

// ---------------------------------------------------------------------------
// Step 1 - how many players
// ---------------------------------------------------------------------------

fn pod_size_view(state: &SetupState) -> Element<'_, Message> {
    // One row, 2 through 8, so the choice reads as a scale you run your eye
    // along rather than a grid you have to search. It wraps if the window is
    // ever too narrow to hold the whole scale.
    let tiles: Vec<Element<Message>> = (MIN_POD..=MAX_POD)
        .map(|n| {
            let tile = style::choice_tile(n.to_string(), if n == 2 { "player" } else { "players" })
                .on_press(Message::Setup(SetupMessage::ChoosePodSize(n)));
            selection_ring(state.pod_size == n, tile)
        })
        .collect();

    step_page(
        step_header(
            step_eyebrow(1),
            "How many players?".to_string(),
            "Everyone at the table, including you.".to_string(),
            Vec::new(),
        ),
        row(tiles)
            .spacing(PAD_HALF)
            .align_y(Alignment::Center)
            .wrap()
            .into(),
        step_footer(
            "Home",
            Message::GoHome,
            footer_hint("Tap a number to continue"),
        ),
        state.error.as_deref(),
    )
}

// ---------------------------------------------------------------------------
// Step 2 - how the table is arranged
// ---------------------------------------------------------------------------

/// A small numbered-box diagram of a layout, using the same rendering logic
/// as the real board so it's an accurate preview, just shrunk down.
fn layout_preview(table: &TableLayout) -> Element<'static, Message> {
    container(layout::render_table(table, |idx| {
        container(
            text((idx + 1).to_string())
                .size(style::T_ACTION)
                .color(style::TEXT),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(style::panel)
        .into()
    }))
    .width(Length::Fixed(PREVIEW_W))
    .height(Length::Fixed(PREVIEW_H))
    .into()
}

fn layout_choice_view(state: &SetupState) -> Element<'_, Message> {
    let options = layout::options_for(state.pod_size);
    let chosen = state.table_layout.as_ref().map(|t| t.name.as_str());

    // Two per row keeps the previews large enough to read at a glance.
    let mut rows: Vec<Element<Message>> = Vec::new();
    for chunk in options.chunks(2) {
        let cards: Vec<Element<Message>> = chunk
            .iter()
            .map(|opt| {
                let selected = chosen == Some(opt.name.as_str());
                let card = button(
                    column![
                        layout_preview(opt),
                        text(opt.name.clone())
                            .size(style::T_SUBHEAD)
                            .color(style::TEXT),
                        choice_caption(selected, "Selected", "Tap to choose"),
                    ]
                    .spacing(PAD_HALF)
                    .align_x(Alignment::Center),
                )
                .padding(PAD)
                .style(style::secondary)
                .on_press(Message::Setup(SetupMessage::ChooseLayout(opt.clone())));
                selection_ring(selected, card)
            })
            .collect();
        rows.push(row(cards).spacing(PAD).into());
    }

    let body: Element<Message> = if rows.is_empty() {
        empty_state(
            "No layouts for this pod size",
            "Go back a step and pick a different number of players.",
        )
    } else {
        scrollable(
            column(rows)
                .spacing(PAD)
                .align_x(Alignment::Center)
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .into()
    };

    step_page(
        step_header(
            step_eyebrow(2),
            "How are you sitting?".to_string(),
            format!(
                "{} players \u{00b7} pick the arrangement that matches your table.",
                state.pod_size
            ),
            Vec::new(),
        ),
        body,
        step_footer(
            "Back",
            Message::Setup(SetupMessage::BackToPodSizeChoice),
            footer_hint("Tap a layout to continue"),
        ),
        state.error.as_deref(),
    )
}

// ---------------------------------------------------------------------------
// Step 3 - filling the seats
// ---------------------------------------------------------------------------

fn grid_view<'a>(
    state: &'a SetupState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let ready = state
        .seats
        .iter()
        .filter(|s| s.player.is_some() && s.commander.is_some())
        .count();
    let total = state.seats.len();

    let board: Element<Message> = match &state.table_layout {
        Some(table) => layout::render_table(table, |idx| {
            seat_tile(
                idx,
                &state.seats[idx],
                table.seat_orientation(idx),
                image_cache,
            )
        }),
        None => empty_state(
            "No table yet",
            "Go back a step and pick how the pod is sitting.",
        ),
    };

    step_page(
        step_header(
            step_eyebrow(3),
            "Set up your pod".to_string(),
            format!("{ready} of {total} seats ready \u{00b7} tap a seat to fill it."),
            vec![style::touch_button("Player Count", style::T_BODY)
                .width(Length::Fixed(UTILITY_W))
                .style(style::secondary)
                .on_press(Message::Setup(SetupMessage::BackToPodSizeChoice))
                .into()],
        ),
        board,
        step_footer(
            "Back",
            Message::Setup(SetupMessage::BackToLayoutChoice),
            forward_action(
                "Next: Turn Order",
                state
                    .all_seats_ready()
                    .then_some(Message::Setup(SetupMessage::ReviewTurnOrder)),
                "Every seat needs a player and a commander",
            ),
        ),
        state.error.as_deref(),
    )
}

fn seat_tile<'a>(
    index: usize,
    seat: &'a SeatSetup,
    facing: SeatOrientation,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let seat_label = format!("SEAT {}", index + 1);

    let content: Element<Message> = match (&seat.player, &seat.commander) {
        (Some(player), Some(commander)) => {
            let art = art::framed_pair(
                commander,
                seat.partner.as_ref(),
                image_cache,
                style::T_BODY,
                facing.radians(),
            );

            let caption = container(
                container(
                    column![
                        text(seat_label)
                            .size(style::T_MICRO)
                            .color(style::TEXT_MUTED),
                        text(player.name.clone())
                            .size(style::T_SUBHEAD)
                            .color(style::TEXT),
                        text(commander.name.clone())
                            .size(style::T_BODY)
                            .color(style::TEXT_MUTED),
                    ]
                    .spacing(PAD_TIGHT),
                )
                .padding([PAD_HALF, PAD])
                .style(style::glass),
            )
            .padding(PAD_HALF)
            .width(Length::Fill);

            let overlay = container(caption)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(iced::alignment::Vertical::Bottom);

            stack![art, overlay].into()
        }
        (Some(player), None) => seat_prompt(
            seat_label,
            player.name.clone(),
            "Tap to pick a commander".to_string(),
        ),
        _ => seat_prompt(
            seat_label,
            "Empty".to_string(),
            "Tap to choose who sits here".to_string(),
        ),
    };

    container(
        button(content)
            .padding(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::ghost)
            .on_press(Message::Setup(SetupMessage::EditSeat(index))),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(style::panel)
    .clip(true)
    .into()
}

/// What an unfinished seat says for itself: which seat it is, what is in it,
/// and what tapping it will do.
fn seat_prompt<'a>(seat_label: String, headline: String, hint: String) -> Element<'a, Message> {
    container(
        column![
            text(seat_label)
                .size(style::T_CAPTION)
                .color(style::TEXT_MUTED),
            text(headline).size(style::T_LEAD).color(style::TEXT),
            text(hint).size(style::T_BODY).color(style::TEXT_MUTED),
        ]
        .spacing(PAD_HALF)
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

// ---------------------------------------------------------------------------
// Step 3, detour - the seat editor
//
// Player, commander, art and framing all live behind one seat, so they share
// the step chrome: same header slot, same footer, only the middle changes.
// ---------------------------------------------------------------------------

fn editor_overlay<'a>(
    state: &'a SetupState,
    players_cache: &'a [Player],
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let seat_index = state.editing_seat.unwrap();
    let seat = &state.seats[seat_index];

    let (title, instruction, body, footer): (String, String, Element<Message>, Element<Message>) =
        if state.framing {
            let name = seat
                .commander_in(state.editing_slot)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| "this seat".to_string());
            (
                format!("Frame {name}"),
                "Drag to move \u{00b7} pinch or scroll to zoom.".to_string(),
                framing_editor(
                    seat,
                    state.editing_slot,
                    seat_index,
                    state.table_layout.as_ref(),
                    image_cache,
                ),
                step_footer(
                    "Back to Grid",
                    Message::Setup(SetupMessage::BackToGrid),
                    forward_action(
                        "Done",
                        Some(Message::Setup(SetupMessage::DoneFraming)),
                        "",
                    ),
                ),
            )
        } else if let Some(target) = &state.art_target {
            let status = if state.loading_art_options {
                "Loading every printing from Scryfall\u{2026}".to_string()
            } else if state.art_options.len() == 1 {
                "1 printing found \u{00b7} tap it to use its art.".to_string()
            } else {
                format!(
                    "{} printings found \u{00b7} tap one to use its art.",
                    state.art_options.len()
                )
            };
            (
                format!("Choose art for {}", target.name),
                status,
                art_gallery(state, image_cache),
                step_footer(
                    "Back to search",
                    Message::Setup(SetupMessage::CancelArtPick),
                    footer_hint("Tap a printing to use its art"),
                ),
            )
        } else if seat.player.is_none() {
            (
                "Who's sitting here?".to_string(),
                "Tap a name, or type a new one below.".to_string(),
                player_picker(state, players_cache),
                step_footer(
                    "Back to Grid",
                    Message::Setup(SetupMessage::BackToGrid),
                    footer_hint("Tap a name to seat that player"),
                ),
            )
        } else if seat.commander_in(state.editing_slot).is_none() {
            let title = if state.editing_slot == PARTNER {
                "Pick a partner"
            } else {
                "Pick a commander"
            };
            (
                title.to_string(),
                "Tap one this player has run before, or search Scryfall by name.".to_string(),
                commander_picker(state, &seat.commander_history),
                step_footer(
                    "Back to Grid",
                    Message::Setup(SetupMessage::BackToGrid),
                    footer_hint("Tap a result to pick it"),
                ),
            )
        } else {
            (
                "This seat is ready".to_string(),
                "Change anything here, or head back to the table.".to_string(),
                seat_summary(seat, image_cache),
                step_footer(
                    "Back to Grid",
                    Message::Setup(SetupMessage::BackToGrid),
                    text("Seat ready")
                        .size(style::T_LABEL)
                        .color(style::SUCCESS)
                        .into(),
                ),
            )
        };

    step_page(
        step_header(
            pane_eyebrow(format!("STEP 3 OF {STEP_COUNT} \u{00b7} SEAT {}", seat_index + 1)),
            title,
            instruction,
            Vec::new(),
        ),
        body,
        footer,
        state.error.as_deref(),
    )
}

fn player_picker<'a>(state: &'a SetupState, players_cache: &'a [Player]) -> Element<'a, Message> {
    let taken = state.players_taken_by_other_seats();
    let available: Vec<&Player> = players_cache
        .iter()
        .filter(|p| !taken.contains(&p.id))
        .collect();

    let list: Element<Message> = if available.is_empty() {
        empty_state(
            "No players to seat",
            "Everyone on file is already at this table. Type a new name below.",
        )
    } else {
        scrollable(
            row(available
                .into_iter()
                .map(|p| {
                    style::touch_button(&p.name, style::T_SUBHEAD)
                        .width(Length::Fixed(CARD_W))
                        .style(style::secondary)
                        .on_press(Message::Setup(SetupMessage::PickExistingPlayer(p.clone())))
                        .into()
                })
                .collect::<Vec<Element<Message>>>())
            .spacing(PAD)
            .wrap(),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    };

    let has_name = !state.new_player_name.trim().is_empty();
    let mut add = style::touch_button("Add Player", style::T_ACTION)
        .width(Length::Fixed(UTILITY_W))
        .style(style::primary);
    if has_name {
        add = add.on_press(Message::Setup(SetupMessage::CreatePlayer));
    }

    column![
        list,
        row![
            text_input("New player name", &state.new_player_name)
                .size(style::T_SUBHEAD)
                .padding(style::FIELD_PAD)
                .style(style::input)
                .on_input(|s| Message::Setup(SetupMessage::NewPlayerNameChanged(s)))
                .on_submit(Message::Setup(SetupMessage::CreatePlayer)),
            add,
        ]
        .spacing(PAD)
        .align_y(Alignment::Center),
    ]
    .spacing(PAD)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn commander_picker<'a>(state: &'a SetupState, history: &'a [SavedDeck]) -> Element<'a, Message> {
    let history_block: Element<Message> = if history.is_empty() {
        container(
            text("No commanders on record for this player yet \u{2014} search below.")
                .size(style::T_BODY)
                .color(style::TEXT_MUTED),
        )
        .padding(PAD)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .style(style::panel)
        .into()
    } else {
        scrollable(
            row(history
                .iter()
                .map(|deck| {
                    style::touch_button(deck.label(), style::T_LABEL)
                        .width(Length::Fixed(CARD_W))
                        .style(style::secondary)
                        .on_press(Message::Setup(SetupMessage::PickHistoryCommander(
                            deck.clone(),
                        )))
                        .into()
                })
                .collect::<Vec<Element<Message>>>())
            .spacing(PAD_HALF)
            .wrap(),
        )
        .width(Length::Fill)
        .height(Length::Fixed(style::TOUCH_H * 2.0 + PAD_HALF))
        .into()
    };

    let search_label: String = if state.cooldown.active() {
        state.cooldown.label()
    } else if state.searching {
        "Searching\u{2026}".into()
    } else {
        "Search".into()
    };

    let mut search_button = style::touch_button(search_label, style::T_ACTION)
        .width(Length::Fixed(UTILITY_W))
        .style(style::primary);
    if !state.cooldown.active() && !state.searching {
        search_button = search_button.on_press(Message::Setup(SetupMessage::SearchCommanders));
    }

    // Every result is the same height with its name and colours on the same
    // two rails, so the list reads as one column of targets rather than a
    // ragged stack.
    let results: Element<Message> = if state.cooldown.active() {
        empty_state(
            "Scryfall asked us to slow down",
            "Search comes back as soon as the countdown on the button runs out.",
        )
    } else if state.searching {
        empty_state(
            "Searching Scryfall\u{2026}",
            "Looking for commanders whose name matches what you typed.",
        )
    } else if state.commander_results.is_empty() {
        empty_state(
            "No results yet",
            "Type part of a commander's name and tap Search.",
        )
    } else {
        scrollable(
            column(
                state
                    .commander_results
                    .iter()
                    .map(|c| {
                        button(
                            container(
                                row![
                                    text(c.name.clone())
                                        .size(style::T_ACTION)
                                        .color(style::TEXT),
                                    iced::widget::horizontal_space(),
                                    container(
                                        text(c.color_identity.clone())
                                            .size(style::T_CAPTION)
                                            .color(style::TEXT_MUTED),
                                    )
                                    .padding([PAD_TIGHT, PAD_HALF])
                                    .style(style::badge),
                                ]
                                .spacing(PAD)
                                .align_y(Alignment::Center),
                            )
                            .padding([0.0, PAD])
                            .center_y(Length::Fill),
                        )
                        .padding(0)
                        .height(Length::Fixed(style::TOUCH_H))
                        .width(Length::Fill)
                        .style(style::secondary)
                        .on_press(Message::Setup(SetupMessage::PickCommanderName(c.clone())))
                        .into()
                    })
                    .collect::<Vec<Element<Message>>>(),
            )
            .spacing(PAD_HALF)
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    };

    column![
        text("This player's commanders")
            .size(style::T_CAPTION)
            .color(style::TEXT_MUTED),
        history_block,
        row![
            text_input("Commander name", &state.commander_query)
                .size(style::T_SUBHEAD)
                .padding(style::FIELD_PAD)
                .style(style::input)
                .on_input(|s| Message::Setup(SetupMessage::CommanderQueryChanged(s)))
                .on_submit(Message::Setup(SetupMessage::SearchCommanders)),
            search_button,
        ]
        .spacing(PAD)
        .align_y(Alignment::Center),
        results,
    ]
    .spacing(PAD)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn art_gallery<'a>(
    state: &'a SetupState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    if state.art_options.is_empty() {
        return if state.loading_art_options {
            empty_state(
                "Loading printings\u{2026}",
                "Fetching every version of this card from Scryfall.",
            )
        } else {
            empty_state(
                "No printings came back",
                "Head back to the search and pick the commander again.",
            )
        };
    }

    // Every card is the same size - a fixed art box over a fixed caption
    // band - so the gallery is a grid rather than a ragged row, however long
    // a set name happens to be.
    let tiles: Vec<Element<Message>> = state
        .art_options
        .iter()
        .map(|card| {
            let thumb: Element<Message> = match card
                .small_url
                .as_deref()
                .and_then(|u| image_cache.get(u))
            {
                Some(handle) => container(
                    image(handle.clone())
                        .width(Length::Fixed(ART_W))
                        .height(Length::Fixed(ART_H))
                        .content_fit(ContentFit::Cover),
                )
                .clip(true)
                .into(),
                None => container(
                    text("Loading\u{2026}")
                        .size(style::T_CAPTION)
                        .color(style::TEXT_MUTED),
                )
                .width(Length::Fixed(ART_W))
                .height(Length::Fixed(ART_H))
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(style::panel)
                .into(),
            };

            button(
                column![
                    thumb,
                    container(
                        text(card.set_name.clone())
                            .size(style::T_CAPTION)
                            .color(style::TEXT_MUTED),
                    )
                    .width(Length::Fixed(ART_W))
                    .height(Length::Fixed(style::TOUCH_H))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .clip(true),
                ]
                .spacing(PAD_HALF)
                .align_x(Alignment::Center),
            )
            .padding(PAD_HALF)
            .style(style::secondary)
            .on_press(Message::Setup(SetupMessage::PickArt(card.clone())))
            .into()
        })
        .collect();

    scrollable(row(tiles).spacing(PAD).wrap())
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Zoom and anchor the art so the part that matters ends up visible in the
/// seat tile during play. The preview is the real renderer at tile aspect.
/// Drag the art around and pinch (or scroll) to zoom, inside a box shaped
/// like the tile this art will actually occupy. The framing is remembered
/// per layout and seat, because the same art needs a different crop in a
/// tall head-of-table tile than in a short wide one.
fn framing_editor<'a>(
    seat: &'a SeatSetup,
    slot: usize,
    seat_index: usize,
    table: Option<&'a TableLayout>,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let Some(commander) = seat.commander_in(slot) else {
        return empty_state(
            "Nothing to frame yet",
            "This seat needs a commander before its art can be placed.",
        );
    };

    let handle = commander
        .portrait_url()
        .and_then(|u| image_cache.get(u))
        .cloned();

    let Some(handle) = handle else {
        return empty_state(
            "Loading art\u{2026}",
            "Fetching this printing's image from Scryfall.",
        );
    };

    // Match the real tile's proportions so what's lined up here is what
    // shows in the game.
    let aspect = table.map_or(1.6, |t| t.tile_aspect(seat_index));
    let preview_h: f32 = 520.0;
    let preview_w = (preview_h * aspect).clamp(320.0, 1100.0);

    let surface = container(panned_image::editable(handle, commander.framing, |framing| {
        Message::Setup(SetupMessage::FramingChanged(framing))
    }))
    .width(Length::Fixed(preview_w))
    .height(Length::Fixed(preview_h))
    .clip(true)
    .style(style::panel);

    let layout_note = match table {
        Some(t) => format!(
            "Seat {} of the {} layout \u{00b7} saved for this seat only",
            seat_index + 1,
            t.name
        ),
        None => "Pick a layout first".to_string(),
    };

    column![
        container(surface).width(Length::Fill).center_x(Length::Fill),
        text(layout_note)
            .size(style::T_CAPTION)
            .color(style::TEXT_MUTED),
    ]
    .spacing(PAD)
    .align_x(Alignment::Center)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn seat_summary<'a>(
    seat: &'a SeatSetup,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let player = seat.player.as_ref().unwrap();
    let commander = seat.commander.as_ref().unwrap();

    let portrait = art::framed_pair(
        commander,
        seat.partner.as_ref(),
        image_cache,
        style::T_LABEL,
        0.0,
    );

    let heading = match &seat.partner {
        Some(p) => format!("{} is playing {} + {}", player.name, commander.name, p.name),
        None => format!("{} is playing {}", player.name, commander.name),
    };

    // Colour identity of a partner pair is the union of both halves.
    let identity = match &seat.partner {
        Some(p) => {
            let mut letters: Vec<char> = commander
                .color_identity
                .chars()
                .chain(p.color_identity.chars())
                .collect();
            letters.sort_unstable();
            letters.dedup();
            letters.into_iter().collect::<String>()
        }
        None => commander.color_identity.clone(),
    };

    let primary_row = row![
        style::touch_button("Change Player", style::T_LABEL)
            .width(Length::Fill)
            .style(style::secondary)
            .on_press(Message::Setup(SetupMessage::ClearSeatPlayer)),
        style::touch_button("Change Commander", style::T_LABEL)
            .width(Length::Fill)
            .style(style::secondary)
            .on_press(Message::Setup(SetupMessage::ClearSeatCommander(PRIMARY))),
        style::touch_button("Change Art", style::T_LABEL)
            .width(Length::Fill)
            .style(style::secondary)
            .on_press(Message::Setup(SetupMessage::ChangeArt(PRIMARY))),
        style::touch_button("Frame Art", style::T_LABEL)
            .width(Length::Fill)
            .style(style::secondary)
            .on_press(Message::Setup(SetupMessage::StartFraming(PRIMARY))),
    ]
    .spacing(PAD_HALF);

    // The partner row only appears once there is one; until then a single
    // button offers to add one, so single-commander decks see no clutter.
    let partner_row: Element<Message> = match &seat.partner {
        Some(partner) => column![
            text(format!("Partner: {}", partner.name))
                .size(style::T_CAPTION)
                .color(style::TEXT_MUTED),
            row![
                style::touch_button("Remove Partner", style::T_LABEL)
                    .width(Length::Fill)
                    .style(style::danger)
                    .on_press(Message::Setup(SetupMessage::ClearSeatCommander(PARTNER))),
                style::touch_button("Partner Art", style::T_LABEL)
                    .width(Length::Fill)
                    .style(style::secondary)
                    .on_press(Message::Setup(SetupMessage::ChangeArt(PARTNER))),
                style::touch_button("Frame Partner", style::T_LABEL)
                    .width(Length::Fill)
                    .style(style::secondary)
                    .on_press(Message::Setup(SetupMessage::StartFraming(PARTNER))),
            ]
            .spacing(PAD_HALF),
        ]
        .spacing(PAD_HALF)
        .into(),
        None => style::touch_button("Add Partner", style::T_LABEL)
            .width(Length::Fixed(CTA_W))
            .style(style::secondary)
            .on_press(Message::Setup(SetupMessage::AddPartner))
            .into(),
    };

    column![
        container(portrait)
            .width(Length::Fill)
            .height(Length::FillPortion(4))
            .clip(true),
        text(heading).size(style::T_HEADING).color(style::TEXT),
        text(format!("Colour identity: {identity}"))
            .size(style::T_BODY)
            .color(style::TEXT_MUTED),
        primary_row,
        partner_row,
    ]
    .spacing(PAD)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

// ---------------------------------------------------------------------------
// Step 4 - who leads, and which way turns pass
// ---------------------------------------------------------------------------

/// The last setup step: tap who leads, set which way turns pass, then start.
/// Every seat shows the position it will play in, so the pod can check the
/// order against the real table before anyone draws a card.
fn turn_order_view<'a>(
    state: &'a SetupState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let order = planned_turn_order(state);

    let board: Element<Message> = match &state.table_layout {
        Some(table) => layout::render_table(table, |idx| {
            // Position in the turn order, 1-indexed, once a leader is set.
            let position = order
                .as_ref()
                .and_then(|o| o.iter().position(|&seat| seat == idx))
                .map(|p| p + 1);
            turn_order_tile(
                idx,
                &state.seats[idx],
                position,
                table.seat_orientation(idx),
                image_cache,
            )
        }),
        None => empty_state(
            "No table yet",
            "Go back and pick how the pod is sitting.",
        ),
    };

    let direction_buttons = row([TurnDirection::Clockwise, TurnDirection::CounterClockwise]
        .into_iter()
        .map(|dir| {
            let selected = state.turn_direction == dir;
            let choice = button(
                column![
                    text(dir.label()).size(style::T_ACTION).color(style::TEXT),
                    choice_caption(selected, "Turns pass this way", "Tap to use"),
                ]
                .spacing(PAD_TIGHT)
                .align_x(Alignment::Center),
            )
            .padding(PAD_HALF)
            .width(Length::Fixed(CARD_W))
            .height(Length::Fixed(style::TOUCH_H))
            .style(style::secondary)
            .on_press(Message::Setup(SetupMessage::SetTurnDirection(dir)));
            selection_ring(selected, choice)
        })
        .collect::<Vec<Element<Message>>>())
    .spacing(PAD);

    let chain: Element<Message> = match &order {
        Some(order) => {
            let names: Vec<String> = order
                .iter()
                .filter_map(|&seat| state.seats[seat].player.as_ref())
                .map(|p| p.name.clone())
                .collect();
            // Naming the wrap-around explicitly says which way turns pass
            // without leaning on an arrow glyph the font may not have.
            let leader = names.first().cloned().unwrap_or_default();
            let joined = names.join("  \u{203a}  ");
            text(format!("{joined}  \u{203a}  back to {leader}"))
                .size(style::T_ACTION)
                .color(style::TEXT)
                .into()
        }
        None => text("Tap a seat above to choose who takes the first turn.")
            .size(style::T_ACTION)
            .color(style::TEXT_MUTED)
            .into(),
    };

    let summary = container(
        column![
            text("TURN ORDER")
                .size(style::T_MICRO)
                .color(style::TEXT_MUTED),
            chain,
        ]
        .spacing(PAD_TIGHT)
        .align_x(Alignment::Center),
    )
    .padding(PAD)
    .width(Length::Fill)
    .center_x(Length::Fill)
    .style(style::panel);

    let body = column![
        container(board).width(Length::Fill).height(Length::Fill),
        container(direction_buttons).center_x(Length::Fill),
        summary,
    ]
    .spacing(PAD)
    .width(Length::Fill)
    .height(Length::Fill);

    step_page(
        step_header(
            step_eyebrow(4),
            "Who goes first?".to_string(),
            "Tap a seat, then choose which way turns pass.".to_string(),
            vec![style::touch_button("Random", style::T_BODY)
                .width(Length::Fixed(UTILITY_W))
                .style(style::secondary)
                .on_press(Message::Setup(SetupMessage::RandomFirstSeat))
                .into()],
        ),
        body.into(),
        step_footer(
            "Back",
            Message::Setup(SetupMessage::BackToGridFromTurnOrder),
            forward_action(
                "Start Game",
                order
                    .is_some()
                    .then_some(Message::Setup(SetupMessage::StartGame)),
                "Pick who takes the first turn",
            ),
        ),
        state.error.as_deref(),
    )
}

/// A seat on the turn-order screen: its art, the player's name, and the
/// position it plays in. The position sits in the middle of the tile in the
/// same frosted chip the life counter uses in-game, so "First Player" lands
/// where everyone is already used to looking.
fn turn_order_tile<'a>(
    index: usize,
    seat: &'a SeatSetup,
    position: Option<usize>,
    facing: SeatOrientation,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let Some(player) = &seat.player else {
        return container(empty_state(
            "Empty seat",
            "Go back a step to put someone here.",
        ))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::panel)
        .clip(true)
        .into();
    };

    let art: Element<Message> = match &seat.commander {
        Some(commander) => art::framed_pair(
            commander,
            seat.partner.as_ref(),
            image_cache,
            style::T_BODY,
            facing.radians(),
        ),
        None => iced::widget::horizontal_space().into(),
    };

    // Centered over the art, matching the in-game life counter: the leader
    // gets the heavier chip, everyone else the lighter one.
    let badge: Element<Message> = match position {
        Some(1) => center_chip(
            text("First Player")
                .size(style::T_TITLE)
                .color(style::TEXT)
                .into(),
            style::glass_strong,
        ),
        Some(p) => center_chip(
            text(format!("#{p}"))
                .size(style::T_TITLE)
                .color(style::TEXT)
                .into(),
            style::glass,
        ),
        None => iced::widget::horizontal_space().into(),
    };

    let caption = container(
        container(
            column![
                text(format!("SEAT {}", index + 1))
                    .size(style::T_MICRO)
                    .color(style::TEXT_MUTED),
                text(player.name.clone())
                    .size(style::T_SUBHEAD)
                    .color(style::TEXT),
            ]
            .spacing(PAD_TIGHT),
        )
        .padding([PAD_HALF, PAD])
        .style(style::glass),
    )
    .padding(PAD_HALF)
    .width(Length::Fill);

    let overlay = container(caption)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::alignment::Vertical::Bottom);

    let tile = button(stack![art, overlay, badge])
        .padding(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::ghost)
        .on_press(Message::Setup(SetupMessage::ChooseFirstSeat(index)));

    container(tile)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(if position == Some(1) {
            style::panel_active
        } else {
            style::panel
        })
        .clip(true)
        .into()
}

/// A frosted chip parked in the middle of a tile, which is where this screen
/// puts anything that has to be read across the table.
fn center_chip<'a>(
    label: Element<'a, Message>,
    chip: fn(&iced::Theme) -> container::Style,
) -> Element<'a, Message> {
    container(container(label).padding([PAD_HALF, PAD_DOUBLE]).style(chip))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}
