use iced::widget::{button, column, container, responsive, row, scrollable, text, Space};
use iced::{Alignment, Element, Length};
use rusqlite::Connection;

use crate::app::Message;
use crate::icon::{self, Glyph};
use crate::model::{GameSummary, Player};
use crate::{db, layout, style, table_preview};

#[derive(Debug, Clone)]
pub enum HomeMessage {
    StartGame,
    ResumeGame,
    PlayAgain,
    Storage,
    Retry,
    ViewStats,
    ViewHistory,
    ManagePlayers,
    ViewGame(i64),
}

/// Loaded when returning home, never queried on each rendered frame.
#[derive(Default)]
pub struct HomeState {
    pub error: Option<String>,
    pub resumable: bool,
    pub saved_layout: Option<layout::TableLayout>,
    pub play_again: bool,
    pub games_played: usize,
    pub recent: Vec<GameSummary>,
}
impl HomeState {
    pub fn load(conn: &Connection) -> Self {
        let (mut recent, mut error) = match db::list_games(conn) {
            Ok(games) => (games, None),
            Err(e) => (Vec::new(), Some(format!("Couldn’t load history: {e}"))),
        };
        let resumable = match db::active_game(conn) {
            Ok(s) => s.is_some(),
            Err(e) => {
                error = Some(format!("Couldn’t check for a saved game: {e}"));
                true
            }
        };
        let games_played = recent.len();
        recent.truncate(3);
        let saved_layout = if resumable {
            match crate::session::load(conn) {
                Ok(Some(game)) => Some(game.table_layout),
                Ok(None) => None,
                Err(e) => {
                    error = Some(e);
                    None
                }
            }
        } else {
            None
        };
        Self {
            saved_layout,
            error,
            resumable,
            play_again: false,
            games_played,
            recent,
        }
    }
}

fn destination(
    glyph: Glyph,
    title: &'static str,
    description: String,
    message: HomeMessage,
    height: f32,
) -> Element<'static, Message> {
    button(
        container(
            column![
                row![
                    icon::view(glyph, 32., style::ACCENT_BRIGHT),
                    iced::widget::horizontal_space(),
                    icon::view(Glyph::Next, 24., style::TEXT_MUTED)
                ],
                Space::with_height(Length::Fill),
                text(title).size(style::T_HEADING),
                text(description)
                    .size(style::T_LABEL)
                    .color(style::TEXT_MUTED),
            ]
            .spacing(style::GAP),
        )
        .height(Length::Fill),
    )
    .padding(32)
    .width(Length::Fill)
    .height(height)
    .style(style::row_button)
    .on_press(Message::Home(message))
    .into()
}

fn start_card(height: f32, state: &HomeState) -> Element<'static, Message> {
    let resumable = state.resumable;
    let play_again = state.play_again && !resumable;
    let table = state
        .saved_layout
        .clone()
        .unwrap_or_else(|| layout::options_for(4).remove(0));
    container(
        column![
            text("YOUR NEXT GAME")
                .size(style::T_CAPTION)
                .color(style::ACCENT_BRIGHT),
            text(if resumable {
                "Your game is saved."
            } else if play_again {
                "Another round?"
            } else {
                "Gather your pod."
            })
            .size(style::T_DISPLAY),
            text(if resumable {
                "Pick up where you left off. Timers resume paused."
            } else if play_again {
                "Same players and decks. Choose who starts."
            } else {
                "Pick your players. Bring your commanders."
            })
            .size(style::T_LABEL)
            .color(style::TEXT_MUTED),
            container(table_preview::view(&table))
                .height(Length::Fill)
                .padding([8, 32]),
            row![
                column![
                    text(if resumable {
                        "Saved table"
                    } else {
                        "2–8 players"
                    })
                    .size(style::T_SUBHEAD),
                    text(if resumable {
                        "Life, damage & turns preserved"
                    } else {
                        "40 starting life"
                    })
                    .size(style::T_BODY)
                    .color(style::TEXT_MUTED)
                ]
                .spacing(style::GAP_XS),
                iced::widget::horizontal_space(),
                style::icon_button(
                    Glyph::Play,
                    if resumable {
                        "Resume Game"
                    } else if play_again {
                        "Play again"
                    } else {
                        "Start Game"
                    },
                    style::T_ACTION
                )
                .width(240)
                .height(style::TOUCH_H_LG)
                .style(style::primary)
                .on_press(Message::Home(if resumable {
                    HomeMessage::ResumeGame
                } else if play_again {
                    HomeMessage::PlayAgain
                } else {
                    HomeMessage::StartGame
                })),
            ]
            .align_y(Alignment::Center)
            .spacing(style::GAP),
        ]
        .spacing(style::GAP_SM),
    )
    .padding(32)
    .width(Length::FillPortion(3))
    .height(height)
    .style(style::panel)
    .into()
}

fn recent_games(state: &HomeState, height: f32) -> Element<'_, Message> {
    let mut games = column![row![
        icon::view(Glyph::History, 28., style::ACCENT_BRIGHT),
        text("Last at the table").size(style::T_SUBHEAD)
    ]
    .spacing(style::GAP_SM)
    .align_y(Alignment::Center),]
    .spacing(style::GAP);
    if state.recent.is_empty() {
        games = games.push(
            container(
                column![
                    text("The first game is yours.").size(style::T_LEAD),
                    text("Your latest results will appear here after you finish a game.")
                        .size(style::T_BODY)
                        .color(style::TEXT_MUTED),
                ]
                .spacing(style::GAP_SM),
            )
            .center_y(200),
        );
    } else {
        for game in &state.recent {
            games = games.push(
                button(
                    column![
                        row![
                            text(game.started_at.format("%b %-d").to_string())
                                .size(style::T_CAPTION)
                                .color(style::TEXT_MUTED),
                            iced::widget::horizontal_space(),
                            text(format!(
                                "{} players · {} {}",
                                game.pod_size,
                                game.ending_turn,
                                if game.ending_turn == 1 {
                                    "turn"
                                } else {
                                    "turns"
                                }
                            ))
                            .size(style::T_CAPTION)
                            .color(style::TEXT_MUTED),
                        ],
                        text(
                            game.winner_name
                                .as_ref()
                                .map(|name| format!("{name} won"))
                                .unwrap_or_else(|| "Game recorded".into())
                        )
                        .size(style::T_SUBHEAD),
                        text(
                            game.winner_commander
                                .clone()
                                .unwrap_or_else(|| "Open game details".into())
                        )
                        .size(style::T_BODY)
                        .color(style::TEXT_MUTED),
                    ]
                    .spacing(style::GAP_XS),
                )
                .padding(style::GAP)
                .width(Length::Fill)
                .style(style::row_button)
                .on_press(Message::Home(HomeMessage::ViewGame(game.id))),
            );
        }
    }
    let body = scrollable(games).height(Length::Fill);
    container(
        column![
            body,
            style::icon_button(Glyph::Next, "All games", style::T_LABEL)
                .width(Length::Fill)
                .style(style::ghost)
                .on_press(Message::Home(HomeMessage::ViewHistory))
        ]
        .spacing(style::GAP_SM),
    )
    .padding(24)
    .width(Length::FillPortion(2))
    .height(height)
    .style(style::panel)
    .into()
}

pub fn view<'a>(state: &'a HomeState, players: &'a [Player]) -> Element<'a, Message> {
    responsive(move |size| {
        let wide = size.width >= 1100.;
        let main_height = if wide {
            (size.height - 440.).clamp(400., 600.)
        } else {
            460.
        };
        let nav_height = if wide {
            (size.height * 0.22).clamp(190., 280.)
        } else {
            168.
        };
        let header = row![
            container(icon::view(Glyph::Decks, 36., style::ACCENT_BRIGHT))
                .padding(16)
                .style(style::panel),
            column![
                text("Commander Pod").size(style::T_TITLE),
                text("A place for every player.")
                    .size(style::T_BODY)
                    .color(style::TEXT_MUTED)
            ]
            .spacing(style::GAP_XS),
            iced::widget::horizontal_space(),
            style::touch_button("Backup & restore", style::T_LABEL)
                .width(200)
                .style(style::ghost)
                .on_press(Message::Home(HomeMessage::Storage)),
        ]
        .spacing(style::GAP)
        .align_y(Alignment::Center);
        let main: Element<Message> = if wide {
            row![
                start_card(main_height, state),
                recent_games(state, main_height)
            ]
            .spacing(24)
            .into()
        } else {
            column![start_card(main_height, state), recent_games(state, 460.)]
                .spacing(24)
                .into()
        };
        let destinations = vec![
            destination(
                Glyph::Players,
                "Players & Decks",
                format!(
                    "{} saved {} · Manage your commanders",
                    players.len(),
                    if players.len() == 1 {
                        "player"
                    } else {
                        "players"
                    }
                ),
                HomeMessage::ManagePlayers,
                nav_height,
            ),
            destination(
                Glyph::History,
                "Game History",
                format!(
                    "{} {} recorded · Every result in one place",
                    state.games_played,
                    if state.games_played == 1 {
                        "game"
                    } else {
                        "games"
                    }
                ),
                HomeMessage::ViewHistory,
                nav_height,
            ),
            destination(
                Glyph::Stats,
                "Table Stats",
                "Win rates, matchups and rivalries".into(),
                HomeMessage::ViewStats,
                nav_height,
            ),
        ];
        let nav: Element<Message> = if wide {
            row(destinations).spacing(24).into()
        } else {
            column(destinations).spacing(16).into()
        };
        let mut extras = column![].spacing(style::GAP);
        if state.play_again && !state.resumable {
            extras = extras.push(
                style::touch_button("Set up a different pod", style::T_LABEL)
                    .style(style::ghost)
                    .on_press(Message::Home(HomeMessage::StartGame)),
            );
        }
        if let Some(error) = &state.error {
            extras = extras.push(text(error).color(style::DANGER)).push(
                style::touch_button("Retry loading", style::T_LABEL)
                    .on_press(Message::Home(HomeMessage::Retry)),
            );
        }
        let mut page = column![header, main];
        if state.error.is_some() || (state.play_again && !state.resumable) {
            page = page.push(extras);
        }
        page = page.push(nav);
        scrollable(container(page.spacing(24).padding(32).max_width(1800)).center_x(Length::Fill))
            .height(Length::Fill)
            .into()
    })
    .into()
}
