//! Circular board control. Its hit area follows the disc, leaving corner taps
//! available to the counters underneath.
use super::{format_duration, GameMessage, GameState};
use crate::{app::Message, style};
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, Text};
use iced::{
    alignment, font, mouse, touch, Color, Element, Font, Point, Rectangle, Renderer, Size, Theme,
};

pub(crate) fn diameter(size: Size) -> f32 {
    if size.width < 1100.0 || size.height < 900.0 {
        168.0
    } else {
        200.0
    }
}

pub(super) fn view(state: &GameState, diameter: f32) -> Element<'_, Message> {
    Canvas::new(Disc { game: state })
        .width(diameter)
        .height(diameter)
        .into()
}

struct Disc<'a> {
    game: &'a GameState,
}

fn contains(bounds: Rectangle, position: Point) -> bool {
    let center = bounds.center();
    (position.x - center.x).powi(2) + (position.y - center.y).powi(2)
        <= (bounds.width.min(bounds.height) / 2.0).powi(2)
}

impl canvas::Program<Message> for Disc<'_> {
    type State = ();

    fn update(
        &self,
        _: &mut (),
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        let position = match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                cursor.position()
            }
            canvas::Event::Touch(touch::Event::FingerPressed { position, .. }) => Some(position),
            _ => None,
        };
        if position.is_some_and(|p| contains(bounds, p)) {
            let action = if self.game.damage_focus.is_some() {
                GameMessage::EndDamageFocus
            } else {
                GameMessage::OpenGameMenu
            };
            (canvas::event::Status::Captured, Some(Message::Game(action)))
        } else {
            (canvas::event::Status::Ignored, None)
        }
    }

    fn mouse_interaction(
        &self,
        _: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.position().is_some_and(|p| contains(bounds, p)) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let radius = bounds.width.min(bounds.height) / 2.0;
        let hovered = cursor.position().is_some_and(|p| contains(bounds, p));
        frame.fill(&Path::circle(center, radius), style::SURFACE_0);
        let face = Path::circle(center, radius - 9.0);
        frame.fill(
            &face,
            if hovered {
                style::SURFACE_2
            } else {
                style::SURFACE_1
            },
        );
        frame.stroke(
            &face,
            Stroke::default()
                .with_width(1.5)
                .with_color(if self.game.damage_focus.is_some() {
                    style::ACCENT_BRIGHT
                } else {
                    style::SURFACE_3
                }),
        );
        let mut label = |content: String, y: f32, size: f32, color: Color, numeric: bool| {
            // Keep long-running clocks and large turn counts inside the circle.
            let size = size.min((radius * 1.4) / (content.chars().count().max(1) as f32 * 0.61));
            frame.fill_text(Text {
                content,
                position: Point::new(center.x, center.y + y),
                color,
                size: size.into(),
                font: Font {
                    family: if numeric {
                        font::Family::Monospace
                    } else {
                        font::Family::SansSerif
                    },
                    weight: if numeric || size >= 24.0 {
                        font::Weight::Medium
                    } else {
                        font::Weight::Normal
                    },
                    ..Font::DEFAULT
                },
                horizontal_alignment: alignment::Horizontal::Center,
                vertical_alignment: alignment::Vertical::Center,
                ..Text::default()
            });
        };
        if let Some(focus) = self.game.damage_focus {
            label("COMMANDER".into(), -43.0, 12.0, style::TEXT_MUTED, false);
            label("DAMAGE".into(), -27.0, 12.0, style::TEXT_MUTED, false);
            label("Done".into(), 2.0, 30.0, style::TEXT, false);
            let name = &self.game.seats[focus].player.name;
            let short = if name.chars().count() > 14 {
                format!("{}…", name.chars().take(13).collect::<String>())
            } else {
                name.clone()
            };
            label(
                format!("To {short}"),
                36.0,
                14.0,
                style::ACCENT_BRIGHT,
                false,
            );
        } else {
            label("GAME".into(), -44.0, 12.0, style::TEXT_MUTED, false);
            label(
                format_duration(self.game.game_seconds),
                -16.0,
                32.0,
                style::TEXT,
                true,
            );
            label(
                format!(
                    "Turn {} · {}",
                    self.game.turn_number,
                    format_duration(self.game.turn_seconds)
                ),
                18.0,
                14.0,
                style::TEXT_MUTED,
                false,
            );
            label(
                if self.game.paused { "Paused" } else { "Menu" }.into(),
                43.0,
                12.0,
                style::ACCENT_BRIGHT,
                false,
            );
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corners_pass_through_but_mouse_and_touch_activate_inside_disc() {
        let (_, game) = crate::session::tests::fixture();
        let disc = Disc { game: &game };
        let bounds = Rectangle {
            x: 100.0,
            y: 200.0,
            width: 168.0,
            height: 168.0,
        };
        for (position, expected) in [(Point::new(101.0, 201.0), false), (bounds.center(), true)] {
            for event in [
                canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                canvas::Event::Touch(touch::Event::FingerPressed {
                    id: touch::Finger(0),
                    position,
                }),
            ] {
                let (status, msg) = canvas::Program::update(
                    &disc,
                    &mut (),
                    event,
                    bounds,
                    mouse::Cursor::Available(position),
                );
                assert_eq!(status == canvas::event::Status::Captured, expected);
                assert_eq!(
                    matches!(msg, Some(Message::Game(GameMessage::OpenGameMenu))),
                    expected
                );
            }
        }
        let mut game = game;
        game.damage_focus = Some(0);
        let disc = Disc { game: &game };
        let (_, msg) = canvas::Program::update(
            &disc,
            &mut (),
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            mouse::Cursor::Available(bounds.center()),
        );
        assert!(matches!(
            msg,
            Some(Message::Game(GameMessage::EndDamageFocus))
        ));
    }
}
