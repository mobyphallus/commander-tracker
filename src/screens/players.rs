use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length, Task};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::model::{Commander, Player};
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
    }
    Task::none()
}

pub fn view(state: &PlayersState) -> Element<'_, Message> {
    if let Some(managed) = &state.managing {
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
                            text(c.color_identity.clone()).size(20).width(Length::Fixed(110.0)),
                            style::touch_button("Remove", 18)
                                .width(Length::Fixed(170.0))
                                .style(button::danger)
                                .on_press(Message::Players(PlayersMessage::RemoveCommander(c.id))),
                        ]
                        .spacing(14)
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
