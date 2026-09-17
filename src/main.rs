mod app;
mod db;
mod layout;
mod model;
mod screens;
mod scryfall;
mod style;

fn main() -> iced::Result {
    iced::application(app::App::title, app::App::update, app::App::view)
        .subscription(app::App::subscription)
        .theme(app::App::theme)
        .run_with(app::App::new)
}
