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
        // Sized for this laptop's panel (3000x2000 @ 1.6 scale), minus the
        // compositor's top bar.
        .window_size((1875.0, 1205.0))
        .centered()
        .antialiasing(true)
        .run_with(app::App::new)
}
