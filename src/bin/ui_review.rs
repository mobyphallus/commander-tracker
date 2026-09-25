#![allow(dead_code)]
#[path = "../app.rs"]
mod app;
#[path = "../art.rs"]
mod art;
#[path = "../cache.rs"]
mod cache;
#[path = "../cards.rs"]
mod cards;
#[path = "../db.rs"]
mod db;
#[path = "../feedback.rs"]
mod feedback;
#[path = "../icon.rs"]
mod icon;
#[path = "../keyboard.rs"]
mod keyboard;
#[path = "../layout.rs"]
mod layout;
#[path = "../model.rs"]
mod model;
#[path = "../moxfield.rs"]
mod moxfield;
#[path = "../panned_image.rs"]
mod panned_image;
#[path = "../rotated.rs"]
mod rotated;
#[path = "../salt.rs"]
mod salt;
#[path = "../screens/mod.rs"]
mod screens;
#[path = "../scryfall.rs"]
mod scryfall;
#[path = "../session.rs"]
mod session;
#[path = "../storage.rs"]
mod storage;
#[path = "../style.rs"]
mod style;
#[path = "../table_preview.rs"]
mod table_preview;
use iced::{window, Element, Size, Task};
use screens::{game, history, home, setup, stats};
struct Review {
    conn: rusqlite::Connection,
    home: home::HomeState,
    game: game::GameState,
    history: history::HistoryState,
    stats: stats::StatsState,
    storage: storage::StorageState,
    setup: setup::SetupState,
    players: Vec<model::Player>,
    decks: Vec<model::SavedDeck>,
    deck_list: screens::deck_list::State,
    player_manager: screens::players::PlayersState,
    images: cards::Images,
    page: usize,
}
#[derive(Debug, Clone)]
enum Msg {
    Capture,
    Captured(window::Screenshot),
    App(app::Message),
}
const PAGES: &[&str] = &[
    "home-resume",
    "home-rematch",
    "game-menu",
    "undo",
    "help",
    "save-error",
    "history",
    "history-search",
    "correction",
    "confirm-correction",
    "stats",
    "restore-confirm",
    "rematch",
    "partner-damage",
    "game-board",
    "game-paused",
    "score-badges",
    "deck-list",
    "commander-options",
    "setup-badges",
    "feedback-out",
    "feedback-qr",
    "feedback-history",
];
impl Review {
    fn new() -> (Self, Task<Msg>) {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        db::init(&conn).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        let players: Vec<_> = [
            "Alexandra the Unreasonably Long-Named",
            "Bo",
            "Casey",
            "Drew",
        ]
        .into_iter()
        .map(|name| db::create_player(&conn, name).unwrap())
        .collect();
        let commanders: Vec<_> = [
            "Tymna the Weaver",
            "Kraum, Ludevic's Opus",
            "Atraxa, Praetors' Voice",
            "The Ur-Dragon",
            "Rograkh, Son of Rohgahh",
        ]
        .into_iter()
        .enumerate()
        .map(|(i, name)| {
            db::upsert_commander(&conn, &format!("review-{i}"), name, None, None, "WUBRG").unwrap()
        })
        .collect();
        let seats: Vec<_> = players
            .iter()
            .enumerate()
            .map(|(i, p)| {
                model::Seat::new(p.clone(), commanders[i].clone(), 40)
                    .with_partner((i == 0).then(|| commanders[4].clone()))
            })
            .collect();
        for i in 0..6 {
            let ended_at = chrono::Utc::now() - chrono::Duration::days(i);
            db::record_game(
                &mut conn,
                &model::FinishedGame {
                    elapsed_seconds: 3600,
                    seats: seats.clone(),
                    winner_seat: Some(i as usize % 4),
                    win_reason: Some(model::WinReason::CombatDamage),
                    ending_turn: 9,
                    kills: vec![],
                    started_at: ended_at - chrono::Duration::hours(1),
                    ended_at,
                },
            )
            .unwrap();
        }
        let mut game =
            game::GameState::new(seats, layout::options_for(4)[0].clone(), vec![0, 1, 2, 3]);
        let _ = game::update(&mut game, &mut conn, game::GameMessage::NextTurn);
        let _ = game::update(
            &mut game,
            &mut conn,
            game::GameMessage::CounterPressStart(0, game::CounterTarget::Life(0), -1),
        );
        let _ = game::update(
            &mut game,
            &mut conn,
            game::GameMessage::CounterPressEnd(game::CounterTarget::Life(0), -1),
        );
        let images = cards::Images::new();
        let player = game.seats[0].player.clone();
        let commander = game.seats[0].commander.id;
        db::record_player_commander_use(&conn, player.id, commander).unwrap();
        db::set_deck_link(
            &conn,
            player.id,
            commander,
            "review-list",
            "https://moxfield.com/decks/review-list",
        )
        .unwrap();
        conn.execute("UPDATE deck_analysis SET bracket=3, salt_total=127 WHERE player_id=?1 AND commander_id=?2", (player.id, commander)).unwrap();
        let make_card = |name: &str, kind: &str, quantity| moxfield::Card {
            name: name.into(),
            type_line: kind.into(),
            quantity,
            scryfall_id: String::new(),
            oracle_text: String::new(),
            color_identity: String::new(),
            usd: None,
            reserved: false,
        };
        let sample = moxfield::Deck {
            public_id: "review-list".into(),
            name: "Tymna and Rograkh — Sample deck".into(),
            url: "https://moxfield.com/decks/review-list".into(),
            owner_bracket: Some(3),
            auto_bracket: None,
            commanders: vec![
                make_card("Tymna the Weaver", "Legendary Creature", 1),
                make_card("Rograkh, Son of Rohgahh", "Legendary Creature", 1),
            ],
            mainboard: vec![
                make_card("Esper Sentinel", "Artifact Creature", 1),
                make_card("Sol Ring", "Artifact", 1),
                make_card("Swords to Plowshares", "Instant", 1),
                make_card("Plains", "Basic Land — Plains", 36),
            ],
        };
        let (mut deck_list, _) =
            screens::deck_list::State::open(&conn, player.id, commander, "Tymna + Rograkh".into());
        let _ = deck_list.update(
            &conn,
            screens::deck_list::Message::Loaded("review-list".into(), Ok(sample)),
        );
        let mut player_manager = screens::players::PlayersState::load(&conn);
        let _ = screens::players::update(
            &mut player_manager,
            &conn,
            screens::players::PlayersMessage::ManageCommanders(player),
        );
        let _ = screens::players::update(
            &mut player_manager,
            &conn,
            screens::players::PlayersMessage::SelectDeck(commander),
        );
        let mut setup = setup::SetupState::rematch(&game, &conn);
        for (seat, saved) in setup.seats.iter_mut().zip(&game.seats) {
            seat.commander_history = vec![model::SavedDeck {
                commander: saved.commander.clone(),
                partner: saved.partner.clone(),
            }];
        }
        let decks = game
            .seats
            .iter()
            .map(|seat| model::SavedDeck {
                commander: seat.commander.clone(),
                partner: seat.partner.clone(),
            })
            .collect();
        let mut review = Self {
            decks,
            deck_list,
            player_manager,
            home: home::HomeState::load(&conn),
            history: history::HistoryState::load(&conn),
            stats: stats::StatsState::load(&conn),
            storage: storage::StorageState::default(),
            conn,
            game,
            setup,
            players,
            images,
            page: review_size_index() * PAGES.len() + review_start_page(),
        };
        review.configure();
        (review, Self::later())
    }
    fn later() -> Task<Msg> {
        Task::perform(
            async { tokio::time::sleep(std::time::Duration::from_millis(700)).await },
            |_| Msg::Capture,
        )
    }
    fn configure(&mut self) {
        let p = self.page % PAGES.len();
        if p == 19 {
            self.setup.stage = setup::SetupStage::Grid;
        }
        self.game.error = None;
        self.game.paused = p == 15;
        self.game.game_seconds = 3725;
        self.game.turn_seconds = 83;
        let _ = game::update(
            &mut self.game,
            &mut self.conn,
            game::GameMessage::EndDamageFocus,
        );
        if p == 13 {
            let _ = game::update(
                &mut self.game,
                &mut self.conn,
                game::GameMessage::StartDamageFocus(1),
            );
        }
        self.game.feedback_panel = None;
        if p == 20 {
            self.game.seats[0].mark_out(model::Elimination {
                cause: model::OutCause::Concede,
                killer_seat: None,
                turn: 1,
            });
            session::save(&self.conn, &self.game).unwrap();
        }
        if p == 21 {
            let url = "http://192.168.1.50:8787/f/0123456789abcdef0123456789abcdef";
            self.game.feedback_panel = Some(feedback::Panel {
                name: "Alexandra".into(),
                url: Some(url.into()),
                qr: feedback::qr(url),
                error: None,
            });
        }
        if p == 22 {
            let token: String = self
                .conn
                .query_row(
                    "SELECT token FROM feedback_members WHERE match_key=?1 AND seat=0",
                    [self.game.started_at.to_rfc3339()],
                    |r| r.get(0),
                )
                .unwrap();
            feedback::submit(&mut self.conn,&token,&feedback::Draft {rating:"4".into(),problem:"1".into(),kingmaker:"2".into(),notes:"Fun game overall. The final turns felt slow; let's agree on quicker turns next time.".into()}).unwrap();
            let _ = game::update(
                &mut self.game,
                &mut self.conn,
                game::GameMessage::StartDeclareWinner(1),
            );
            let _ = game::update(
                &mut self.game,
                &mut self.conn,
                game::GameMessage::PickWinReason(model::WinReason::CombatDamage),
            );
            let _ = game::update(
                &mut self.game,
                &mut self.conn,
                game::GameMessage::ConfirmEndGame,
            );
            let id = db::list_games(&self.conn).unwrap()[0].id;
            history::update(
                &mut self.history,
                &self.conn,
                history::HistoryMessage::ViewGame(id),
            );
        }
        self.game.game_menu_open = p == 2;
        self.game.undo_open = p == 3;
        self.game.help_open = p == 4;
        if p == 0 {
            self.home.resumable = true;
            self.home.play_again = false;
        }
        if p == 1 {
            self.home.resumable = false;
            self.home.play_again = true;
        }
        if p == 5 {
            self.game.error = Some(
                "Couldn’t save this game: disk is full. Your game is still open; try again.".into(),
            );
        }
        if p == 6 {
            self.history = history::HistoryState::load(&self.conn);
        }
        if p == 7 {
            history::update(
                &mut self.history,
                &self.conn,
                history::HistoryMessage::Search,
            );
        }
        if p == 8 {
            if let Some(id) = self.history.games.first().map(|g| g.id) {
                history::update(
                    &mut self.history,
                    &self.conn,
                    history::HistoryMessage::ViewGame(id),
                );
                history::update(&mut self.history, &self.conn, history::HistoryMessage::Edit);
            }
        }
        if p == 9 {
            history::update(
                &mut self.history,
                &self.conn,
                history::HistoryMessage::Review,
            );
        }
        if p == 11 {
            let _ = storage::update(
                &mut self.storage,
                &mut self.conn,
                storage::StorageMessage::Chosen(Ok(Some(
                    std::env::temp_dir().join("commander-pod-review-backup.db"),
                ))),
            );
        }
    }
    fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Capture => window::get_latest()
                .and_then(window::screenshot)
                .map(Msg::Captured),
            Msg::Captured(shot) => {
                let target = review_size();
                let width = (target.width as f64 * shot.scale_factor).round() as u32;
                let height = (target.height as f64 * shot.scale_factor).round() as u32;
                assert!(
                    shot.size.width >= width && shot.size.height >= height,
                    "review surface too small: {}x{}; need {}x{}",
                    shot.size.width,
                    shot.size.height,
                    width,
                    height
                );
                let shot = shot
                    .crop(iced::Rectangle {
                        x: 0,
                        y: 0,
                        width,
                        height,
                    })
                    .expect("review viewport must fit screenshot");
                let path = format!(
                    "/tmp/commander-finish-{}-{}.png",
                    self.page / PAGES.len(),
                    PAGES[self.page % PAGES.len()]
                );
                {
                    use image::ImageEncoder;
                    // Bound encoded captures independently of the window's review layout.
                    let pixels = image::RgbaImage::from_raw(
                        shot.size.width,
                        shot.size.height,
                        shot.bytes.to_vec(),
                    )
                    .expect("screenshot must contain RGBA pixels");
                    let pixels = if pixels.width() > 1200 {
                        let height = ((u64::from(pixels.height()) * 1200)
                            / u64::from(pixels.width()))
                        .max(1) as u32;
                        image::imageops::resize(
                            &pixels,
                            1200,
                            height,
                            image::imageops::FilterType::Lanczos3,
                        )
                    } else {
                        pixels
                    };
                    let file = std::fs::File::create(&path).unwrap();
                    image::codecs::png::PngEncoder::new_with_quality(
                        file,
                        image::codecs::png::CompressionType::Default,
                        image::codecs::png::FilterType::Adaptive,
                    )
                    .write_image(
                        pixels.as_raw(),
                        pixels.width(),
                        pixels.height(),
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                }
                eprintln!(
                    "{}: {}x{} ({} bytes)",
                    path,
                    shot.size.width,
                    shot.size.height,
                    std::fs::metadata(&path).unwrap().len()
                );
                self.page += 1;
                if self.page % PAGES.len() == 0 {
                    std::process::exit(0)
                }
                self.configure();
                Self::later()
            }
            Msg::App(_) => Task::none(),
        }
    }
    fn view(&self) -> Element<'_, Msg> {
        let p = self.page % PAGES.len();
        let view = match p {
            0 | 1 => home::view(&self.home, &self.players),
            2..=5 | 13..=15 | 20..=21 => game::view(&self.game, &self.images),
            6..=9 | 22 => history::view(&self.history),
            16 => iced::widget::container(cards::grid(
                self.decks
                    .iter()
                    .enumerate()
                    .map(|(i, deck)| {
                        cards::deck_tile(
                            deck,
                            i == 0,
                            &self.images,
                            cards::DeckMeta {
                                bracket: Some(i as u8 + 2),
                                salt: Some([0., 82., 247., 1234.][i]),
                            },
                            app::Message::GoHome,
                            app::Message::GoHome,
                            app::Message::GoHome,
                            320.,
                        )
                    })
                    .collect(),
            ))
            .padding(24)
            .into(),
            17 => screens::deck_list::view(&self.deck_list)
                .map(|msg| app::Message::Players(screens::players::PlayersMessage::DeckList(msg))),
            18 => screens::players::view(&self.player_manager, &self.images),
            10 => stats::view(&self.stats, &self.images),
            11 => storage::view(&self.storage),
            _ => setup::view(&self.setup, &self.players, &self.images),
        };
        iced::widget::container(view.map(Msg::App))
            .width(review_size().width)
            .height(review_size().height)
            .into()
    }
}
fn review_start_page() -> usize {
    let page = std::env::args()
        .nth(2)
        .map(|s| s.parse::<usize>().expect("page must be an integer"))
        .unwrap_or(0);
    assert!(page < PAGES.len(), "review page out of range");
    page
}
fn review_size_index() -> usize {
    std::env::args()
        .nth(1)
        .map(|v| v.parse::<usize>().expect("size index must be 0, 1, or 2"))
        .filter(|&i| i < 3)
        .unwrap_or(0)
}
fn review_size() -> Size {
    [
        Size::new(1875., 1205.),
        Size::new(800., 1280.),
        Size::new(1280., 800.),
    ][review_size_index()]
}
fn main() -> iced::Result {
    iced::application(
        |_: &Review| "UI Review - Commander Pod".to_string(),
        Review::update,
        Review::view,
    )
    .theme(|_| style::app_theme())
    .antialiasing(true)
    // Fit the portrait review surface on a landscape desktop. The fixed container
    // above still lays out at the requested logical dimensions.
    .scale_factor(|_| 0.5)
    .window(iced::window::Settings {
        size: review_size(),
        ..Default::default()
    })
    .run_with(Review::new)
}
