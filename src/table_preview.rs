//! Scalable seating diagrams shared by home and the first two setup steps.
//! The geometry and orientation come from the same layout as the live board.
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, Text};
use iced::{alignment, mouse, Element, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

use crate::layout::TableLayout;
use crate::style;

pub fn view<'a, Msg: 'a>(table: &TableLayout) -> Element<'a, Msg> {
    Canvas::new(Preview(table.clone()))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

struct Preview(TableLayout);
impl<Msg> canvas::Program<Msg> for Preview {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if self.0.columns.is_empty() {
            return vec![frame.into_geometry()];
        }
        let gap = (bounds.width * 0.025).clamp(4., 12.);
        let outer = gap;
        let width = ((bounds.width - outer * 2. - gap * (self.0.columns.len() - 1) as f32)
            / self.0.columns.len() as f32)
            .max(0.);
        for (col, seats) in self.0.columns.iter().enumerate() {
            let height = ((bounds.height - outer * 2. - gap * (seats.len() - 1) as f32)
                / seats.len() as f32)
                .max(0.);
            for (row, &seat) in seats.iter().enumerate() {
                let origin = Point::new(
                    outer + col as f32 * (width + gap),
                    outer + row as f32 * (height + gap),
                );
                let rect = Path::rounded_rectangle(
                    origin,
                    Size::new(width, height),
                    (height.min(width) * 0.12).min(style::R_MD).into(),
                );
                frame.fill(
                    &rect,
                    if seat == 0 {
                        style::ACCENT_DEEP
                    } else {
                        style::SURFACE_2
                    },
                );
                frame.stroke(
                    &rect,
                    Stroke::default()
                        .with_color(if seat == 0 {
                            style::ACCENT_BRIGHT
                        } else {
                            style::SURFACE_3
                        })
                        .with_width(1.5),
                );
                let facing = self.0.seat_orientation(seat);
                let (span, depth) = if facing.is_sideways() {
                    (height, width)
                } else {
                    (width, height)
                };
                frame.with_save(|f| {
                    f.translate(Vector::new(origin.x + width / 2., origin.y + height / 2.));
                    f.rotate(iced::Radians(facing.radians()));
                    // A short line at the player's edge makes the reading direction clear.
                    let line = Path::line(
                        Point::new(-span * 0.17, depth * 0.34),
                        Point::new(span * 0.17, depth * 0.34),
                    );
                    f.stroke(
                        &line,
                        Stroke::default()
                            .with_color(style::ACCENT_BRIGHT)
                            .with_width(3.),
                    );
                    f.fill_text(Text {
                        content: (seat + 1).to_string(),
                        position: Point::new(0., -depth * 0.035),
                        size: (span.min(depth) * 0.3).clamp(12., 36.).into(),
                        color: style::TEXT,
                        horizontal_alignment: alignment::Horizontal::Center,
                        vertical_alignment: alignment::Vertical::Center,
                        ..Text::default()
                    });
                });
            }
        }
        vec![frame.into_geometry()]
    }
}
