use std::collections::HashMap;

use iced::widget::image;
use iced::{Element, Subscription, Task, Theme};
use rusqlite::Connection;

use crate::db;
use crate::model::{Commander, Player};
use crate::screens::{game, history, setup, stats};

pub enum Screen {
    Setup(setup::SetupState),
    Game(game::GameState),
    Stats(stats::StatsState),
    History(history::HistoryState),
}

pub struct App {
    conn: Connection,
    players: Vec<Player>,
    commanders: Vec<Commander>,
    image_cache: HashMap<String, image::Handle>,
    screen: Screen,
}

#[derive(Debug, Clone)]
pub enum Message {
    Setup(setup::SetupMessage),
    Game(game::GameMessage),
    History(history::HistoryMessage),
    ArtLoaded(String, Result<Vec<u8>, String>),
    GoToStats,
    GoToHistory,
    BackToSetup,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let conn = db::open().expect("failed to open local database");
        let players = db::list_players(&conn).unwrap_or_default();
        let commanders = db::list_commanders(&conn).unwrap_or_default();
        (
            Self {
                conn,
                players,
                commanders,
                image_cache: HashMap::new(),
                screen: Screen::Setup(setup::SetupState::new()),
            },
            Task::none(),
        )
    }

    pub fn title(&self) -> String {
        "Commander Pod".to_string()
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }

    pub fn subscription(&self) -> Subscription<Message> {
        match &self.screen {
            Screen::Game(state) if !state.paused => {
                iced::time::every(std::time::Duration::from_secs(1))
                    .map(|_| Message::Game(game::GameMessage::Tick))
            }
            _ => Subscription::none(),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ArtLoaded(id, result) => {
                if let Ok(bytes) = result {
                    self.image_cache
                        .insert(id, image::Handle::from_bytes(bytes));
                }
                Task::none()
            }
            Message::GoToStats => {
                self.screen = Screen::Stats(stats::StatsState::load(&self.conn));
                Task::none()
            }
            Message::GoToHistory => {
                self.screen = Screen::History(history::HistoryState::load(&self.conn));
                Task::none()
            }
            Message::BackToSetup => {
                self.screen = Screen::Setup(setup::SetupState::new());
                Task::none()
            }
            Message::Setup(msg) => {
                if let Screen::Setup(state) = &mut self.screen {
                    let (task, action) = setup::update(
                        state,
                        &self.conn,
                        &mut self.commanders,
                        &mut self.players,
                        msg,
                    );
                    if let Some(setup::Action::StartGame(seats, layout)) = action {
                        self.screen = Screen::Game(game::GameState::new(seats, layout));
                    }
                    task
                } else {
                    Task::none()
                }
            }
            Message::Game(msg) => {
                if let Screen::Game(state) = &mut self.screen {
                    let (task, action) = game::update(state, &mut self.conn, msg);
                    match action {
                        Some(game::Action::Finished) | Some(game::Action::Abandoned) => {
                            self.screen = Screen::Setup(setup::SetupState::new());
                        }
                        None => {}
                    }
                    task
                } else {
                    Task::none()
                }
            }
            Message::History(msg) => {
                if let Screen::History(state) = &mut self.screen {
                    history::update(state, &self.conn, msg);
                }
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        match &self.screen {
            Screen::Setup(state) => setup::view(state, &self.players, &self.commanders),
            Screen::Game(state) => game::view(state, &self.image_cache),
            Screen::Stats(state) => stats::view(state),
            Screen::History(state) => history::view(state),
        }
    }
}
