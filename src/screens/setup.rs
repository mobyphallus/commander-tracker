use std::collections::HashMap;

use iced::widget::{button, column, container, image, row, scrollable, stack, text, text_input};
use iced::{ContentFit, Element, Length, Task};
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
        SetupStage::ChoosePodSize => pod_size_view(),
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

/// A small numbered-box diagram of a layout, using the same rendering logic
/// as the real board so it's an accurate preview, just shrunk down.
fn layout_preview(table: &TableLayout) -> Element<'static, Message> {
    container(layout::render_table(table, |idx| {
        container(text((idx + 1).to_string()).size(style::T_ACTION))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(style::panel)
            .into()
    }))
    .width(Length::Fixed(300.0))
    .height(Length::Fixed(190.0))
    .into()
}

/// A full-bleed step screen: title, subtitle, body, and a back button
/// pinned at the bottom, so the two setup steps feel like one flow.
fn step_screen<'a>(
    title: &'a str,
    subtitle: String,
    body: Element<'a, Message>,
    back: Message,
) -> Element<'a, Message> {
    let header = container(
        column![text(title).size(style::T_TITLE), text(subtitle).size(style::T_LABEL)]
            .spacing(8)
            .align_x(iced::Alignment::Center),
    )
    .padding(24)
    .width(Length::Fill)
    .center_x(Length::Fill)
    .style(style::header);

    container(
        column![
            header,
            container(body)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
            style::touch_button("Back", style::T_ACTION)
                .width(Length::Fixed(280.0))
                .style(style::secondary)
                .on_press(back),
        ]
        .spacing(style::GAP)
        .align_x(iced::Alignment::Center)
        .padding(style::GAP),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn layout_choice_view(state: &SetupState) -> Element<'_, Message> {
    let options = layout::options_for(state.pod_size);

    // Two per row keeps the previews large enough to read at a glance.
    let mut rows: Vec<Element<Message>> = Vec::new();
    for chunk in options.chunks(2) {
        let cards: Vec<Element<Message>> = chunk
            .iter()
            .map(|opt| {
                let label = opt.name.clone();
                button(
                    column![layout_preview(opt), text(label).size(style::T_SUBHEAD)]
                        .spacing(16)
                        .align_x(iced::Alignment::Center),
                )
                .padding(24)
                .style(style::secondary)
                .on_press(Message::Setup(SetupMessage::ChooseLayout(opt.clone())))
                .into()
            })
            .collect();
        rows.push(row(cards).spacing(28).into());
    }

    step_screen(
        "How are you sitting?",
        format!("{} players - pick the arrangement that matches your table", state.pod_size),
        scrollable(
            column(rows)
                .spacing(28)
                .align_x(iced::Alignment::Center)
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .into(),
        Message::Setup(SetupMessage::BackToPodSizeChoice),
    )
}

fn pod_size_view<'a>() -> Element<'a, Message> {
    // One row, 2 through 8, so the choice reads as a scale you run your eye
    // along rather than a grid you have to search. It wraps if the window is
    // ever too narrow to hold the whole scale.
    let tiles: Vec<Element<Message>> = (MIN_POD..=MAX_POD)
        .map(|n| {
            style::choice_tile(n.to_string(), if n == 2 { "player" } else { "players" })
                .on_press(Message::Setup(SetupMessage::ChoosePodSize(n)))
                .into()
        })
        .collect();

    step_screen(
        "How many players?",
        "Everyone at the table, including you".to_string(),
        row(tiles)
            .spacing(style::GAP)
            .align_y(iced::Alignment::Center)
            .wrap()
            .into(),
        Message::GoHome,
    )
}

fn grid_view<'a>(
    state: &'a SetupState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let header = container(
        row![
            text("Set up your pod").size(style::T_HEADING),
            iced::widget::horizontal_space(),
            style::touch_button("Layout", style::T_BODY)
                .width(Length::Fixed(160.0))
                .style(style::secondary)
                .on_press(Message::Setup(SetupMessage::BackToLayoutChoice)),
            style::touch_button("Player Count", style::T_BODY)
                .width(Length::Fixed(220.0))
                .style(style::secondary)
                .on_press(Message::Setup(SetupMessage::BackToPodSizeChoice)),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .padding(16)
    .width(Length::Fill)
    .style(style::header);

    let board = match &state.table_layout {
        Some(table) => {
            layout::render_table(table, |idx| {
                seat_tile(
                    idx,
                    &state.seats[idx],
                    table.seat_orientation(idx),
                    image_cache,
                )
            })
        }
        None => text("Pick a layout first.").size(style::T_LABEL).into(),
    };

    let mut start_button = style::cta_button("Next: Turn Order", style::T_LEAD).width(Length::Fixed(480.0));
    if state.all_seats_ready() {
        start_button = start_button
            .style(style::success)
            .on_press(Message::Setup(SetupMessage::ReviewTurnOrder));
    }

    let mut content =
        column![header, container(board).height(Length::Fill)].spacing(style::GAP);

    if let Some(e) = &state.error {
        content = content.push(
            container(text(e.clone()).size(style::T_LABEL))
                .padding(16)
                .width(Length::Fill)
                .style(style::panel_danger),
        );
    }

    content = content.push(container(start_button).center_x(Length::Fill));

    container(content.padding(style::GAP))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn seat_tile<'a>(
    index: usize,
    seat: &'a SeatSetup,
    facing: SeatOrientation,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    match (&seat.player, &seat.commander) {
        (Some(player), Some(commander)) => {
            let art = art::framed_pair(
                commander,
                seat.partner.as_ref(),
                image_cache,
                18,
                facing.radians(),
            );

            let caption = container(
                container(
                    column![
                        text(player.name.clone()).size(style::T_SUBHEAD),
                        text(commander.name.clone()).size(style::T_BODY),
                    ]
                    .spacing(4),
                )
                .padding([12, 20])
                .style(style::glass),
            )
            .padding(14)
            .width(Length::Fill);

            let overlay = container(caption)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(iced::alignment::Vertical::Bottom);

            button(stack![art, overlay])
                .padding(0)
                .width(Length::Fill)
                .height(Length::Fill)
                .on_press(Message::Setup(SetupMessage::EditSeat(index)))
                .into()
        }
        (Some(player), None) => button(
            container(
                column![
                    text(player.name.clone()).size(style::T_LEAD),
                    text("Tap to pick a commander").size(style::T_BODY),
                ]
                .spacing(10)
                .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::secondary)
        .on_press(Message::Setup(SetupMessage::EditSeat(index)))
        .into(),
        _ => button(
            container(text(format!("+ Add Player {}", index + 1)).size(style::T_HEADING))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::secondary)
        .on_press(Message::Setup(SetupMessage::EditSeat(index)))
        .into(),
    }
}

fn editor_overlay<'a>(
    state: &'a SetupState,
    players_cache: &'a [Player],
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let seat_index = state.editing_seat.unwrap();
    let seat = &state.seats[seat_index];

    let body: Element<Message> = if state.framing {
        framing_editor(
            seat,
            state.editing_slot,
            seat_index,
            state.table_layout.as_ref(),
            image_cache,
        )
    } else if state.art_target.is_some() {
        art_gallery(state, image_cache)
    } else if seat.player.is_none() {
        player_picker(state, players_cache)
    } else if seat.commander_in(state.editing_slot).is_none() {
        commander_picker(state, &seat.commander_history)
    } else {
        seat_summary(seat, image_cache)
    };

    let top = row![
        style::touch_button("\u{00ab} Back to Grid", style::T_LABEL)
            .width(Length::Fixed(280.0))
            .style(style::secondary)
            .on_press(Message::Setup(SetupMessage::BackToGrid)),
    ];

    let mut content = column![top, body].spacing(style::GAP).height(Length::Fill);

    if let Some(e) = &state.error {
        content = content.push(
            container(text(e.clone()).size(style::T_LABEL))
                .padding(16)
                .width(Length::Fill)
                .style(style::panel_danger),
        );
    }

    container(content.padding(style::GAP))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn player_picker<'a>(state: &'a SetupState, players_cache: &'a [Player]) -> Element<'a, Message> {
    let taken = state.players_taken_by_other_seats();
    let available: Vec<&Player> = players_cache.iter().filter(|p| !taken.contains(&p.id)).collect();

    let existing = row(available
        .into_iter()
        .map(|p| {
            style::touch_button(&p.name, style::T_SUBHEAD)
                .width(Length::Fixed(300.0))
                .style(style::secondary)
                .on_press(Message::Setup(SetupMessage::PickExistingPlayer(p.clone())))
                .into()
        })
        .collect::<Vec<Element<Message>>>())
    .spacing(14)
    .wrap();

    column![
        text("Who's sitting here?").size(style::T_TITLE),
        scrollable(existing).height(Length::Fill),
        row![
            text_input("New player name", &state.new_player_name)
                .size(style::T_SUBHEAD)
                .padding(22)
                .style(style::input)
                .on_input(|s| Message::Setup(SetupMessage::NewPlayerNameChanged(s)))
                .on_submit(Message::Setup(SetupMessage::CreatePlayer)),
            style::touch_button("Add Player", style::T_ACTION)
                .width(Length::Fixed(240.0))
                .style(style::primary)
                .on_press(Message::Setup(SetupMessage::CreatePlayer)),
        ]
        .spacing(14)
        .align_y(iced::Alignment::Center),
    ]
    .spacing(22)
    .height(Length::Fill)
    .into()
}

fn commander_picker<'a>(
    state: &'a SetupState,
    history: &'a [SavedDeck],
) -> Element<'a, Message> {
    let history_row: Element<Message> = if history.is_empty() {
        text("No commanders played yet - search below to add one.")
            .size(style::T_BODY)
            .into()
    } else {
        row(history
            .iter()
            .map(|deck| {
                style::touch_button(deck.label(), 20)
                    .width(Length::Fixed(320.0))
                    .style(style::secondary)
                    .on_press(Message::Setup(SetupMessage::PickHistoryCommander(
                        deck.clone(),
                    )))
                    .into()
            })
            .collect::<Vec<Element<Message>>>())
        .spacing(12)
        .wrap()
        .into()
    };

    let results = column(
        state
            .commander_results
            .iter()
            .map(|c| {
                button(
                    container(text(format!("{}   [{}]", c.name, c.color_identity)).size(style::T_ACTION))
                        .padding([0, 20])
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
    .spacing(10);

    let search_label: String = if state.cooldown.active() {
        state.cooldown.label()
    } else if state.searching {
        "Searching...".into()
    } else {
        "Search".into()
    };

    let mut search_button = style::touch_button(search_label, style::T_ACTION)
        .width(Length::Fixed(220.0))
        .style(style::primary);
    if !state.cooldown.active() {
        search_button =
            search_button.on_press(Message::Setup(SetupMessage::SearchCommanders));
    }

    let heading = if state.editing_slot == PARTNER {
        "Pick a partner"
    } else {
        "Pick a commander"
    };

    column![
        text(heading).size(style::T_TITLE),
        text("This player's commanders").size(style::T_BODY),
        scrollable(history_row).height(Length::Fixed(190.0)),
        row![
            text_input("Commander name", &state.commander_query)
                .size(style::T_SUBHEAD)
                .padding(22)
                .style(style::input)
                .on_input(|s| Message::Setup(SetupMessage::CommanderQueryChanged(s)))
                .on_submit(Message::Setup(SetupMessage::SearchCommanders)),
            search_button,
        ]
        .spacing(14)
        .align_y(iced::Alignment::Center),
        scrollable(results).height(Length::Fill),
    ]
    .spacing(18)
    .height(Length::Fill)
    .into()
}

fn art_gallery<'a>(
    state: &'a SetupState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let target = state.art_target.as_ref().unwrap();

    let tiles: Vec<Element<Message>> = state
        .art_options
        .iter()
        .map(|card| {
            let thumb: Element<Message> = match card.small_url.as_deref().and_then(|u| image_cache.get(u)) {
                Some(handle) => image(handle.clone())
                    .width(Length::Fixed(300.0))
                    .height(Length::Fixed(220.0))
                    .content_fit(ContentFit::Cover)
                    .into(),
                None => container(text("...").size(style::T_LABEL))
                    .width(Length::Fixed(300.0))
                    .height(Length::Fixed(220.0))
                    .center_x(Length::Fixed(300.0))
                    .center_y(Length::Fixed(220.0))
                    .into(),
            };
            button(
                column![thumb, text(card.set_name.clone()).size(style::T_CAPTION)]
                    .spacing(8)
                    .align_x(iced::Alignment::Center),
            )
            .padding(10)
            .style(style::secondary)
            .on_press(Message::Setup(SetupMessage::PickArt(card.clone())))
            .into()
        })
        .collect();

    let status = if state.loading_art_options {
        text("Loading every printing from Scryfall...").size(style::T_BODY)
    } else {
        text(format!("{} printings found", state.art_options.len())).size(style::T_BODY)
    };

    column![
        text(format!("Choose art for {}", target.name)).size(style::T_TITLE),
        status,
        scrollable(row(tiles).spacing(16).wrap()).height(Length::Fill),
        style::touch_button("Back to search", style::T_LABEL)
            .width(Length::Fixed(280.0))
            .style(style::secondary)
            .on_press(Message::Setup(SetupMessage::CancelArtPick)),
    ]
    .spacing(18)
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
        return text("Pick a commander first.").size(style::T_ACTION).into();
    };

    let handle = commander
        .portrait_url()
        .and_then(|u| image_cache.get(u))
        .cloned();

    let Some(handle) = handle else {
        return container(text("Loading art...").size(style::T_SUBHEAD))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into();
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
        text(format!("Frame {}", commander.name)).size(style::T_HEADING),
        text("Drag to move \u{00b7} pinch or scroll to zoom").size(style::T_LABEL),
        container(surface).width(Length::Fill).center_x(Length::Fill),
        text(layout_note).size(style::T_CAPTION),
        container(
            style::cta_button("Done", style::T_SUBHEAD)
                .width(Length::Fixed(360.0))
                .style(style::success)
                .on_press(Message::Setup(SetupMessage::DoneFraming))
        )
        .width(Length::Fill)
        .center_x(Length::Fill),
    ]
    .spacing(16)
    .align_x(iced::Alignment::Center)
    .height(Length::Fill)
    .into()
}

fn seat_summary<'a>(
    seat: &'a SeatSetup,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let player = seat.player.as_ref().unwrap();
    let commander = seat.commander.as_ref().unwrap();

    let portrait = art::framed_pair(commander, seat.partner.as_ref(), image_cache, 20, 0.0);

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
            .style(style::primary)
            .on_press(Message::Setup(SetupMessage::StartFraming(PRIMARY))),
    ]
    .spacing(14);

    // The partner row only appears once there is one; until then a single
    // button offers to add one, so single-commander decks see no clutter.
    let partner_row: Element<Message> = match &seat.partner {
        Some(partner) => column![
            text(format!("Partner: {}", partner.name)).size(style::T_ACTION),
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
                    .style(style::primary)
                    .on_press(Message::Setup(SetupMessage::StartFraming(PARTNER))),
            ]
            .spacing(14),
        ]
        .spacing(10)
        .into(),
        None => style::touch_button("Add Partner", style::T_LABEL)
            .width(Length::Fixed(360.0))
            .style(style::secondary)
            .on_press(Message::Setup(SetupMessage::AddPartner))
            .into(),
    };

    column![
        container(portrait).width(Length::Fill).height(Length::FillPortion(4)),
        text(heading).size(style::T_HEADING),
        text(format!("Color identity: {identity}")).size(style::T_BODY),
        primary_row,
        partner_row,
    ]
    .spacing(18)
    .height(Length::Fill)
    .into()
}

/// The last setup step: tap who leads, set which way turns pass, then start.
/// Every seat shows the position it will play in, so the pod can check the
/// order against the real table before anyone draws a card.
fn turn_order_view<'a>(
    state: &'a SetupState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let order = planned_turn_order(state);

    let header = container(
        row![
            column![
                text("Who goes first?").size(style::T_HEADING),
                text("Tap a seat, then choose which way turns pass.").size(style::T_BODY),
            ]
            .spacing(6),
            iced::widget::horizontal_space(),
            style::touch_button("Random", style::T_LABEL)
                .width(Length::Fixed(180.0))
                .style(style::secondary)
                .on_press(Message::Setup(SetupMessage::RandomFirstSeat)),
            style::touch_button("Back", style::T_LABEL)
                .width(Length::Fixed(160.0))
                .style(style::secondary)
                .on_press(Message::Setup(SetupMessage::BackToGridFromTurnOrder)),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .padding(16)
    .width(Length::Fill)
    .style(style::header);

    let board = match &state.table_layout {
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
        None => text("Pick a layout first.").size(style::T_LABEL).into(),
    };

    let direction_buttons = row(
        [TurnDirection::Clockwise, TurnDirection::CounterClockwise]
            .into_iter()
            .map(|dir| {
                let selected = state.turn_direction == dir;
                style::touch_button(dir.label(), 22)
                    .width(Length::Fixed(340.0))
                    .style(if selected {
                        style::primary
                    } else {
                        style::secondary
                    })
                    .on_press(Message::Setup(SetupMessage::SetTurnDirection(dir)))
                    .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(16);

    let summary: Element<Message> = match &order {
        Some(order) => {
            let names: Vec<String> = order
                .iter()
                .filter_map(|&seat| state.seats[seat].player.as_ref())
                .map(|p| p.name.clone())
                .collect();
            // Naming the wrap-around explicitly says which way turns pass
            // without leaning on an arrow glyph the font may not have.
            let leader = names.first().cloned().unwrap_or_default();
            let chain = names.join("  \u{203a}  ");
            container(text(format!("{chain}  \u{203a}  back to {leader}")).size(style::T_ACTION))
                .padding(16)
                .width(Length::Fill)
                .center_x(Length::Fill)
                .style(style::panel)
                .into()
        }
        None => container(text("Pick who takes the first turn.").size(style::T_ACTION))
            .padding(16)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .style(style::panel)
            .into(),
    };

    let mut start_button = style::cta_button("Start Game", style::T_LEAD).width(Length::Fixed(480.0));
    if order.is_some() {
        start_button = start_button
            .style(style::success)
            .on_press(Message::Setup(SetupMessage::StartGame));
    }

    let mut content = column![
        header,
        container(board).height(Length::Fill),
        container(direction_buttons).center_x(Length::Fill),
        summary,
    ]
    .spacing(style::GAP);

    if let Some(e) = &state.error {
        content = content.push(
            container(text(e.clone()).size(style::T_LABEL))
                .padding(16)
                .width(Length::Fill)
                .style(style::panel_danger),
        );
    }

    content = content.push(container(start_button).center_x(Length::Fill));

    container(content.padding(style::GAP))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
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
        return container(text("Empty seat").size(style::T_LABEL))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(style::panel)
            .into();
    };

    let art: Element<Message> = match &seat.commander {
        Some(commander) => art::framed_pair(
            commander,
            seat.partner.as_ref(),
            image_cache,
            18,
            facing.radians(),
        ),
        None => iced::widget::horizontal_space().into(),
    };

    // Centered over the art, matching the in-game life counter: the leader
    // gets the heavier chip, everyone else the lighter one.
    let badge: Element<Message> = match position {
        Some(1) => container(
            container(text("First Player").size(style::T_TITLE))
                .padding([14, 34])
                .style(style::glass_strong),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into(),
        Some(p) => container(
            container(text(format!("#{p}")).size(style::T_TITLE))
                .padding([14, 34])
                .style(style::glass),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into(),
        None => iced::widget::horizontal_space().into(),
    };

    let caption = container(
        container(text(player.name.clone()).size(style::T_SUBHEAD))
            .padding([12, 20])
            .style(style::glass),
    )
    .padding(14)
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
