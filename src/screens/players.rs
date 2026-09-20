use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::model::Player;
use crate::style;

pub struct PlayersState {
    pub players: Vec<Player>,
    pub editing: Option<(i64, String)>,
    pub error: Option<String>,
}

impl PlayersState {
    pub fn load(conn: &Connection) -> Self {
        Self {
            players: db::list_players(conn).unwrap_or_default(),
            editing: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PlayersMessage {
    StartEdit(i64, String),
    NameChanged(String),
    Save,
    Cancel,
}

pub fn update(state: &mut PlayersState, conn: &Connection, message: PlayersMessage) {
    state.error = None;
    match message {
        PlayersMessage::StartEdit(id, name) => {
            state.editing = Some((id, name));
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
                        Ok(()) => state.players = db::list_players(conn).unwrap_or_default(),
                        Err(e) => state.error = Some(format!("Couldn't rename: {e}")),
                    }
                }
            }
        }
        PlayersMessage::Cancel => {
            state.editing = None;
        }
    }
}

pub fn view<'a>(state: &'a PlayersState) -> Element<'a, Message> {
    let rows = column(
        state
            .players
            .iter()
            .map(|p| {
                if let Some((id, name)) = &state.editing {
                    if *id == p.id {
                        return row![
                            text_input("Player name", name)
                                .size(26)
                                .padding(22)
                                .on_input(|s| Message::Players(PlayersMessage::NameChanged(s)))
                                .on_submit(Message::Players(PlayersMessage::Save)),
                            style::touch_button("Save", 22)
                                .width(Length::Fixed(180.0))
                                .style(button::success)
                                .on_press(Message::Players(PlayersMessage::Save)),
                            style::touch_button("Cancel", 22)
                                .width(Length::Fixed(180.0))
                                .style(button::secondary)
                                .on_press(Message::Players(PlayersMessage::Cancel)),
                        ]
                        .spacing(14)
                        .align_y(iced::Alignment::Center)
                        .into();
                    }
                }
                container(
                    row![
                        text(p.name.clone()).size(26).width(Length::Fill),
                        style::touch_button("Rename", 20)
                            .width(Length::Fixed(200.0))
                            .style(button::secondary)
                            .on_press(Message::Players(PlayersMessage::StartEdit(
                                p.id,
                                p.name.clone()
                            ))),
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

    let mut content = column![header, scrollable(rows).height(Length::Fill)].spacing(style::GAP);

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
