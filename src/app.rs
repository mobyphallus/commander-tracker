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
    Storage(crate::storage::StorageState),
    Setup(setup::SetupState),
    Game(game::GameState),
    Stats(stats::StatsState),
    History(history::HistoryState),
    Players(players::PlayersState),
}

pub struct App {
    feedback: crate::feedback::Service,
    conn: Connection,
    fatal_error: Option<String>,
    rematch: Option<setup::SetupState>,
    players: Vec<Player>,
    image_cache: HashMap<String, image::Handle>,
    screen: Screen,
    home: home::HomeState,
}

#[derive(Debug, Clone)]
pub enum Message {
    Home(home::HomeMessage),
    Setup(setup::SetupMessage),
    Game(game::GameMessage),
    History(history::HistoryMessage),
    Players(players::PlayersMessage),
    Stats(stats::StatsMessage),
    ArtLoaded(String, Result<Vec<u8>, String>),
    GoHome,
    RetryDatabase,
    Storage(crate::storage::StorageMessage),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let (conn, fatal_error) = match db::open() {
            Ok(conn) => (conn, None),
            Err(e) => (
                Connection::open_in_memory().expect("SQLite initialization failed"),
                Some(format!(
                    "Couldn’t open your database: {e}. Your saved files have not been replaced."
                )),
            ),
        };
        let mut home = home::HomeState::load(&conn);
        let players = match db::list_players(&conn) {
            Ok(players) => players,
            Err(e) => {
                home.error = Some(format!("Couldn’t load players: {e}"));
                Vec::new()
            }
        };
        let feedback = if fatal_error.is_none() {
            crate::feedback::Service::start(&conn)
        } else {
            Default::default()
        };
        (
            Self {
                feedback,
                conn,
                fatal_error,
                rematch: None,
                players,
                image_cache: HashMap::new(),
                screen: Screen::Home,
                home,
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
            // Only ticks while a Scryfall rate-limit lockout is counting
            // down, so the wait is visible on the search button.
            Screen::Setup(state) if state.cooldown.active() => {
                iced::time::every(std::time::Duration::from_secs(1))
                    .map(|_| Message::Setup(setup::SetupMessage::CooldownTick))
            }
            Screen::Players(state) if state.cooldown.active() => {
                iced::time::every(std::time::Duration::from_secs(1))
                    .map(|_| Message::Players(players::PlayersMessage::CooldownTick))
            }
            _ => Subscription::none(),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        if self.fatal_error.is_some() {
            if matches!(message, Message::RetryDatabase) {
                let (app, task) = Self::new();
                *self = app;
                return task;
            }
            return Task::none();
        }
        match message {
            Message::RetryDatabase => Task::none(),
            Message::Storage(msg) => {
                let restoring = matches!(msg, crate::storage::StorageMessage::ConfirmRestore);
                if restoring {
                    self.feedback = Default::default();
                    self.rematch = None;
                    self.image_cache.clear();
                }
                if let Screen::Storage(state) = &mut self.screen {
                    let task = crate::storage::update(state, &mut self.conn, msg);
                    if restoring {
                        self.feedback = crate::feedback::Service::start(&self.conn);
                    }
                    return task;
                }
                Task::none()
            }
            Message::ArtLoaded(id, result) => {
                if let Ok(bytes) = result {
                    // Decoded up front rather than handed over as raw bytes:
                    // framing needs the art's real pixel dimensions to work
                    // out how it covers a tile, and an undecoded handle
                    // never exposes them.
                    if let Ok(decoded) = ::image::load_from_memory(&bytes) {
                        let rgba = decoded.to_rgba8();
                        let (w, h) = (rgba.width(), rgba.height());
                        self.image_cache
                            .insert(id, image::Handle::from_rgba(w, h, rgba.into_raw()));
                    }
                }
                Task::none()
            }
            Message::GoHome => {
                self.home = home::HomeState::load(&self.conn);
                self.home.play_again = self.rematch.is_some();
                self.refresh_players();
                self.screen = Screen::Home;
                Task::none()
            }
            Message::Home(msg) => {
                match msg {
                    home::HomeMessage::Retry => {
                        self.home = home::HomeState::load(&self.conn);
                        self.home.play_again = self.rematch.is_some();
                        self.refresh_players();
                    }
                    home::HomeMessage::Storage => {
                        self.screen = Screen::Storage(crate::storage::StorageState::default());
                    }
                    home::HomeMessage::ResumeGame => match crate::session::load(&self.conn) {
                        Ok(Some(mut state)) => {
                            if let Err(error) = crate::session::save(&self.conn, &state) {
                                state.error = Some(error);
                            }
                            crate::feedback::refresh_tiles(&self.conn, &mut state, &self.feedback);
                            let task = self.game_art(&state);
                            self.screen = Screen::Game(state);
                            return task;
                        }
                        Ok(None) => self.home = home::HomeState::load(&self.conn),
                        Err(e) => self.home.error = Some(e),
                    },
                    home::HomeMessage::PlayAgain => {
                        if !self.home.resumable {
                            if let Some(state) = self.rematch.take() {
                                self.screen = Screen::Setup(state);
                            }
                        }
                    }
                    home::HomeMessage::StartGame => {
                        if !self.home.resumable {
                            self.screen = Screen::Setup(setup::SetupState::new());
                        }
                    }
                    home::HomeMessage::ViewStats => {
                        let state = stats::StatsState::load(&self.conn);
                        let task = state.art_task();
                        self.screen = Screen::Stats(state);
                        return task;
                    }
                    home::HomeMessage::ViewGame(id) => {
                        let mut state = history::HistoryState::load(&self.conn);
                        history::update(
                            &mut state,
                            &self.conn,
                            history::HistoryMessage::ViewGame(id),
                        );
                        self.screen = Screen::History(state);
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
                    if let Some(setup::Action::StartGame(seats, layout, turn_order)) = action {
                        let mut game = game::GameState::new(seats, layout, turn_order);
                        game.help_open = db::setting(&self.conn, "gestures_seen")
                            .ok()
                            .flatten()
                            .is_none();
                        game.paused = game.help_open;
                        if let Err(e) = crate::session::save(&self.conn, &game) {
                            game.error = Some(e);
                        }
                        self.screen = Screen::Game(game);
                    }
                    task
                } else {
                    Task::none()
                };
                // Cheap local query; keeps the player list current after
                // adding a new player, without threading a shared cache
                // through every setup message.
                self.refresh_players();
                task
            }
            Message::Game(msg) => {
                if let Screen::Game(state) = &mut self.screen {
                    if let game::GameMessage::OpenFeedback(seat) = msg {
                        // Existing saves from before this feature get their links here too.
                        if let Err(error) = crate::session::save(&self.conn, state) {
                            state.error = Some(error);
                            return Task::none();
                        }
                        state.feedback_panel = Some(crate::feedback::panel(
                            &self.conn,
                            state,
                            seat,
                            &self.feedback,
                        ));
                        return Task::none();
                    }
                    let (task, action) = game::update(state, &mut self.conn, msg);
                    crate::feedback::refresh_tiles(&self.conn, state, &self.feedback);
                    match action {
                        Some(game::Action::Finished)
                        | Some(game::Action::Abandoned)
                        | Some(game::Action::Suspended) => {
                            if matches!(action, Some(game::Action::Finished)) {
                                self.rematch = Some(setup::SetupState::rematch(state, &self.conn));
                            }
                            self.home = home::HomeState::load(&self.conn);
                            self.home.play_again = self.rematch.is_some();
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
                    if let history::HistoryMessage::OpenFeedback(member) = msg {
                        state.feedback_panel = Some(crate::feedback::saved_panel(
                            &self.conn,
                            member,
                            &self.feedback,
                        ));
                        return Task::none();
                    }
                    history::update(state, &self.conn, msg);
                }
                Task::none()
            }
            Message::Stats(msg) => {
                if let Screen::Stats(state) = &mut self.screen {
                    if matches!(msg, stats::StatsMessage::Retry) {
                        *state = stats::StatsState::load(&self.conn);
                        return state.art_task();
                    }
                    stats::update(state, msg);
                }
                Task::none()
            }
            Message::Players(msg) => {
                let task = if let Screen::Players(state) = &mut self.screen {
                    players::update(state, &self.conn, msg)
                } else {
                    Task::none()
                };
                self.refresh_players();
                task
            }
        }
    }

    fn refresh_players(&mut self) {
        match db::list_players(&self.conn) {
            Ok(players) => self.players = players,
            Err(e) => self.home.error = Some(format!("Couldn’t load players: {e}")),
        }
    }
    fn game_art(&self, state: &game::GameState) -> Task<Message> {
        let urls: Vec<String> = state
            .seats
            .iter()
            .flat_map(|s| [Some(&s.commander), s.partner.as_ref()])
            .flatten()
            .filter_map(|c| c.portrait_url().map(str::to_owned))
            .collect();
        crate::screens::players::fetch_all(urls)
    }
    pub fn view(&self) -> Element<'_, Message> {
        if let Some(error) = &self.fatal_error {
            return iced::widget::container(
                iced::widget::column![
                    iced::widget::text("Unable to open saved data").size(style::T_TITLE),
                    iced::widget::text(error),
                    style::touch_button("Retry", style::T_ACTION).on_press(Message::RetryDatabase)
                ]
                .spacing(style::GAP),
            )
            .padding(32)
            .into();
        }
        match &self.screen {
            Screen::Storage(state) => crate::storage::view(state),
            Screen::Home => home::view(&self.home, &self.players),
            Screen::Setup(state) => setup::view(state, &self.players, &self.image_cache),
            Screen::Game(state) => game::view(state, &self.image_cache),
            Screen::Stats(state) => stats::view(state, &self.image_cache),
            Screen::History(state) => history::view(state),
            Screen::Players(state) => players::view(state, &self.image_cache),
        }
    }
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use crate::model::WinReason;
    use game::{CounterTarget, GameMessage};

    fn fixture() -> App {
        let (conn, game) = crate::session::tests::fixture();
        App {
            feedback: Default::default(),
            home: home::HomeState::load(&conn),
            players: db::list_players(&conn).unwrap(),
            conn,
            fatal_error: None,
            rematch: None,
            image_cache: HashMap::new(),
            screen: Screen::Game(game),
        }
    }

    #[test]
    fn suspend_resume_finish_correct_and_rematch_through_app_messages() {
        let mut app = fixture();
        for msg in [
            GameMessage::CounterPressStart(0, CounterTarget::Life(0), -1),
            GameMessage::CounterPressEnd(CounterTarget::Life(0), -1),
            GameMessage::SaveAndHome,
        ] {
            let _ = app.update(Message::Game(msg));
        }
        assert!(matches!(app.screen, Screen::Home));
        assert!(app.home.resumable);
        let _ = app.update(Message::Home(home::HomeMessage::ResumeGame));
        let Screen::Game(game) = &app.screen else {
            panic!("resume did not open game")
        };
        assert!(game.paused);
        assert_eq!(game.seats[0].life, 39);
        for msg in [
            GameMessage::TogglePause,
            GameMessage::StartDeclareWinner(0),
            GameMessage::PickWinReason(WinReason::CombatDamage),
            GameMessage::ConfirmEndGame,
        ] {
            let _ = app.update(Message::Game(msg));
        }
        assert!(matches!(app.screen, Screen::Home));
        assert!(!app.home.resumable);
        assert!(app.home.play_again);
        let games = db::list_games(&app.conn).unwrap();
        assert_eq!(games.len(), 1);
        let _ = app.update(Message::Home(home::HomeMessage::ViewGame(games[0].id)));
        let detail = db::game_detail(&app.conn, games[0].id).unwrap();
        for msg in [
            history::HistoryMessage::Edit,
            history::HistoryMessage::Winner(Some(detail.seats[1].game_player_id)),
            history::HistoryMessage::Reason(WinReason::Poison),
            history::HistoryMessage::Review,
            history::HistoryMessage::SaveCorrection,
        ] {
            let _ = app.update(Message::History(msg));
        }
        let _ = app.update(Message::GoHome);
        assert_eq!(app.home.recent[0].winner_name.as_deref(), Some("Bo"));
        let _ = app.update(Message::Home(home::HomeMessage::ViewStats));
        let Screen::Stats(state) = &app.screen else {
            panic!("stats did not open")
        };
        assert!(state.error.is_none());
        let _ = app.update(Message::GoHome);
        let _ = app.update(Message::Home(home::HomeMessage::PlayAgain));
        let Screen::Setup(state) = &app.screen else {
            panic!("rematch did not open")
        };
        assert!(state.all_seats_ready());
        assert_eq!(state.first_seat, None);
    }

    #[test]
    fn failed_suspend_keeps_game_open_and_retry_preserves_counters() {
        let mut app = fixture();
        app.conn.pragma_update(None, "query_only", true).unwrap();
        let _ = app.update(Message::Game(GameMessage::SaveAndHome));
        let Screen::Game(game) = &app.screen else {
            panic!("unsaved game was closed")
        };
        assert!(game.error.is_some());
        app.conn.pragma_update(None, "query_only", false).unwrap();
        let _ = app.update(Message::Game(GameMessage::RetrySave));
        let Screen::Game(game) = &app.screen else {
            panic!("retry closed game")
        };
        assert!(game.error.is_none());
        let _ = app.update(Message::Game(GameMessage::SaveAndHome));
        assert!(app.home.resumable);
        assert_eq!(
            crate::session::load(&app.conn).unwrap().unwrap().seats[0].life,
            40
        );
    }
}
