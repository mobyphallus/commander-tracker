use std::collections::HashMap;

use iced::widget::{button, column, container, image, row, scrollable, text, text_input};
use iced::{ContentFit, Element, Length, Task};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::model::{Commander, Player, SavedDeck};
use crate::scryfall::{self, Cooldown, ScryfallCard, ScryfallError};
use crate::style;

pub struct PlayersState {
    pub players: Vec<Player>,
    pub new_player_name: String,
    pub editing: Option<(i64, String)>,
    /// Seat of a pending delete, so it takes two taps to remove someone.
    pub confirming_delete: Option<i64>,
    /// When set, we're managing this player's commander list instead of the
    /// roster.
    pub managing: Option<ManagedPlayer>,
    /// Non-zero while Scryfall has us locked out for exceeding the rate
    /// limit. Lives on the screen rather than on `managing` so it survives
    /// closing and reopening a player's commander list.
    pub cooldown: Cooldown,
    pub error: Option<String>,
}

pub struct ManagedPlayer {
    pub player: Player,
    pub commanders: Vec<SavedDeck>,
    pub query: String,
    pub results: Vec<ScryfallCard>,
    pub searching: bool,
    /// Set while picking a different printing's art for one of this
    /// player's commanders.
    pub art_for: Option<Commander>,
    pub art_options: Vec<ScryfallCard>,
    pub loading_art: bool,
    /// The commander whose partner is being chosen, if any.
    pub pairing: Option<Commander>,
}

impl PlayersState {
    pub fn load(conn: &Connection) -> Self {
        Self {
            players: db::list_players(conn).unwrap_or_default(),
            new_player_name: String::new(),
            editing: None,
            confirming_delete: None,
            managing: None,
            cooldown: Cooldown::default(),
            error: None,
        }
    }

    fn refresh(&mut self, conn: &Connection) {
        self.players = db::list_players(conn).unwrap_or_default();
    }
}

#[derive(Debug, Clone)]
pub enum PlayersMessage {
    NewNameChanged(String),
    CreatePlayer,
    StartEdit(i64, String),
    NameChanged(String),
    Save,
    Cancel,
    AskDelete(i64),
    ConfirmDelete(i64),
    CancelDelete,
    ManageCommanders(Player),
    CloseManage,
    QueryChanged(String),
    Search,
    SearchResults(Result<Vec<ScryfallCard>, ScryfallError>),
    AddCommander(ScryfallCard),
    RemoveCommander(i64),
    ChangeArt(Commander),
    ArtOptionsLoaded(Result<Vec<ScryfallCard>, ScryfallError>),
    PickArt(ScryfallCard),
    CancelArt,
    StartPairing(Commander),
    PickPartner(Commander),
    Unpair(Commander),
    CancelPairing,
    CooldownTick,
}

pub fn update(
    state: &mut PlayersState,
    conn: &Connection,
    message: PlayersMessage,
) -> Task<Message> {
    state.error = None;
    match message {
        PlayersMessage::NewNameChanged(s) => state.new_player_name = s,
        PlayersMessage::CreatePlayer => {
            let name = state.new_player_name.trim().to_string();
            if name.is_empty() {
                return Task::none();
            }
            match db::create_player(conn, &name) {
                Ok(_) => {
                    state.new_player_name.clear();
                    state.refresh(conn);
                }
                Err(e) => state.error = Some(format!("Couldn't add player: {e}")),
            }
        }
        PlayersMessage::StartEdit(id, name) => {
            state.editing = Some((id, name));
            state.confirming_delete = None;
        }
        PlayersMessage::NameChanged(s) => {
            if let Some((_, name)) = &mut state.editing {
                *name = s;
            }
        }
        PlayersMessage::Save => {
            if let Some((id, name)) = state.editing.take() {
                let trimmed = name.trim().to_string();
                if trimmed.is_empty() {
                    state.error = Some("Name can't be empty.".into());
                } else {
                    match db::rename_player(conn, id, &trimmed) {
                        Ok(()) => state.refresh(conn),
                        Err(e) => state.error = Some(format!("Couldn't rename: {e}")),
                    }
                }
            }
        }
        PlayersMessage::Cancel => state.editing = None,
        PlayersMessage::AskDelete(id) => {
            state.confirming_delete = Some(id);
            state.editing = None;
        }
        PlayersMessage::CancelDelete => state.confirming_delete = None,
        PlayersMessage::ConfirmDelete(id) => {
            state.confirming_delete = None;
            match db::player_game_count(conn, id) {
                Ok(0) => match db::delete_player(conn, id) {
                    Ok(()) => state.refresh(conn),
                    Err(e) => state.error = Some(format!("Couldn't delete: {e}")),
                },
                Ok(n) => {
                    state.error = Some(format!(
                        "That player is in {n} recorded game(s), so their history would break. Rename them instead."
                    ))
                }
                Err(e) => state.error = Some(format!("Couldn't check history: {e}")),
            }
        }
        PlayersMessage::ManageCommanders(player) => {
            let commanders = db::player_commander_history(conn, player.id).unwrap_or_default();
            state.managing = Some(ManagedPlayer {
                player,
                commanders,
                query: String::new(),
                results: Vec::new(),
                searching: false,
                art_for: None,
                art_options: Vec::new(),
                loading_art: false,
                pairing: None,
            });
        }
        PlayersMessage::CloseManage => state.managing = None,
        PlayersMessage::QueryChanged(s) => {
            if let Some(m) = &mut state.managing {
                m.query = s;
            }
        }
        PlayersMessage::Search => {
            if state.cooldown.active() {
                return Task::none();
            }
            if let Some(m) = &mut state.managing {
                let query = m.query.clone();
                if query.trim().is_empty() {
                    return Task::none();
                }
                m.searching = true;
                return Task::perform(scryfall::search_commanders(query), |res| {
                    Message::Players(PlayersMessage::SearchResults(res))
                });
            }
        }
        PlayersMessage::SearchResults(res) => {
            if let Some(m) = &mut state.managing {
                m.searching = false;
            }
            match res {
                Ok(list) => {
                    if let Some(m) = &mut state.managing {
                        m.results = list;
                    }
                    state.error = None;
                }
                Err(e) => {
                    state.cooldown.absorb(&e);
                    state.error = Some(e.to_string());
                }
            }
        }
        PlayersMessage::CooldownTick => {
            state.cooldown.tick();
            if !state.cooldown.active() {
                state.error = None;
            }
        }
        PlayersMessage::AddCommander(card) => {
            let Some(m) = &mut state.managing else {
                return Task::none();
            };
            match db::upsert_commander(
                conn,
                &card.oracle_id,
                &card.name,
                card.image_url.as_deref(),
                card.art_crop_url.as_deref(),
                &card.color_identity,
            ) {
                Ok(commander) => {
                    let _ = db::record_player_commander_use(conn, m.player.id, commander.id);
                    m.commanders =
                        db::player_commander_history(conn, m.player.id).unwrap_or_default();
                    m.results.clear();
                    m.query.clear();
                }
                Err(e) => state.error = Some(format!("Couldn't add commander: {e}")),
            }
        }
        PlayersMessage::RemoveCommander(commander_id) => {
            if let Some(m) = &mut state.managing {
                match db::remove_player_commander(conn, m.player.id, commander_id) {
                    Ok(()) => {
                        m.commanders =
                            db::player_commander_history(conn, m.player.id).unwrap_or_default()
                    }
                    Err(e) => state.error = Some(format!("Couldn't remove: {e}")),
                }
            }
        }
        PlayersMessage::ChangeArt(commander) => {
            if let Some(m) = &mut state.managing {
                let oracle_id = commander.oracle_id.clone();
                m.art_for = Some(commander);
                m.art_options.clear();
                m.loading_art = true;
                return Task::perform(scryfall::fetch_prints(oracle_id), |res| {
                    Message::Players(PlayersMessage::ArtOptionsLoaded(res))
                });
            }
        }
        PlayersMessage::ArtOptionsLoaded(res) => {
            if let Some(m) = &mut state.managing {
                m.loading_art = false;
            }
            match res {
                Ok(list) => {
                    let thumbs: Vec<Task<Message>> = list
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
                    if let Some(m) = &mut state.managing {
                        m.art_options = list;
                    }
                    return Task::batch(thumbs);
                }
                Err(e) => {
                    state.cooldown.absorb(&e);
                    state.error = Some(e.to_string());
                }
            }
        }
        PlayersMessage::PickArt(card) => {
            let Some(m) = &mut state.managing else {
                return Task::none();
            };
            let Some(target) = m.art_for.take() else {
                return Task::none();
            };
            m.art_options.clear();
            match db::upsert_commander(
                conn,
                &target.oracle_id,
                &target.name,
                card.image_url.as_deref(),
                card.art_crop_url.as_deref(),
                &target.color_identity,
            ) {
                Ok(commander) => {
                    m.commanders =
                        db::player_commander_history(conn, m.player.id).unwrap_or_default();
                    if let Some(url) = commander.portrait_url() {
                        let url = url.to_string();
                        let key = url.clone();
                        return Task::perform(scryfall::fetch_image(url), move |res| {
                            Message::ArtLoaded(key.clone(), res)
                        });
                    }
                }
                Err(e) => state.error = Some(format!("Couldn't save art: {e}")),
            }
        }
        PlayersMessage::CancelArt => {
            if let Some(m) = &mut state.managing {
                m.art_for = None;
                m.art_options.clear();
            }
        }
        PlayersMessage::StartPairing(commander) => {
            if let Some(m) = &mut state.managing {
                m.pairing = Some(commander);
            }
        }
        PlayersMessage::PickPartner(partner) => {
            let Some(m) = &mut state.managing else {
                return Task::none();
            };
            let Some(primary) = m.pairing.take() else {
                return Task::none();
            };
            if primary.id != partner.id {
                let _ = db::set_player_partner(conn, m.player.id, primary.id, Some(partner.id));
            }
            m.commanders = db::player_commander_history(conn, m.player.id).unwrap_or_default();
        }
        PlayersMessage::Unpair(commander) => {
            if let Some(m) = &mut state.managing {
                let _ = db::set_player_partner(conn, m.player.id, commander.id, None);
                m.commanders = db::player_commander_history(conn, m.player.id).unwrap_or_default();
            }
        }
        PlayersMessage::CancelPairing => {
            if let Some(m) = &mut state.managing {
                m.pairing = None;
            }
        }
    }
    Task::none()
}

// ---------------------------------------------------------------------------
// Layout rhythm
//
// One padding scale for the whole screen, all of it derived from
// [`style::GAP`], and one set of column widths so every row in every list
// breaks at the same places.
// ---------------------------------------------------------------------------

/// Row padding: snug vertically, a full gap in from the edge. With a
/// [`style::TOUCH_H`] control inside, every row lands on the same height.
const ROW_PAD: [u16; 2] = [style::GAP_SM, style::GAP];

/// Vertical padding that puts a [`style::T_SUBHEAD`] field on exactly the
/// standard touch height, so a row being renamed is the same height as the
/// row it replaced. iced's default line height is 1.3x the font size.
const FIELD_PAD: [f32; 2] = [style::FIELD_PAD, style::GAP as f32];

/// Action column widths. Shared by the roster and the commander list so the
/// right-hand edge of every list is one straight line.
const W_BACK: f32 = 200.0;
const W_WIDE: f32 = 240.0;
const W_ACTION: f32 = 180.0;
const W_NARROW: f32 = 150.0;
/// The colour-identity column - short strings like "WUB", but wide enough
/// for all five.
const W_IDENTITY: f32 = 120.0;
/// One art thumbnail.
const ART_W: f32 = 300.0;
const ART_H: f32 = 220.0;

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

/// The title block every view on this screen starts with, with the way out
/// in the top right where it is on every other screen.
fn screen_header<'a>(title: String, size: u16, back: Message) -> Element<'a, Message> {
    container(
        row![
            text(title).size(size),
            iced::widget::horizontal_space(),
            style::touch_button("Back", style::T_LABEL)
                .width(Length::Fixed(W_BACK))
                .style(style::secondary)
                .on_press(back),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(style::GAP)
    .width(Length::Fill)
    .style(style::header)
    .into()
}

/// Something deliberate to look at when a list is empty - centred, so it
/// reads as a state of the screen rather than as rows that failed to draw.
fn empty_state<'a>(headline: &'a str, note: &'a str) -> Element<'a, Message> {
    container(
        column![
            text(headline).size(style::T_LEAD).color(style::TEXT),
            text(note).size(style::T_BODY).color(style::TEXT_MUTED),
        ]
        .spacing(style::GAP_XS)
        .align_x(iced::Alignment::Center),
    )
    .padding(style::GAP * 2)
    .center_x(Length::Fill)
    .style(style::panel)
    .into()
}

/// [`empty_state`] floated in the middle of whatever space is left, for the
/// cases where the empty list *is* the whole screen.
fn empty_fill<'a>(headline: &'a str, note: &'a str) -> Element<'a, Message> {
    container(empty_state(headline, note))
        .width(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

/// A quiet heading over a list, so the two halves of the commander screen
/// say what they are. Takes an owned `String` as happily as a literal, for
/// the lines that have to be built at render time.
fn section_label<'a>(label: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    text(label)
        .size(style::T_CAPTION)
        .color(style::TEXT_MUTED)
        .into()
}

/// A text field and the button that commits it, held in one panel so they
/// read as a single control rather than as a field that happens to have a
/// button next to it.
fn field_pod<'a>(
    field: impl Into<Element<'a, Message>>,
    action: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(
        row![field.into(), action.into()]
            .spacing(style::GAP_XS)
            .align_y(iced::Alignment::Center),
    )
    .padding(style::GAP_XS)
    .width(Length::Fill)
    .style(style::panel)
    .into()
}

/// One row of a list: a name that takes the slack, then the actions in a
/// fixed-width cluster on the right.
fn list_row<'a>(
    body: impl Into<Element<'a, Message>>,
    style_fn: fn(&iced::Theme) -> container::Style,
) -> Element<'a, Message> {
    container(body.into())
        .padding(ROW_PAD)
        .width(Length::Fill)
        .style(style_fn)
        .into()
}

/// The error banner. Errors here are sentences ("that player is in 3 games"),
/// so they get a full-width panel rather than a toast.
fn error_banner<'a>(message: &'a str) -> Element<'a, Message> {
    container(text(message).size(style::T_LABEL))
        .padding(style::GAP)
        .width(Length::Fill)
        .style(style::panel_danger)
        .into()
}

// ---------------------------------------------------------------------------
// Roster
// ---------------------------------------------------------------------------

pub fn view<'a>(
    state: &'a PlayersState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    if let Some(managed) = &state.managing {
        if let Some(commander) = &managed.pairing {
            return pairing_view(managed, commander, image_cache);
        }
        if managed.art_for.is_some() {
            return art_view(managed, image_cache);
        }
        return manage_view(state, managed);
    }

    let roster: Element<Message> = if state.players.is_empty() {
        empty_fill(
            "No players yet",
            "Add everyone who sits at this table - they'll keep their commanders and their record.",
        )
    } else {
        scrollable(
            column(
                state
                    .players
                    .iter()
                    .map(|p| player_row(state, p))
                    .collect::<Vec<Element<Message>>>(),
            )
            .spacing(style::GAP_SM),
        )
        .height(Length::Fill)
        .into()
    };

    let add_row = field_pod(
        text_input("New player name", &state.new_player_name)
            .size(style::T_SUBHEAD)
            .padding(FIELD_PAD)
            .style(style::input)
            .on_input(|s| Message::Players(PlayersMessage::NewNameChanged(s)))
            .on_submit(Message::Players(PlayersMessage::CreatePlayer)),
        style::touch_button("Add Player", style::T_ACTION)
            .width(Length::Fixed(W_WIDE))
            .style(style::primary)
            .on_press(Message::Players(PlayersMessage::CreatePlayer)),
    );

    let mut content = column![
        screen_header("Players".to_string(), style::T_TITLE, Message::GoHome),
        add_row,
        roster,
    ]
    .spacing(style::GAP);

    if let Some(e) = &state.error {
        content = content.push(error_banner(e));
    }

    container(content.padding(style::GAP))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn player_row<'a>(state: &'a PlayersState, p: &'a Player) -> Element<'a, Message> {
    // Renaming: the field stands in for the name, and the row is lit so it's
    // obvious which person is being edited.
    if let Some((id, name)) = &state.editing {
        if *id == p.id {
            return list_row(
                row![
                    text_input("Player name", name)
                        .size(style::T_SUBHEAD)
                        .padding(FIELD_PAD)
                        .style(style::input)
                        .on_input(|s| Message::Players(PlayersMessage::NameChanged(s)))
                        .on_submit(Message::Players(PlayersMessage::Save)),
                    row![
                        style::touch_button("Save", style::T_LABEL)
                            .width(Length::Fixed(W_ACTION))
                            .style(style::success)
                            .on_press(Message::Players(PlayersMessage::Save)),
                        style::touch_button("Cancel", style::T_LABEL)
                            .width(Length::Fixed(W_NARROW))
                            .style(style::ghost)
                            .on_press(Message::Players(PlayersMessage::Cancel)),
                    ]
                    .spacing(style::GAP_SM),
                ]
                .spacing(style::GAP)
                .align_y(iced::Alignment::Center),
                style::panel_active,
            );
        }
    }

    // Confirming a delete: the whole row turns into the question, and this
    // is the one moment the destructive action is allowed to shout.
    if state.confirming_delete == Some(p.id) {
        return list_row(
            row![
                column![
                    text(format!("Delete {}?", p.name))
                        .size(style::T_SUBHEAD)
                        .color(style::TEXT),
                    text("This can't be undone.")
                        .size(style::T_CAPTION)
                        .color(style::TEXT_MUTED),
                ]
                .spacing(style::GAP_XS)
                .width(Length::Fill),
                row![
                    style::touch_button("Yes, delete", style::T_LABEL)
                        .width(Length::Fixed(W_WIDE))
                        .style(style::danger)
                        .on_press(Message::Players(PlayersMessage::ConfirmDelete(p.id))),
                    style::touch_button("Keep", style::T_LABEL)
                        .width(Length::Fixed(W_NARROW))
                        .style(style::secondary)
                        .on_press(Message::Players(PlayersMessage::CancelDelete)),
                ]
                .spacing(style::GAP_SM),
            ]
            .spacing(style::GAP)
            .align_y(iced::Alignment::Center),
            style::panel_danger,
        );
    }

    // Resting state. Delete is a ghost: findable, but it doesn't sit there
    // in red next to the two harmless actions - the confirmation step is
    // where the warning belongs.
    list_row(
        row![
            text(p.name.clone())
                .size(style::T_SUBHEAD)
                .color(style::TEXT)
                .width(Length::Fill),
            row![
                style::touch_button("Commanders", style::T_LABEL)
                    .width(Length::Fixed(W_WIDE))
                    .style(style::secondary)
                    .on_press(Message::Players(PlayersMessage::ManageCommanders(p.clone()))),
                style::touch_button("Rename", style::T_LABEL)
                    .width(Length::Fixed(W_ACTION))
                    .style(style::secondary)
                    .on_press(Message::Players(PlayersMessage::StartEdit(
                        p.id,
                        p.name.clone()
                    ))),
                style::touch_button("Delete", style::T_LABEL)
                    .width(Length::Fixed(W_NARROW))
                    .style(style::danger_ghost)
                    .on_press(Message::Players(PlayersMessage::AskDelete(p.id))),
            ]
            .spacing(style::GAP_SM),
        ]
        .spacing(style::GAP)
        .align_y(iced::Alignment::Center),
        style::panel,
    )
}

// ---------------------------------------------------------------------------
// One player's commanders
// ---------------------------------------------------------------------------

fn manage_view<'a>(state: &'a PlayersState, managed: &'a ManagedPlayer) -> Element<'a, Message> {
    let owned: Element<Message> = if managed.commanders.is_empty() {
        empty_state(
            "No commanders saved yet",
            "Search below and tap a card to add it to this player's decks.",
        )
    } else {
        column(
            managed
                .commanders
                .iter()
                .map(|deck| {
                    let c = &deck.commander;
                    // A paired deck offers to unpair; a lone commander
                    // offers to pick a partner from this same list.
                    let pair_button = match &deck.partner {
                        Some(_) => style::touch_button("Unpair", style::T_LABEL)
                            .width(Length::Fixed(W_ACTION))
                            .style(style::ghost)
                            .on_press(Message::Players(PlayersMessage::Unpair(c.clone()))),
                        None => style::touch_button("Set Partner", style::T_LABEL)
                            .width(Length::Fixed(W_ACTION))
                            .style(style::secondary)
                            .on_press(Message::Players(PlayersMessage::StartPairing(c.clone()))),
                    };
                    list_row(
                        row![
                            text(deck.label())
                                .size(style::T_SUBHEAD)
                                .color(style::TEXT)
                                .width(Length::Fill),
                            text(c.color_identity.clone())
                                .size(style::T_LABEL)
                                .color(style::TEXT_MUTED)
                                .width(Length::Fixed(W_IDENTITY)),
                            row![
                                style::touch_button("Art", style::T_LABEL)
                                    .width(Length::Fixed(W_NARROW))
                                    .style(style::secondary)
                                    .on_press(Message::Players(PlayersMessage::ChangeArt(
                                        c.clone()
                                    ))),
                                pair_button,
                                style::touch_button("Remove", style::T_LABEL)
                                    .width(Length::Fixed(W_NARROW))
                                    .style(style::danger_ghost)
                                    .on_press(Message::Players(PlayersMessage::RemoveCommander(
                                        c.id
                                    ))),
                            ]
                            .spacing(style::GAP_SM),
                        ]
                        .spacing(style::GAP)
                        .align_y(iced::Alignment::Center),
                        style::panel,
                    )
                })
                .collect::<Vec<Element<Message>>>(),
        )
        .spacing(style::GAP_SM)
        .into()
    };

    let results: Element<Message> = if managed.results.is_empty() {
        empty_state(
            "Nothing found yet",
            "Type part of a commander's name and search - results land here.",
        )
    } else {
        column(
            managed
                .results
                .iter()
                .map(|c| {
                    button(
                        container(
                            row![
                                text(c.name.clone())
                                    .size(style::T_ACTION)
                                    .color(style::TEXT)
                                    .width(Length::Fill),
                                text(c.color_identity.clone())
                                    .size(style::T_LABEL)
                                    .color(style::TEXT_MUTED)
                                    .width(Length::Fixed(W_IDENTITY)),
                            ]
                            .spacing(style::GAP)
                            .align_y(iced::Alignment::Center),
                        )
                        .padding([0, style::GAP])
                        .center_y(Length::Fill),
                    )
                    .padding(0)
                    .height(Length::Fixed(style::TOUCH_H))
                    .width(Length::Fill)
                    .style(style::row_button)
                    .on_press(Message::Players(PlayersMessage::AddCommander(c.clone())))
                    .into()
                })
                .collect::<Vec<Element<Message>>>(),
        )
        .spacing(style::GAP_SM)
        .into()
    };

    let search_label: String = if state.cooldown.active() {
        state.cooldown.label()
    } else if managed.searching {
        "Searching...".into()
    } else {
        "Search".into()
    };

    let mut search_button = style::touch_button(search_label, style::T_ACTION)
        .width(Length::Fixed(W_WIDE))
        .style(style::primary);
    if !state.cooldown.active() {
        search_button = search_button.on_press(Message::Players(PlayersMessage::Search));
    }

    let mut content = column![
        screen_header(
            format!("{}'s Commanders", managed.player.name),
            style::T_HEADING,
            Message::Players(PlayersMessage::CloseManage),
        ),
        column![
            section_label("Saved decks"),
            scrollable(owned).height(Length::Fill),
        ]
        .spacing(style::GAP_SM)
        .height(Length::FillPortion(2)),
        column![
            section_label("Add a commander"),
            field_pod(
                text_input("Search Scryfall by name", &managed.query)
                    .size(style::T_SUBHEAD)
                    .padding(FIELD_PAD)
                    .style(style::input)
                    .on_input(|s| Message::Players(PlayersMessage::QueryChanged(s)))
                    .on_submit(Message::Players(PlayersMessage::Search)),
                search_button,
            ),
            scrollable(results).height(Length::Fill),
        ]
        .spacing(style::GAP_SM)
        .height(Length::FillPortion(3)),
    ]
    .spacing(style::GAP);

    if let Some(e) = &state.error {
        content = content.push(error_banner(e));
    }

    container(content.padding(style::GAP))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Pick a different printing's art for one of this player's commanders.
fn art_view<'a>(
    managed: &'a ManagedPlayer,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let target = managed.art_for.as_ref().unwrap();

    let tiles: Vec<Element<Message>> = managed
        .art_options
        .iter()
        .map(|card| {
            let thumb: Element<Message> =
                match card.small_url.as_deref().and_then(|u| image_cache.get(u)) {
                    Some(handle) => image(handle.clone())
                        .width(Length::Fixed(ART_W))
                        .height(Length::Fixed(ART_H))
                        .content_fit(ContentFit::Cover)
                        .into(),
                    None => container(
                        text("Loading")
                            .size(style::T_CAPTION)
                            .color(style::TEXT_MUTED),
                    )
                    .center_x(Length::Fixed(ART_W))
                    .center_y(Length::Fixed(ART_H))
                    .into(),
                };
            button(
                column![
                    thumb,
                    text(card.set_name.clone())
                        .size(style::T_CAPTION)
                        .color(style::TEXT_MUTED),
                ]
                .spacing(style::GAP_XS)
                .align_x(iced::Alignment::Center),
            )
            .padding(style::GAP_XS)
            .style(style::secondary)
            .on_press(Message::Players(PlayersMessage::PickArt(card.clone())))
            .into()
        })
        .collect();

    let body: Element<Message> = if tiles.is_empty() {
        let (headline, note) = if managed.loading_art {
            ("Looking up printings", "Fetching every version from Scryfall.")
        } else {
            (
                "No printings found",
                "Scryfall had nothing else for this card - the current art stays.",
            )
        };
        empty_fill(headline, note)
    } else {
        scrollable(row(tiles).spacing(style::GAP).wrap())
            .height(Length::Fill)
            .into()
    };

    let status = if managed.loading_art {
        "Loading every printing from Scryfall...".to_string()
    } else {
        format!("{} printings found", managed.art_options.len())
    };

    container(
        column![
            screen_header(
                format!("Art for {}", target.name),
                style::T_HEADING,
                Message::Players(PlayersMessage::CancelArt),
            ),
            section_label(status),
            body,
        ]
        .spacing(style::GAP)
        .padding(style::GAP),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Pick which of this player's other commanders pairs with `primary` as a
/// saved partner deck. Only their own saved commanders are offered - a
/// partner has to be something they already play.
fn pairing_view<'a>(
    managed: &'a ManagedPlayer,
    primary: &'a Commander,
    _image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let candidates: Vec<Element<Message>> = managed
        .commanders
        .iter()
        .filter(|d| d.commander.id != primary.id && d.partner.is_none())
        .map(|d| {
            style::touch_button(d.commander.name.clone(), style::T_ACTION)
                .width(Length::Fill)
                .style(style::row_button)
                .on_press(Message::Players(PlayersMessage::PickPartner(
                    d.commander.clone(),
                )))
                .into()
        })
        .collect();

    let body: Element<Message> = if candidates.is_empty() {
        empty_fill(
            "Nothing to pair with",
            "This player needs a second unpaired commander saved before the two can share a seat.",
        )
    } else {
        scrollable(column(candidates).spacing(style::GAP_SM))
            .height(Length::Fill)
            .into()
    };

    container(
        column![
            screen_header(
                format!("Pair with {}", primary.name),
                style::T_HEADING,
                Message::Players(PlayersMessage::CancelPairing),
            ),
            section_label("Picking either half of a saved pair brings the other with it"),
            body,
        ]
        .spacing(style::GAP)
        .padding(style::GAP),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
