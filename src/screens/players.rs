use std::collections::HashMap;

use iced::widget::{button, column, container, image, row, scrollable, text, text_input};
use iced::{ContentFit, Element, Length, Task};
use rusqlite::Connection;

use crate::app::Message;
use crate::art;
use crate::db;
use crate::model::{ArtAnchor, Commander, Player, MAX_ART_ZOOM, MIN_ART_ZOOM};
use crate::scryfall::{self, ScryfallCard};
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
    pub error: Option<String>,
}

pub struct ManagedPlayer {
    pub player: Player,
    pub commanders: Vec<Commander>,
    pub query: String,
    pub results: Vec<ScryfallCard>,
    pub searching: bool,
    /// Set while picking a different printing's art for one of this
    /// player's commanders.
    pub art_for: Option<Commander>,
    pub art_options: Vec<ScryfallCard>,
    pub loading_art: bool,
    /// Set while zooming/anchoring one commander's art.
    pub framing: Option<Commander>,
}

impl PlayersState {
    pub fn load(conn: &Connection) -> Self {
        Self {
            players: db::list_players(conn).unwrap_or_default(),
            new_player_name: String::new(),
            editing: None,
            confirming_delete: None,
            managing: None,
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
    SearchResults(Result<Vec<ScryfallCard>, String>),
    AddCommander(ScryfallCard),
    RemoveCommander(i64),
    ChangeArt(Commander),
    ArtOptionsLoaded(Result<Vec<ScryfallCard>, String>),
    PickArt(ScryfallCard),
    CancelArt,
    StartFraming(Commander),
    SetArtZoom(f32),
    SetArtAnchor(ArtAnchor),
    DoneFraming,
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
                framing: None,
            });
        }
        PlayersMessage::CloseManage => state.managing = None,
        PlayersMessage::QueryChanged(s) => {
            if let Some(m) = &mut state.managing {
                m.query = s;
            }
        }
        PlayersMessage::Search => {
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
                match res {
                    Ok(list) => m.results = list,
                    Err(e) => state.error = Some(format!("Scryfall search failed: {e}")),
                }
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
                        m.art_options = list;
                        return Task::batch(thumbs);
                    }
                    Err(e) => state.error = Some(format!("Couldn't load printings: {e}")),
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
        PlayersMessage::StartFraming(commander) => {
            if let Some(m) = &mut state.managing {
                m.framing = Some(commander);
            }
        }
        PlayersMessage::SetArtZoom(zoom) => {
            if let Some(m) = &mut state.managing {
                if let Some(c) = &mut m.framing {
                    c.art_zoom = zoom.clamp(MIN_ART_ZOOM, MAX_ART_ZOOM);
                    let _ =
                        db::set_commander_framing(conn, c.id, c.art_zoom, c.art_anchor);
                    m.commanders =
                        db::player_commander_history(conn, m.player.id).unwrap_or_default();
                }
            }
        }
        PlayersMessage::SetArtAnchor(anchor) => {
            if let Some(m) = &mut state.managing {
                if let Some(c) = &mut m.framing {
                    c.art_anchor = anchor;
                    let _ = db::set_commander_framing(conn, c.id, c.art_zoom, anchor);
                    m.commanders =
                        db::player_commander_history(conn, m.player.id).unwrap_or_default();
                }
            }
        }
        PlayersMessage::DoneFraming => {
            if let Some(m) = &mut state.managing {
                m.framing = None;
            }
        }
    }
    Task::none()
}

pub fn view<'a>(
    state: &'a PlayersState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    if let Some(managed) = &state.managing {
        if let Some(commander) = &managed.framing {
            return framing_view(commander, image_cache);
        }
        if managed.art_for.is_some() {
            return art_view(managed, image_cache);
        }
        return manage_view(state, managed);
    }

    let rows = column(
        state
            .players
            .iter()
            .map(|p| player_row(state, p))
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(12);

    let header = container(
        row![
            text("Players").size(38),
            iced::widget::horizontal_space(),
            style::touch_button("Back", 20)
                .width(Length::Fixed(200.0))
                .style(button::secondary)
                .on_press(Message::GoHome),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(16)
    .width(Length::Fill)
    .style(style::header);

    let add_row = row![
        text_input("New player name", &state.new_player_name)
            .size(26)
            .padding(22)
            .on_input(|s| Message::Players(PlayersMessage::NewNameChanged(s)))
            .on_submit(Message::Players(PlayersMessage::CreatePlayer)),
        style::touch_button("Add Player", 22)
            .width(Length::Fixed(240.0))
            .style(button::primary)
            .on_press(Message::Players(PlayersMessage::CreatePlayer)),
    ]
    .spacing(14)
    .align_y(iced::Alignment::Center);

    let mut content = column![header, add_row, scrollable(rows).height(Length::Fill)]
        .spacing(style::GAP);

    if let Some(e) = &state.error {
        content = content.push(
            container(text(e.clone()).size(20))
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

fn player_row<'a>(state: &'a PlayersState, p: &'a Player) -> Element<'a, Message> {
    if let Some((id, name)) = &state.editing {
        if *id == p.id {
            return row![
                text_input("Player name", name)
                    .size(26)
                    .padding(22)
                    .on_input(|s| Message::Players(PlayersMessage::NameChanged(s)))
                    .on_submit(Message::Players(PlayersMessage::Save)),
                style::touch_button("Save", 22)
                    .width(Length::Fixed(160.0))
                    .style(button::success)
                    .on_press(Message::Players(PlayersMessage::Save)),
                style::touch_button("Cancel", 22)
                    .width(Length::Fixed(160.0))
                    .style(button::secondary)
                    .on_press(Message::Players(PlayersMessage::Cancel)),
            ]
            .spacing(14)
            .align_y(iced::Alignment::Center)
            .into();
        }
    }

    if state.confirming_delete == Some(p.id) {
        return container(
            row![
                text(format!("Delete {}?", p.name)).size(26).width(Length::Fill),
                style::touch_button("Yes, delete", 20)
                    .width(Length::Fixed(220.0))
                    .style(button::danger)
                    .on_press(Message::Players(PlayersMessage::ConfirmDelete(p.id))),
                style::touch_button("Keep", 20)
                    .width(Length::Fixed(160.0))
                    .style(button::secondary)
                    .on_press(Message::Players(PlayersMessage::CancelDelete)),
            ]
            .spacing(14)
            .align_y(iced::Alignment::Center),
        )
        .padding([10, 20])
        .width(Length::Fill)
        .style(style::panel_danger)
        .into();
    }

    container(
        row![
            text(p.name.clone()).size(26).width(Length::Fill),
            style::touch_button("Commanders", 20)
                .width(Length::Fixed(230.0))
                .style(button::secondary)
                .on_press(Message::Players(PlayersMessage::ManageCommanders(p.clone()))),
            style::touch_button("Rename", 20)
                .width(Length::Fixed(180.0))
                .style(button::secondary)
                .on_press(Message::Players(PlayersMessage::StartEdit(
                    p.id,
                    p.name.clone()
                ))),
            style::touch_button("Delete", 20)
                .width(Length::Fixed(160.0))
                .style(button::danger)
                .on_press(Message::Players(PlayersMessage::AskDelete(p.id))),
        ]
        .spacing(14)
        .align_y(iced::Alignment::Center),
    )
    .padding([10, 20])
    .width(Length::Fill)
    .style(style::panel)
    .into()
}

fn manage_view<'a>(state: &'a PlayersState, managed: &'a ManagedPlayer) -> Element<'a, Message> {
    let header = container(
        row![
            text(format!("{}'s Commanders", managed.player.name)).size(34),
            iced::widget::horizontal_space(),
            style::touch_button("Back", 20)
                .width(Length::Fixed(200.0))
                .style(button::secondary)
                .on_press(Message::Players(PlayersMessage::CloseManage)),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(16)
    .width(Length::Fill)
    .style(style::header);

    let owned: Element<Message> = if managed.commanders.is_empty() {
        text("No commanders saved yet - search below to add one.")
            .size(20)
            .into()
    } else {
        column(
            managed
                .commanders
                .iter()
                .map(|c| {
                    container(
                        row![
                            text(c.name.clone()).size(24).width(Length::Fill),
                            text(c.color_identity.clone()).size(20).width(Length::Fixed(90.0)),
                            style::touch_button("Art", 18)
                                .width(Length::Fixed(130.0))
                                .style(button::secondary)
                                .on_press(Message::Players(PlayersMessage::ChangeArt(c.clone()))),
                            style::touch_button("Frame", 18)
                                .width(Length::Fixed(150.0))
                                .style(button::secondary)
                                .on_press(Message::Players(PlayersMessage::StartFraming(
                                    c.clone()
                                ))),
                            style::touch_button("Remove", 18)
                                .width(Length::Fixed(160.0))
                                .style(button::danger)
                                .on_press(Message::Players(PlayersMessage::RemoveCommander(c.id))),
                        ]
                        .spacing(12)
                        .align_y(iced::Alignment::Center),
                    )
                    .padding([10, 20])
                    .width(Length::Fill)
                    .style(style::panel)
                    .into()
                })
                .collect::<Vec<Element<Message>>>(),
        )
        .spacing(12)
        .into()
    };

    let results = column(
        managed
            .results
            .iter()
            .map(|c| {
                button(
                    container(text(format!("{}   [{}]", c.name, c.color_identity)).size(22))
                        .padding([0, 20])
                        .center_y(Length::Fill),
                )
                .padding(0)
                .height(Length::Fixed(style::TOUCH_H))
                .width(Length::Fill)
                .style(button::secondary)
                .on_press(Message::Players(PlayersMessage::AddCommander(c.clone())))
                .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(10);

    let search_label = if managed.searching {
        "Searching..."
    } else {
        "Search"
    };

    let mut content = column![
        header,
        scrollable(owned).height(Length::FillPortion(2)),
        row![
            text_input("Add a commander by name", &managed.query)
                .size(26)
                .padding(22)
                .on_input(|s| Message::Players(PlayersMessage::QueryChanged(s)))
                .on_submit(Message::Players(PlayersMessage::Search)),
            style::touch_button(search_label, 22)
                .width(Length::Fixed(220.0))
                .style(button::primary)
                .on_press(Message::Players(PlayersMessage::Search)),
        ]
        .spacing(14)
        .align_y(iced::Alignment::Center),
        scrollable(results).height(Length::FillPortion(3)),
    ]
    .spacing(style::GAP);

    if let Some(e) = &state.error {
        content = content.push(
            container(text(e.clone()).size(20))
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
                        .width(Length::Fixed(300.0))
                        .height(Length::Fixed(220.0))
                        .content_fit(ContentFit::Cover)
                        .into(),
                    None => container(text("...").size(20))
                        .width(Length::Fixed(300.0))
                        .height(Length::Fixed(220.0))
                        .center_x(Length::Fixed(300.0))
                        .center_y(Length::Fixed(220.0))
                        .into(),
                };
            button(
                column![thumb, text(card.set_name.clone()).size(16)]
                    .spacing(8)
                    .align_x(iced::Alignment::Center),
            )
            .padding(10)
            .style(button::secondary)
            .on_press(Message::Players(PlayersMessage::PickArt(card.clone())))
            .into()
        })
        .collect();

    let status = if managed.loading_art {
        text("Loading every printing from Scryfall...").size(18)
    } else {
        text(format!("{} printings found", managed.art_options.len())).size(18)
    };

    container(
        column![
            text(format!("Choose art for {}", target.name)).size(40),
            status,
            scrollable(row(tiles).spacing(16).wrap()).height(Length::Fill),
            style::touch_button("Back", 20)
                .width(Length::Fixed(280.0))
                .style(button::secondary)
                .on_press(Message::Players(PlayersMessage::CancelArt)),
        ]
        .spacing(18)
        .align_x(iced::Alignment::Center)
        .padding(style::GAP),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Zoom/anchor a commander's art so the right part shows in a seat tile.
fn framing_view<'a>(
    commander: &'a Commander,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let zoom = commander.art_zoom;

    let preview = container(art::framed(commander, image_cache, 20))
        .width(Length::Fixed(640.0))
        .height(Length::Fixed(360.0))
        .style(style::panel);

    let anchor_grid = column(
        ArtAnchor::GRID
            .iter()
            .map(|row_anchors| {
                row(row_anchors
                    .iter()
                    .map(|a| {
                        let selected = commander.art_anchor == *a;
                        button(container(text("")).width(Length::Fill).height(Length::Fill))
                            .width(Length::Fixed(72.0))
                            .height(Length::Fixed(72.0))
                            .style(if selected {
                                button::primary
                            } else {
                                button::secondary
                            })
                            .on_press(Message::Players(PlayersMessage::SetArtAnchor(*a)))
                            .into()
                    })
                    .collect::<Vec<Element<Message>>>())
                .spacing(10)
                .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(10);

    container(
        column![
            text(format!("Frame {}", commander.name)).size(40),
            text("Zoom in, then pick which part of the art stays in the tile.").size(18),
            row![
                preview,
                column![text("Focus").size(20), anchor_grid]
                    .spacing(12)
                    .align_x(iced::Alignment::Center),
            ]
            .spacing(28)
            .align_y(iced::Alignment::Center),
            row![
                style::touch_button("\u{2212} Zoom out", 20)
                    .width(Length::Fixed(230.0))
                    .style(button::secondary)
                    .on_press(Message::Players(PlayersMessage::SetArtZoom(zoom - 0.15))),
                container(text(format!("{:.0}%", zoom * 100.0)).size(28))
                    .width(Length::Fixed(140.0))
                    .center_x(Length::Fixed(140.0)),
                style::touch_button("Zoom in +", 20)
                    .width(Length::Fixed(230.0))
                    .style(button::secondary)
                    .on_press(Message::Players(PlayersMessage::SetArtZoom(zoom + 0.15))),
            ]
            .spacing(14)
            .align_y(iced::Alignment::Center),
            style::cta_button("Done", 24)
                .width(Length::Fixed(300.0))
                .style(button::success)
                .on_press(Message::Players(PlayersMessage::DoneFraming)),
        ]
        .spacing(22)
        .align_x(iced::Alignment::Center)
        .padding(style::GAP),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
