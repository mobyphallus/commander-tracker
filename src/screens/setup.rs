use std::collections::HashMap;

use iced::widget::{
    button, column, container, image, mouse_area, row, scrollable, stack, text, text_input,
};
use iced::{Alignment, Element, Length, Task};
use rusqlite::Connection;

use crate::app::Message;
use crate::art;
use crate::cards;
use crate::db;
use crate::keyboard::{self, Keyboard};
use crate::layout::{self, SeatOrientation, TableLayout, TurnDirection};
use crate::model::{
    ArtFraming, Commander, Player, SavedDeck, Seat, PARTNER, PRIMARY, STARTING_LIFE,
};
use crate::panned_image;
use crate::scryfall::{self, Cooldown, ScryfallCard, ScryfallError};
use crate::style;

pub const MIN_POD: usize = 2;
pub const MAX_POD: usize = 8;

/// The two text fields in setup. The app's own keyboard types into one at a
/// time and has to be told which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    NewPlayer,
    CommanderQuery,
}

impl Field {
    fn id(self) -> text_input::Id {
        text_input::Id::new(match self {
            Field::NewPlayer => "setup.player",
            Field::CommanderQuery => "setup.commander",
        })
    }

    /// What this field's Enter key does, which is also what the keyboard's
    /// submit key stands in for.
    fn action(self) -> SetupMessage {
        match self {
            Field::NewPlayer => SetupMessage::CreatePlayer,
            Field::CommanderQuery => SetupMessage::SearchCommanders,
        }
    }

    fn action_label(self) -> &'static str {
        match self {
            Field::NewPlayer => "Add",
            Field::CommanderQuery => "Search",
        }
    }
}

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
    /// What their linked Moxfield decks were scored at, by commander id.
    /// Shown on the deck tiles so a pod can pick decks that match without
    /// leaving the table.
    pub deck_links: HashMap<i64, db::DeckLink>,
    /// Whose deck this seat is playing, when it isn't their own.
    pub borrowed_from: Option<Player>,
    pub borrowed_scores: Option<(SavedDeck, cards::DeckMeta)>,
}

impl SeatSetup {
    pub fn commander_in(&self, slot: usize) -> Option<&Commander> {
        match slot {
            PARTNER => self.partner.as_ref(),
            _ => self.commander.as_ref(),
        }
    }

    /// What this seat's linked Moxfield deck was scored at, for the tile to
    /// show. A deck with no link has nothing to say.
    fn deck_meta(&self, deck: &SavedDeck) -> cards::DeckMeta {
        self.deck_links
            .get(&deck.commander.id)
            .map(|link| cards::DeckMeta {
                bracket: link.bracket,
                salt: link.salt_total,
            })
            .unwrap_or_default()
    }

    fn selected_meta(&self) -> cards::DeckMeta {
        // Scores describe the saved list, including its partner configuration.
        let Some(commander) = &self.commander else {
            return cards::DeckMeta::default();
        };
        if self.borrowed_from.is_some() {
            return self
                .borrowed_scores
                .as_ref()
                .filter(|(deck, _)| {
                    deck.commander.id == commander.id
                        && deck.partner.as_ref().map(|p| p.id)
                            == self.partner.as_ref().map(|p| p.id)
                })
                .map(|(_, meta)| *meta)
                .unwrap_or_default();
        }
        let saved = self.commander_history.iter().find(|deck| {
            deck.commander.id == commander.id
                && deck.partner.as_ref().map(|p| p.id) == self.partner.as_ref().map(|p| p.id)
        });
        saved.map(|deck| self.deck_meta(deck)).unwrap_or_default()
    }

    fn commander_label(&self) -> String {
        match (&self.commander, &self.partner) {
            (Some(c), Some(p)) => format!("{} + {}", c.name, p.name),
            (Some(c), None) => c.name.clone(),
            _ => "Choose a commander".into(),
        }
    }

    pub fn set_commander_in(&mut self, slot: usize, commander: Option<Commander>) {
        match slot {
            PARTNER => self.partner = commander,
            _ => self.commander = commander,
        }
    }
}

/// Browsing someone else's collection to borrow a deck from it.
///
/// Two steps in one piece of state: with no `lender` yet it lists everyone
/// who owns a deck; once one is picked it shows that person's decks.
#[derive(Debug, Clone, Default)]
pub struct Borrowing {
    pub lender: Option<Player>,
    pub decks: Vec<SavedDeck>,
    pub links: HashMap<i64, db::DeckLink>,
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
    /// Set while the seat is borrowing a deck from another player.
    pub borrowing: Option<Borrowing>,
    /// The app's own keyboard, and which field it's typing into.
    pub kb: Keyboard<Field>,
    pub error: Option<String>,
    pub score_summary: Option<(String, Option<crate::salt::Analysis>)>,
    pub profile_pictures: HashMap<i64, image::Handle>,
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
            borrowing: None,
            kb: Keyboard::default(),
            error: None,
            score_summary: None,
            profile_pictures: HashMap::new(),
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
        self.borrowing = None;
        self.kb.close();
    }

    /// The text in `field`, for the keyboard to type into.
    fn field_text(&self, field: Field) -> &str {
        match field {
            Field::NewPlayer => &self.new_player_name,
            Field::CommanderQuery => &self.commander_query,
        }
    }

    fn set_field_text(&mut self, field: Field, value: String) {
        match field {
            Field::NewPlayer => self.new_player_name = value,
            Field::CommanderQuery => self.commander_query = value,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SetupMessage {
    OpenScore(i64, i64, String),
    CloseScore,
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
    /// Drop a set of search results and go back to this player's own decks.
    ShowSavedDecks,
    /// Go looking through everyone else's collections.
    StartBorrow,
    /// Browse this player's decks to borrow one.
    PickLender(Player),
    /// Seat a deck belonging to the lender currently being browsed.
    BorrowDeck(SavedDeck),
    CancelBorrow,
    /// A text field was tapped, so the keyboard comes up on it.
    Focus(Field),
    Key(keyboard::Key),
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
        SetupMessage::OpenScore(player, commander, label) => {
            state.score_summary = Some((label, db::deck_breakdown(conn, player, commander)));
            state.kb.close();
            (Task::none(), None)
        }
        SetupMessage::CloseScore => {
            state.score_summary = None;
            (Task::none(), None)
        }
        SetupMessage::ChoosePodSize(n) => {
            if !(MIN_POD..=MAX_POD).contains(&n) {
                state.error = Some("Choose between 2 and 8 players.".into());
                return (Task::none(), None);
            }
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
            state.profile_pictures = crate::screens::players::load_pictures(conn);
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
                    state.seats[seat].deck_links = HashMap::new();
                    state.new_player_name.clear();
                    state.kb.close();
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
            let links = db::deck_links(conn, player.id).unwrap_or_default();
            // Their saved decks are about to be shown as pictures, so fetch
            // them while they're reading the seat's name.
            let art = Task::batch(
                history
                    .iter()
                    .flat_map(|d| [Some(&d.commander), d.partner.as_ref()])
                    .flatten()
                    .filter_map(|c| c.portrait_url().map(str::to_string))
                    .map(|url| {
                        let key = url.clone();
                        Task::perform(scryfall::fetch_image(url), move |res| {
                            Message::ArtLoaded(key.clone(), res)
                        })
                    }),
            );
            state.seats[seat].player = Some(player);
            state.seats[seat].commander_history = history;
            state.seats[seat].deck_links = links;
            state.kb.close();
            (art, None)
        }
        SetupMessage::ClearSeatPlayer => {
            if let Some(seat) = state.editing_seat {
                state.seats[seat].player = None;
                state.seats[seat].commander_history.clear();
                state.seats[seat].deck_links.clear();
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
                    // The card is what's being picked, so the pictures come
                    // down with the names rather than on demand.
                    let thumbs = Task::batch(
                        list.iter()
                            .take(cards::PREFETCH)
                            .filter_map(|c| c.small_url.clone().or_else(|| c.image_url.clone()))
                            .map(|url| {
                                let key = url.clone();
                                Task::perform(scryfall::fetch_image(url), move |res| {
                                    Message::ArtLoaded(key.clone(), res)
                                })
                            }),
                    );
                    state.commander_results = list;
                    state.error = None;
                    return (thumbs, None);
                }
                Err(e) => {
                    state.cooldown.absorb(&e);
                    state.error = Some(e.to_string());
                }
            }
            (Task::none(), None)
        }
        SetupMessage::ShowSavedDecks => {
            state.commander_results.clear();
            state.commander_query.clear();
            state.kb.close();
            (Task::none(), None)
        }
        SetupMessage::StartBorrow => {
            state.borrowing = Some(Borrowing::default());
            state.kb.close();
            (Task::none(), None)
        }
        SetupMessage::PickLender(lender) => {
            let decks = db::player_commander_history(conn, lender.id).unwrap_or_default();
            let links = db::deck_links(conn, lender.id).unwrap_or_default();
            let art = Task::batch(
                decks
                    .iter()
                    .flat_map(|d| [Some(&d.commander), d.partner.as_ref()])
                    .flatten()
                    .filter_map(|c| c.portrait_url().map(str::to_string))
                    .map(|url| {
                        let key = url.clone();
                        Task::perform(scryfall::fetch_image(url), move |res| {
                            Message::ArtLoaded(key.clone(), res)
                        })
                    }),
            );
            state.borrowing = Some(Borrowing {
                lender: Some(lender),
                decks,
                links,
            });
            (art, None)
        }
        SetupMessage::BorrowDeck(deck) => {
            let Some(seat) = state.editing_seat else {
                return (Task::none(), None);
            };
            let Some(lender) = state.borrowing.as_ref().and_then(|b| b.lender.clone()) else {
                return (Task::none(), None);
            };

            if state.editing_slot == PRIMARY {
                let meta = state
                    .borrowing
                    .as_ref()
                    .and_then(|b| b.links.get(&deck.commander.id))
                    .map(|link| cards::DeckMeta {
                        bracket: link.bracket,
                        salt: link.salt_total,
                    })
                    .unwrap_or_default();
                state.seats[seat].borrowed_scores = Some((deck.clone(), meta));
                // A single-commander loan must not retain a previous partner.
                state.seats[seat].partner = None;
            }
            // Deliberately no `record_player_commander_use` for the pilot.
            // That call is what puts a deck in someone's collection, and a
            // borrowed deck stays its owner's - the loan is recorded on the
            // game, not on the borrower. Nor is it recorded for the lender,
            // who isn't the one playing it.
            let mut tasks = vec![load_portrait_task(&deck.commander)];
            let slot = state.editing_slot;
            let commander = resolve_framing(state, conn, seat, deck.commander);
            state.seats[seat].set_commander_in(slot, Some(commander));
            if slot == PRIMARY {
                if let Some(partner) = deck.partner {
                    tasks.push(load_portrait_task(&partner));
                    let partner = resolve_framing(state, conn, seat, partner);
                    state.seats[seat].partner = Some(partner);
                }
                state.seats[seat].borrowed_from = Some(lender);
            }
            state.editing_seat = None;
            state.editing_slot = PRIMARY;
            state.clear_editor_fields();
            (Task::batch(tasks), None)
        }
        SetupMessage::CancelBorrow => {
            state.borrowing = None;
            (Task::none(), None)
        }
        SetupMessage::Focus(field) => {
            let value = state.field_text(field).to_string();
            state.kb.open(field, &value);
            (text_input::focus(field.id()), None)
        }
        SetupMessage::Key(key) => {
            let Some(field) = state.kb.field() else {
                return (Task::none(), None);
            };
            let mut value = state.field_text(field).to_string();
            let outcome = state.kb.press(key, &mut value);
            state.set_field_text(field, value);
            match outcome {
                keyboard::Outcome::Submit => update(state, conn, field.action()),
                _ => (Task::none(), None),
            }
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
                state.seats[seat].borrowed_from = None;
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
                    // A deck picked off Scryfall is this player's own, so
                    // any loan left over from a previous pick is void.
                    if slot == PRIMARY {
                        state.seats[seat].borrowed_from = None;
                    }
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
                // reopen on a seat that still looks half-filled. The loan
                // goes with it - it belonged to the deck, not the seat.
                if slot == PRIMARY {
                    state.seats[seat].partner = None;
                    state.seats[seat].borrowed_from = None;
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
                        .borrowed_from(s.borrowed_from.clone())
                    })
                    .collect();
                (
                    Task::none(),
                    Some(Action::StartGame(seats, layout, turn_order)),
                )
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
    if let Some((label, analysis)) = &state.score_summary {
        let body = match analysis {
            Some(analysis) => crate::screens::breakdown::body(analysis),
            None => empty_state(
                "No saved summary",
                "Link and analyze this deck in Players to see its breakdown.",
            ),
        };
        return step_page(
            step_header(
                pane_eyebrow("DECK SUMMARY".into()),
                label.clone(),
                "Bracket and salt breakdown".into(),
                vec![],
            ),
            body,
            step_footer(
                "Back",
                Message::Setup(SetupMessage::CloseScore),
                footer_hint("Return to your pod"),
            ),
            None,
        );
    }
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
const BACK_W: f32 = 200.0;
const CTA_W: f32 = 280.0;
const UTILITY_W: f32 = 176.0;
/// One printing in the art gallery. Card-shaped, because a printing is a
/// whole card and a box that isn't its shape would make the picture
/// overflow - see the note in [`crate::cards`].
const ART_W: f32 = 200.0;
const ART_H: f32 = ART_W * 204.0 / 146.0;
/// "Step 2 of 4", plus a progress segment per step so the distance left is
/// readable without counting words.
fn step_eyebrow<'a>(step: usize) -> Element<'a, Message> {
    let dots = row((1..=STEP_COUNT)
        .map(|n| {
            container(iced::widget::Space::new(32, 4))
                .style(if n <= step {
                    style::meter_fill
                } else {
                    style::meter_track
                })
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
        text(instruction)
            .size(style::T_LABEL)
            .color(style::TEXT_MUTED),
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
        style::icon_button(crate::icon::Glyph::Back, back_label, style::T_ACTION)
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
fn forward_action(
    label: &str,
    message: Option<Message>,
    blocked: &str,
) -> Element<'static, Message> {
    let enabled = message.is_some();
    let mut cta = style::icon_button(
        crate::icon::Glyph::Check,
        label.to_string(),
        style::T_ACTION,
    )
    .height(style::TOUCH_H_LG)
    .width(Length::Fixed(CTA_W))
    .style(style::primary);
    if let Some(message) = message {
        cta = cta.on_press(message);
    }

    let mut bar = column![].spacing(PAD_TIGHT).align_x(Alignment::End);
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

// ---------------------------------------------------------------------------
// Text entry
//
// There is no hardware keyboard on the table and iced 0.13 can't ask the
// compositor for one, so a field here raises the app's own.
// ---------------------------------------------------------------------------

/// A text field that brings the keyboard up when it's tapped.
///
/// `text_input` captures the press that focuses it but lets the release go
/// past, so the release is what the keyboard listens for. The field keeps
/// its own caret, and a real keyboard still works alongside.
fn keyed_field<'a>(
    field: Field,
    placeholder: &'a str,
    value: &'a str,
    on_input: fn(String) -> Message,
) -> Element<'a, Message> {
    mouse_area(
        text_input(placeholder, value)
            .id(field.id())
            .size(style::T_SUBHEAD)
            .padding(style::FIELD_PAD)
            .style(style::input)
            .on_input(on_input)
            .on_submit(Message::Setup(field.action())),
    )
    .on_release(Message::Setup(SetupMessage::Focus(field)))
    .into()
}

/// Puts the keyboard under a pane while one of its fields is being typed
/// into. It takes its space from the pane rather than floating over it:
/// what you're typing into is the thing you most need to keep seeing.
fn with_keyboard<'a>(state: &SetupState, body: Element<'a, Message>) -> Element<'a, Message> {
    let Some(field) = state.kb.field() else {
        return body;
    };
    column![
        container(body).height(Length::Fill),
        keyboard::view(
            &state.kb,
            |key| Message::Setup(SetupMessage::Key(key)),
            Some(field.action_label()),
        ),
    ]
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

// ---------------------------------------------------------------------------
// Step 1 - how many players
// ---------------------------------------------------------------------------

fn pod_size_view(state: &SetupState) -> Element<'_, Message> {
    let body = iced::widget::responsive(move |size| {
        let height = ((size.height - PAD) / 2.).max(220.);
        let first = row((2..=5)
            .map(|n| pod_choice(n, state.pod_size == n, height))
            .collect::<Vec<_>>())
        .spacing(PAD);
        let second = row((6..=8)
            .map(|n| pod_choice(n, state.pod_size == n, height))
            .collect::<Vec<_>>())
        .spacing(PAD);
        scrollable(
            column![first, second]
                .spacing(PAD)
                .padding([0., PAD_DOUBLE]),
        )
        .height(Length::Fill)
        .into()
    });
    step_page(
        step_header(
            step_eyebrow(1),
            "How many are playing?".into(),
            "Choose your pod size. Everyone starts at 40 life.".into(),
            Vec::new(),
        ),
        body.into(),
        step_footer(
            "Home",
            Message::GoHome,
            footer_hint("2–8 players · Tap a card to continue"),
        ),
        state.error.as_deref(),
    )
}

fn pod_choice(count: usize, selected: bool, height: f32) -> Element<'static, Message> {
    let table = layout::options_for(count).remove(0);
    button(
        column![
            row![
                text(count.to_string()).size(style::T_DISPLAY),
                text("players")
                    .size(style::T_LABEL)
                    .color(style::TEXT_MUTED),
                iced::widget::horizontal_space(),
                crate::icon::view(crate::icon::Glyph::Next, 24., style::ACCENT_BRIGHT),
            ]
            .spacing(PAD_HALF)
            .align_y(Alignment::Center),
            container(crate::table_preview::view(&table))
                .height(Length::Fill)
                .padding(PAD_HALF),
        ]
        .spacing(PAD),
    )
    .padding(PAD_DOUBLE)
    .width(Length::Fill)
    .height(height)
    .style(if selected {
        style::tile_selected
    } else {
        style::row_button
    })
    .on_press(Message::Setup(SetupMessage::ChoosePodSize(count)))
    .into()
}

// ---------------------------------------------------------------------------
// Step 2 - how the table is arranged
// ---------------------------------------------------------------------------

fn layout_description(table: &TableLayout) -> &'static str {
    match table.name.as_str() {
        "Stacked" => "Face each other across the screen",
        "Two Sides" | "Three Pairs" | "Four Pairs" => "Split evenly along the two long sides",
        "Two Heads" => "One player at each end of the table",
        "Head Left" => "One player at the left end",
        "Head Right" => "One player at the right end",
        _ => "Match the seats to your table",
    }
}

fn layout_card(table: &TableLayout, selected: bool, height: f32) -> Element<'static, Message> {
    button(
        column![
            row![
                text(table.name.clone()).size(style::T_HEADING),
                iced::widget::horizontal_space(),
                crate::icon::view(
                    if selected {
                        crate::icon::Glyph::Check
                    } else {
                        crate::icon::Glyph::Next
                    },
                    28.,
                    style::ACCENT_BRIGHT
                ),
            ]
            .align_y(Alignment::Center)
            .spacing(PAD),
            text(layout_description(table))
                .size(style::T_LABEL)
                .color(style::TEXT_MUTED),
            container(crate::table_preview::view(table))
                .height(Length::Fill)
                .padding(PAD),
            row![
                text(format!("{} seats", table.ring_order().len()))
                    .size(style::T_CAPTION)
                    .color(style::TEXT_MUTED),
                iced::widget::horizontal_space(),
                text(if selected {
                    "Use this layout"
                } else {
                    "Choose layout"
                })
                .size(style::T_ACTION)
                .color(style::ACCENT_BRIGHT),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(PAD),
    )
    .padding(PAD_DOUBLE)
    .height(height)
    .width(Length::Fill)
    .style(if selected {
        style::tile_selected
    } else {
        style::row_button
    })
    .on_press(Message::Setup(SetupMessage::ChooseLayout(table.clone())))
    .into()
}

fn layout_choice_view(state: &SetupState) -> Element<'_, Message> {
    let body = iced::widget::responsive(move |size| {
        let options = layout::options_for(state.pod_size);
        let chosen = state.table_layout.as_ref();
        let stacked = size.width < 1000. && options.len() > 1;
        let height = if stacked {
            ((size.height - PAD) / 2.).max(300.)
        } else {
            size.height.max(360.)
        };
        let cards: Vec<Element<Message>> = options
            .iter()
            .map(|table| layout_card(table, chosen == Some(table), height))
            .collect();
        let choices: Element<Message> = if stacked {
            column(cards).spacing(PAD).into()
        } else {
            row(cards).spacing(PAD).into()
        };
        scrollable(
            container(choices)
                .padding([0., PAD_DOUBLE])
                .width(Length::Fill),
        )
        .height(Length::Fill)
        .into()
    });
    step_page(
        step_header(
            step_eyebrow(2),
            "Make room at the table".into(),
            format!(
                "{} players · Choose the layout that matches where everyone is sitting.",
                state.pod_size
            ),
            Vec::new(),
        ),
        body.into(),
        step_footer(
            "Back",
            Message::Setup(SetupMessage::BackToPodSizeChoice),
            footer_hint("Numbers mark seats · Purple lines face each player"),
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
                        text(seat.commander_label())
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

    container(stack![
        button(content)
            .padding(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::ghost)
            .on_press(Message::Setup(SetupMessage::EditSeat(index))),
        score_overlay(seat),
    ])
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
                    forward_action("Done", Some(Message::Setup(SetupMessage::DoneFraming)), ""),
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
        } else if let Some(borrowing) = &state.borrowing {
            let (title, instruction, hint) = match &borrowing.lender {
                Some(lender) => (
                    format!("Borrow from {}", lender.name),
                    format!(
                        "Whatever they win with counts for {} and for the deck - never for {}.",
                        seat.player
                            .as_ref()
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| "this seat".into()),
                        lender.name
                    ),
                    "Tap a deck to play it",
                ),
                None => (
                    "Whose deck?".to_string(),
                    "Anyone with decks on record can lend one, whether or not they're playing."
                        .to_string(),
                    "Tap a name to see their decks",
                ),
            };
            (
                title,
                instruction,
                borrow_picker(borrowing, players_cache, image_cache),
                step_footer(
                    "Back",
                    Message::Setup(SetupMessage::CancelBorrow),
                    footer_hint(hint),
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
                "Tap one this player has run before, borrow one, or search Scryfall.".to_string(),
                commander_picker(state, seat, image_cache),
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

    with_keyboard(
        state,
        step_page(
            step_header(
                pane_eyebrow(format!(
                    "STEP 3 OF {STEP_COUNT} \u{00b7} SEAT {}",
                    seat_index + 1
                )),
                title,
                instruction,
                Vec::new(),
            ),
            body,
            footer,
            state.error.as_deref(),
        ),
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
        crate::screens::players::player_grid(
            available,
            &state.profile_pictures,
            "Choose player",
            |p| Message::Setup(SetupMessage::PickExistingPlayer(p)),
        )
    };

    let has_name = !state.new_player_name.trim().is_empty();
    let mut add = style::icon_button(crate::icon::Glyph::Add, "Add Player", style::T_ACTION)
        .width(Length::Fixed(UTILITY_W))
        .style(style::primary);
    if has_name {
        add = add.on_press(Message::Setup(SetupMessage::CreatePlayer));
    }

    column![
        list,
        row![
            keyed_field(
                Field::NewPlayer,
                "New player name",
                &state.new_player_name,
                |s| Message::Setup(SetupMessage::NewPlayerNameChanged(s)),
            ),
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

/// Picking this seat's commander: the decks this player already runs, then
/// Scryfall for anything else.
///
/// Both are grids of cards, because a commander is a picture first. The old
/// list of names put their whole deck list in a two-row strip you had to
/// scroll inside another scroll, which meant reading ten labels to find the
/// deck they play every week.
fn commander_picker<'a>(
    state: &'a SetupState,
    seat: &'a SeatSetup,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let history = &seat.commander_history;
    // Search results take the whole pane while they're up, rather than
    // splitting it with the saved decks and leaving both cramped.
    let showing_results = state.searching || !state.commander_results.is_empty();

    let body: Element<Message> = if state.searching {
        empty_state(
            "Searching Scryfall\u{2026}",
            "Looking for commanders whose name matches what you typed.",
        )
    } else if !state.commander_results.is_empty() {
        cards::grid(
            state
                .commander_results
                .iter()
                .map(|card| {
                    cards::card_tile(
                        card,
                        image_cache,
                        Message::Setup(SetupMessage::PickCommanderName(card.clone())),
                    )
                })
                .collect(),
        )
    } else if state.cooldown.active() {
        empty_state(
            "Scryfall asked us to slow down",
            "Search comes back as soon as the countdown on the button runs out.",
        )
    } else if history.is_empty() {
        empty_state(
            "Nothing on record for this player yet",
            "Search for their commander by name and it'll be waiting here next time.",
        )
    } else {
        cards::adaptive_grid(history.len(), move |i, width| {
            let deck = &history[i];
            cards::deck_tile(
                deck,
                false,
                image_cache,
                seat.deck_meta(deck),
                Message::Setup(SetupMessage::PickHistoryCommander(deck.clone())),
                Message::Setup(SetupMessage::OpenScore(
                    seat.player.as_ref().unwrap().id,
                    deck.commander.id,
                    deck.label(),
                )),
                width,
            )
        })
    };

    let caption: Element<Message> = if showing_results {
        row![
            text("Results from Scryfall")
                .size(style::T_CAPTION)
                .color(style::TEXT_MUTED)
                .width(Length::Fill),
            style::touch_button("Their Decks", style::T_LABEL)
                .width(Length::Fixed(UTILITY_W))
                .style(style::ghost)
                .on_press(Message::Setup(SetupMessage::ShowSavedDecks)),
        ]
        .align_y(Alignment::Center)
        .into()
    } else {
        text(match history.len() {
            0 => "No saved decks".to_string(),
            1 => "1 deck this player runs".to_string(),
            n => format!("{n} decks this player runs"),
        })
        .size(style::T_CAPTION)
        .color(style::TEXT_MUTED)
        .into()
    };

    let search_label: String = if state.cooldown.active() {
        state.cooldown.label()
    } else if state.searching {
        "Searching\u{2026}".into()
    } else {
        "Search".into()
    };

    let mut search_button =
        style::icon_button(crate::icon::Glyph::Search, search_label, style::T_ACTION)
            .width(Length::Fixed(UTILITY_W))
            .style(style::primary);
    if !state.cooldown.active() && !state.searching && !state.commander_query.trim().is_empty() {
        search_button = search_button.on_press(Message::Setup(SetupMessage::SearchCommanders));
    }

    column![
        row![
            keyed_field(
                Field::CommanderQuery,
                "Commander name",
                &state.commander_query,
                |s| Message::Setup(SetupMessage::CommanderQueryChanged(s)),
            ),
            search_button,
            style::touch_button("Borrow", style::T_ACTION)
                .width(Length::Fixed(UTILITY_W))
                .style(style::secondary)
                .on_press(Message::Setup(SetupMessage::StartBorrow)),
        ]
        .spacing(PAD)
        .align_y(Alignment::Center),
        caption,
        body,
    ]
    .spacing(PAD)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Borrowing, in two steps: whose collection, then which deck of theirs.
///
/// The lender list deliberately isn't limited to the people at this table -
/// a deck's owner doesn't have to be playing for someone to sleeve it up.
fn borrow_picker<'a>(
    borrowing: &'a Borrowing,
    players_cache: &'a [Player],
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    match &borrowing.lender {
        None => {
            let lenders: Vec<Element<Message>> = players_cache
                .iter()
                .map(|p| {
                    style::name_tile(&p.name)
                        .style(style::secondary)
                        .on_press(Message::Setup(SetupMessage::PickLender(p.clone())))
                        .into()
                })
                .collect();
            if lenders.is_empty() {
                empty_state(
                    "Nobody to borrow from",
                    "No one on the roster has a deck saved yet.",
                )
            } else {
                cards::grid(lenders)
            }
        }
        Some(_) => {
            if borrowing.decks.is_empty() {
                return empty_state(
                    "No decks on record",
                    "This player hasn't saved a deck yet, so there's nothing to lend.",
                );
            }
            cards::adaptive_grid(borrowing.decks.len(), move |i, width| {
                let deck = &borrowing.decks[i];
                let meta = borrowing
                    .links
                    .get(&deck.commander.id)
                    .map(|link| cards::DeckMeta {
                        bracket: link.bracket,
                        salt: link.salt_total,
                    })
                    .unwrap_or_default();
                cards::deck_tile(
                    deck,
                    false,
                    image_cache,
                    meta,
                    Message::Setup(SetupMessage::BorrowDeck(deck.clone())),
                    Message::Setup(SetupMessage::OpenScore(
                        borrowing.lender.as_ref().unwrap().id,
                        deck.commander.id,
                        deck.label(),
                    )),
                    width,
                )
            })
        }
    }
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
            let thumb: Element<Message> =
                match card.small_url.as_deref().and_then(|u| image_cache.get(u)) {
                    Some(handle) => cards::card_picture(handle, ART_W),
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

    cards::grid(tiles)
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

    let surface = container(panned_image::editable(
        handle,
        commander.framing,
        |framing| Message::Setup(SetupMessage::FramingChanged(framing)),
    ))
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
        container(surface)
            .width(Length::Fill)
            .center_x(Length::Fill),
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

    iced::widget::responsive(move |size| {
        let portrait = art::framed_pair(
            commander,
            seat.partner.as_ref(),
            image_cache,
            style::T_LABEL,
            0.0,
        );
        let deck = SavedDeck {
            commander: commander.clone(),
            partner: seat.partner.clone(),
        };
        let identity = cards::identity(&deck);
        let action = |glyph, label: &'static str, message| {
            style::icon_button(glyph, label, style::T_BODY)
                .width(Length::Fill)
                .style(style::secondary)
                .on_press(Message::Setup(message))
        };
        let mut actions = column![
            text("PLAYER")
                .size(style::T_CAPTION)
                .color(style::ACCENT_BRIGHT),
            text(player.name.clone()).size(style::T_TITLE),
            action(
                crate::icon::Glyph::Players,
                "Change player",
                SetupMessage::ClearSeatPlayer
            ),
            text("COMMANDER")
                .size(style::T_CAPTION)
                .color(style::ACCENT_BRIGHT),
            text(commander.name.clone()).size(style::T_SUBHEAD),
            cards::mana_row(&identity),
            action(
                crate::icon::Glyph::Decks,
                "Change commander",
                SetupMessage::ClearSeatCommander(PRIMARY)
            ),
            row![
                action(
                    crate::icon::Glyph::Image,
                    "Change art",
                    SetupMessage::ChangeArt(PRIMARY)
                ),
                action(
                    crate::icon::Glyph::Frame,
                    "Frame art",
                    SetupMessage::StartFraming(PRIMARY)
                ),
            ]
            .spacing(PAD_HALF),
        ]
        .spacing(PAD);
        if let Some(partner) = &seat.partner {
            actions = actions
                .push(
                    text("PARTNER")
                        .size(style::T_CAPTION)
                        .color(style::ACCENT_BRIGHT),
                )
                .push(text(partner.name.clone()).size(style::T_LABEL))
                .push(
                    row![
                        action(
                            crate::icon::Glyph::Image,
                            "Change art",
                            SetupMessage::ChangeArt(PARTNER)
                        ),
                        action(
                            crate::icon::Glyph::Frame,
                            "Frame art",
                            SetupMessage::StartFraming(PARTNER)
                        ),
                    ]
                    .spacing(PAD_HALF),
                )
                .push(
                    style::icon_button(crate::icon::Glyph::Delete, "Remove partner", style::T_BODY)
                        .width(Length::Fill)
                        .style(style::danger)
                        .on_press(Message::Setup(SetupMessage::ClearSeatCommander(PARTNER))),
                );
        } else {
            actions = actions.push(action(
                crate::icon::Glyph::Add,
                "Add partner",
                SetupMessage::AddPartner,
            ));
        }
        let settings = container(scrollable(actions))
            .padding(PAD_DOUBLE)
            .style(style::panel);
        let preview = container(stack![portrait, score_overlay(seat)])
            .padding(3)
            .style(style::panel)
            .clip(true);
        if size.width >= 1000.0 {
            row![
                preview.width(Length::Fill).height(Length::Fill),
                settings.width(460).height(Length::Fill)
            ]
            .spacing(PAD)
            .into()
        } else {
            column![
                preview.width(Length::Fill).height(Length::FillPortion(2)),
                settings.width(Length::Fill).height(Length::FillPortion(3))
            ]
            .spacing(PAD)
            .into()
        }
    })
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

    let body = iced::widget::responsive(move |size| {
        let directions = column(
            [TurnDirection::Clockwise, TurnDirection::CounterClockwise]
                .into_iter()
                .map(|dir| {
                    let selected = state.turn_direction == dir;
                    style::icon_button(
                        crate::icon::Glyph::Rotate(dir == TurnDirection::Clockwise),
                        dir.label(),
                        style::T_BODY,
                    )
                    .width(Length::Fill)
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
        .spacing(PAD_HALF);
        let sequence: Element<Message> = match planned_turn_order(state) {
            Some(order) => column(
                order
                    .iter()
                    .enumerate()
                    .map(|(position, &idx)| {
                        let name = state.seats[idx]
                            .player
                            .as_ref()
                            .map(|p| p.name.clone())
                            .unwrap_or_default();
                        container(
                            row![
                                container(
                                    text(format!("{}", position + 1))
                                        .size(style::T_LABEL)
                                        .color(style::ACCENT_BRIGHT)
                                )
                                .center_x(40)
                                .center_y(40)
                                .style(style::panel_active),
                                column![
                                    text(name).size(style::T_LABEL),
                                    text(if position == 0 {
                                        "First turn".to_string()
                                    } else {
                                        format!("Seat {}", idx + 1)
                                    })
                                    .size(style::T_CAPTION)
                                    .color(style::TEXT_MUTED)
                                ]
                                .spacing(2),
                            ]
                            .spacing(PAD)
                            .align_y(Alignment::Center),
                        )
                        .padding(PAD_HALF)
                        .width(Length::Fill)
                        .style(style::table_row)
                        .into()
                    })
                    .collect::<Vec<Element<Message>>>(),
            )
            .spacing(PAD_HALF)
            .into(),
            None => container(
                column![
                    crate::icon::view(crate::icon::Glyph::Players, 32.0, style::ACCENT_BRIGHT),
                    text("Choose the first player").size(style::T_SUBHEAD),
                    text("Tap a seat on the table, or let Random choose for you.")
                        .size(style::T_BODY)
                        .color(style::TEXT_MUTED),
                ]
                .spacing(PAD),
            )
            .padding(PAD)
            .into(),
        };
        let controls = container(
            column![
                text("TURN DIRECTION")
                    .size(style::T_CAPTION)
                    .color(style::ACCENT_BRIGHT),
                directions,
                text("UP NEXT")
                    .size(style::T_CAPTION)
                    .color(style::ACCENT_BRIGHT),
                scrollable(column![sequence]).height(Length::Fill),
            ]
            .spacing(PAD),
        )
        .padding(PAD_DOUBLE)
        .style(style::panel);
        // Build the table inside responsive: each preview keeps its share of
        // the available height and the sequence scrolls independently.
        let order = planned_turn_order(state);
        let board: Element<Message> = match &state.table_layout {
            Some(table) => layout::render_table(table, |idx| {
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
            None => empty_state("No table yet", "Go back and choose a layout."),
        };
        if size.width >= 1100.0 {
            row![
                container(board).width(Length::Fill).height(Length::Fill),
                controls.width(360).height(Length::Fill)
            ]
            .spacing(PAD)
            .into()
        } else {
            column![
                container(board)
                    .width(Length::Fill)
                    .height(Length::FillPortion(3)),
                controls.width(Length::Fill).height(Length::FillPortion(2))
            ]
            .spacing(PAD)
            .into()
        }
    });

    step_page(
        step_header(
            step_eyebrow(4),
            "Who goes first?".to_string(),
            "Tap a seat, then choose which way turns pass.".to_string(),
            vec![
                style::icon_button(crate::icon::Glyph::Shuffle, "Random", style::T_BODY)
                    .width(Length::Fixed(UTILITY_W))
                    .style(style::secondary)
                    .on_press(Message::Setup(SetupMessage::RandomFirstSeat))
                    .into(),
            ],
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

/// Reuse the saved-deck badges without taking space away from the art.
fn score_overlay(seat: &SeatSetup) -> Element<'_, Message> {
    let (Some(player), Some(commander)) = (&seat.player, &seat.commander) else {
        return iced::widget::horizontal_space()
            .width(Length::Shrink)
            .into();
    };
    container(cards::score_button(
        seat.selected_meta(),
        Message::Setup(SetupMessage::OpenScore(
            seat.borrowed_from.as_ref().unwrap_or(player).id,
            commander.id,
            seat.commander_label(),
        )),
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Alignment::End)
    .align_y(Alignment::Start)
    .padding(PAD_HALF)
    .into()
}

/// Host-facing preview, with the turn number separated from the player caption.
fn turn_order_tile<'a>(
    index: usize,
    seat: &'a SeatSetup,
    position: Option<usize>,
    _facing: SeatOrientation,
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
            0.0,
        ),
        None => iced::widget::horizontal_space().into(),
    };

    let badge = container(
        container(
            text(match position {
                Some(1) => "1 · First turn".to_string(),
                Some(p) => format!("{p} · Turn order"),
                None => format!("Seat {}", index + 1),
            })
            .size(style::T_LABEL)
            .color(style::TEXT),
        )
        .padding([PAD_HALF, PAD])
        .style(if position == Some(1) {
            style::panel_active
        } else {
            style::glass_strong
        }),
    )
    .padding(PAD)
    .width(Length::Fill)
    .height(Length::Fill)
    .align_y(Alignment::Start);

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
        .padding(3)
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

#[cfg(test)]
mod comparison_tests {
    use super::*;

    fn commander(id: i64) -> Commander {
        Commander {
            id,
            oracle_id: id.to_string(),
            name: format!("Commander {id}"),
            image_url: None,
            art_crop_url: None,
            color_identity: String::new(),
            framing: ArtFraming::default(),
        }
    }

    #[test]
    fn scores_follow_the_saved_pair_and_clear_when_partner_changes() {
        let deck = SavedDeck {
            commander: commander(1),
            partner: Some(commander(2)),
        };
        let mut seat = SeatSetup {
            commander: Some(deck.commander.clone()),
            partner: deck.partner.clone(),
            commander_history: vec![deck],
            ..SeatSetup::default()
        };
        seat.deck_links.insert(
            1,
            db::DeckLink {
                public_id: "deck".into(),
                url: String::new(),
                deck_name: "Deck".into(),
                bracket: Some(3),
                salt_total: Some(123.4),
            },
        );
        assert_eq!(
            seat.selected_meta(),
            cards::DeckMeta {
                bracket: Some(3),
                salt: Some(123.4)
            }
        );
        seat.partner = Some(commander(3));
        assert_eq!(seat.selected_meta(), cards::DeckMeta::default());
        seat.partner = None;
        assert_eq!(seat.selected_meta(), cards::DeckMeta::default());
    }

    #[test]
    fn missing_scores_are_not_presented_as_zero() {
        assert_eq!(
            SeatSetup::default().selected_meta(),
            cards::DeckMeta::default()
        );
    }

    #[test]
    fn borrowing_carries_owner_scores_and_drops_a_previous_partner() {
        let conn = Connection::open_in_memory().unwrap();
        let lender = Player {
            id: 2,
            name: "Owner".into(),
        };
        let deck = SavedDeck {
            commander: commander(1),
            partner: None,
        };
        let link = db::DeckLink {
            public_id: "owner-list".into(),
            url: String::new(),
            deck_name: "Owner deck".into(),
            bracket: Some(4),
            salt_total: Some(222.0),
        };
        let mut state = SetupState::new();
        state.seats.push(SeatSetup {
            player: Some(Player {
                id: 1,
                name: "Borrower".into(),
            }),
            partner: Some(commander(3)),
            commander_history: vec![deck.clone()],
            ..Default::default()
        });
        state.editing_seat = Some(0);
        state.borrowing = Some(Borrowing {
            lender: Some(lender.clone()),
            decks: vec![deck.clone()],
            links: HashMap::from([(1, link)]),
        });
        let _ = update(&mut state, &conn, SetupMessage::BorrowDeck(deck));
        assert_eq!(state.seats[0].borrowed_from, Some(lender));
        assert!(state.seats[0].partner.is_none());
        assert_eq!(
            state.seats[0].selected_meta(),
            cards::DeckMeta {
                bracket: Some(4),
                salt: Some(222.0)
            }
        );
        state.seats[0].partner = Some(commander(8));
        assert_eq!(state.seats[0].selected_meta(), cards::DeckMeta::default());
        state.seats[0].partner = None;
        state.seats[0].borrowed_from = None;
        assert_eq!(
            state.seats[0].selected_meta(),
            cards::DeckMeta::default(),
            "owner scores must not leak into the borrower's own deck"
        );
    }
}

#[cfg(test)]
mod pod_limit_tests {
    use super::*;

    #[test]
    fn pod_size_is_limited_to_two_through_eight_without_resetting_valid_seats() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let mut state = SetupState::new();
        for n in MIN_POD..=MAX_POD {
            let _ = update(&mut state, &conn, SetupMessage::ChoosePodSize(n));
            assert_eq!(state.seats.len(), n);
            assert_eq!(state.stage, SetupStage::ChooseLayout);
        }
        for n in [0, 1, 9, usize::MAX] {
            let _ = update(&mut state, &conn, SetupMessage::ChoosePodSize(n));
            assert_eq!(state.pod_size, MAX_POD);
            assert_eq!(state.seats.len(), MAX_POD);
            assert!(state.error.is_some());
            assert!(layout::options_for(n).is_empty());
        }
    }
}
