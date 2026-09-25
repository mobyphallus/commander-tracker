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
];
impl Review {
    fn new() -> (Self, Task<Msg>) {
        let source = rusqlite::Connection::open_with_flags(
            "/home/moby/.local/share/commander_pod/pod.db",
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        rusqlite::backup::Backup::new(&source, &mut conn)
            .unwrap()
            .run_to_completion(100, std::time::Duration::from_millis(5), None)
            .unwrap();
        db::init(&conn).unwrap();
        let players = db::list_players(&conn).unwrap();
        let commanders = db::all_commanders(&conn).unwrap();
        let seats = players
            .iter()
            .take(4)
            .enumerate()
            .map(|(i, p)| model::Seat::new(p.clone(), commanders[i % commanders.len()].clone(), 40))
            .collect();
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
        let mut images = cards::Images::new();
        for c in &commanders {
            if let Some(url) = c.portrait_url() {
                let mut hash: u64 = 0xcbf29ce484222325;
                for b in url.bytes() {
                    hash ^= b as u64;
                    hash = hash.wrapping_mul(0x100000001b3);
                }
                if let Ok(bytes) = std::fs::read(format!(
                    "/home/moby/.local/share/commander_pod/art/{hash:016x}"
                )) {
                    if let Ok(img) = image::load_from_memory(&bytes) {
                        let rgba = img.to_rgba8();
                        images.insert(
                            url.into(),
                            iced::widget::image::Handle::from_rgba(
                                rgba.width(),
                                rgba.height(),
                                rgba.into_raw(),
                            ),
                        );
                    }
                }
            }
        }
        let setup = setup::SetupState::rematch(&game, &conn);
        let mut review = Self {
            home: home::HomeState::load(&conn),
            history: history::HistoryState::load(&conn),
            stats: stats::StatsState::load(&conn),
            storage: storage::StorageState::default(),
            conn,
            game,
            setup,
            players,
            images,
            page: 0,
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
        self.game.error = None;
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
                    "/home/moby/Backups/commander-pod-backup.db".into(),
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
                self.page += 1;
                if self.page == PAGES.len() * 3 {
                    std::process::exit(0)
                }
                self.configure();
                if self.page == PAGES.len() {
                    Task::batch([
                        window::get_latest()
                            .and_then(|id| window::resize(id, Size::new(800., 1280.))),
                        Self::later(),
                    ])
                } else {
                    Self::later()
                }
            }
            Msg::App(_) => Task::none(),
        }
    }
    fn view(&self) -> Element<'_, Msg> {
        let p = self.page % PAGES.len();
        let view = match p {
            0 | 1 => home::view(&self.home, &self.players),
            2..=5 => game::view(&self.game, &self.images),
            6..=9 => history::view(&self.history),
            10 => stats::view(&self.stats, &self.images),
            11 => storage::view(&self.storage),
            _ => setup::view(&self.setup, &self.players, &self.images),
        };
        iced::widget::container(view.map(Msg::App))
            .width(match self.page / PAGES.len() {
                0 => 1875.,
                1 => 800.,
                _ => 1280.,
            })
            .height(if self.page / PAGES.len() == 2 {
                800.
            } else {
                1205.
            })
            .into()
    }
}
fn main() -> iced::Result {
    iced::application(
        |_: &Review| "UI Review - Commander Pod".to_string(),
        Review::update,
        Review::view,
    )
    .theme(|_| style::app_theme())
    .window(iced::window::Settings {
        size: Size::new(1875., 1205.),
        ..Default::default()
    })
    .run_with(Review::new)
}
