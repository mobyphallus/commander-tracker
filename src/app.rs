use std::collections::HashMap;

use iced::widget::image;
use iced::{Element, Subscription, Task, Theme};
use rusqlite::Connection;

use crate::db;
use crate::model::Player;
use crate::screens::{game, history, home, players, setup, stats};
use crate::style;

pub enum Screen {
    Home,
    Setup(setup::SetupState),
    Game(game::GameState),
    Stats(stats::StatsState),
    History(history::HistoryState),
    Players(players::PlayersState),
}

pub struct App {
    conn: Connection,
    players: Vec<Player>,
    image_cache: HashMap<String, image::Handle>,
    screen: Screen,
}

#[derive(Debug, Clone)]
pub enum Message {
    Home(home::HomeMessage),
    Setup(setup::SetupMessage),
    Game(game::GameMessage),
    History(history::HistoryMessage),
    Players(players::PlayersMessage),
    ArtLoaded(String, Result<Vec<u8>, String>),
    GoHome,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let conn = db::open().expect("failed to open local database");
        let players = db::list_players(&conn).unwrap_or_default();
        (
            Self {
                conn,
                players,
                image_cache: HashMap::new(),
                screen: Screen::Home,
            },
            Task::none(),
        )
    }

    pub fn title(&self) -> String {
        "Commander Pod".to_string()
    }

    pub fn theme(&self) -> Theme {
        style::app_theme()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        match &self.screen {
            Screen::Game(state) => {
                let mut subs = Vec::new();
                if !state.paused {
                    subs.push(
                        iced::time::every(std::time::Duration::from_secs(1))
                            .map(|_| Message::Game(game::GameMessage::Tick)),
                    );
                }
                if state.press_hold.is_some() {
                    subs.push(
                        iced::time::every(std::time::Duration::from_millis(100))
                            .map(|_| Message::Game(game::GameMessage::HoldTick)),
                    );
                }
                Subscription::batch(subs)
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
            Message::GoHome => {
                self.screen = Screen::Home;
                Task::none()
            }
            Message::Home(msg) => {
                match msg {
                    home::HomeMessage::StartGame => {
                        self.screen = Screen::Setup(setup::SetupState::new());
                    }
                    home::HomeMessage::ViewStats => {
                        self.screen = Screen::Stats(stats::StatsState::load(&self.conn));
                    }
                    home::HomeMessage::ViewHistory => {
                        self.screen = Screen::History(history::HistoryState::load(&self.conn));
                    }
                    home::HomeMessage::ManagePlayers => {
                        self.screen = Screen::Players(players::PlayersState::load(&self.conn));
                    }
                }
                Task::none()
            }
            Message::Setup(msg) => {
                let task = if let Screen::Setup(state) = &mut self.screen {
                    let (task, action) = setup::update(state, &self.conn, msg);
                    if let Some(setup::Action::StartGame(seats)) = action {
                        self.screen = Screen::Game(game::GameState::new(seats));
                    }
                    task
                } else {
                    Task::none()
                };
                // Cheap local query; keeps the player list current after
                // adding a new player, without threading a shared cache
                // through every setup message.
                self.players = db::list_players(&self.conn).unwrap_or_default();
                task
            }
            Message::Game(msg) => {
                if let Screen::Game(state) = &mut self.screen {
                    let (task, action) = game::update(state, &mut self.conn, msg);
                    match action {
                        Some(game::Action::Finished) | Some(game::Action::Abandoned) => {
                            self.screen = Screen::Home;
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
            Message::Players(msg) => {
                if let Screen::Players(state) = &mut self.screen {
                    players::update(state, &self.conn, msg);
                }
                self.players = db::list_players(&self.conn).unwrap_or_default();
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        match &self.screen {
            Screen::Home => home::view(),
            Screen::Setup(state) => setup::view(state, &self.players, &self.image_cache),
            Screen::Game(state) => game::view(state, &self.image_cache),
            Screen::Stats(state) => stats::view(state),
            Screen::History(state) => history::view(state),
            Screen::Players(state) => players::view(state),
        }
    }
}
