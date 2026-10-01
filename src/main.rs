mod app;
mod art;
mod cache;
mod cards;
mod commander_salt;
mod db;
mod feedback;
mod session;
mod storage;

mod icon;
mod keyboard;
mod layout;
mod model;
mod moxfield;
mod panned_image;
mod power;
mod preferences;
mod rotated;
mod salt;
mod screens;
mod scryfall;
mod style;
mod table_preview;

fn main() -> iced::Result {
    iced::application(app::App::title, app::App::update, app::App::view)
        .subscription(app::App::subscription)
        .theme(app::App::theme)
        .window(iced::window::Settings {
            // Sized for this laptop's panel (3000x2000 @ 1.6 scale), minus
            // the compositor's top bar.
            size: iced::Size::new(1875.0, 1205.0),
            position: iced::window::Position::Centered,
            icon: icon::window_icon(),
            ..Default::default()
        })
        .antialiasing(true)
        .run_with(app::App::new)
}
