//! The window/taskbar icon, drawn procedurally: five colour pips arranged in
//! a ring on a dark rounded tile. Deliberately a generic five-colour mark
//! rather than any trademarked Magic artwork.

const SIZE: u32 = 256;

/// WUBRG, in the usual clockwise order starting at the top.
const PIPS: [(u8, u8, u8); 5] = [
    (0xF6, 0xF1, 0xDF), // white
    (0x5B, 0x9B, 0xD5), // blue
    (0x2A, 0x2A, 0x33), // black
    (0xC6, 0x4B, 0x3F), // red
    (0x5F, 0xA8, 0x6B), // green
];

pub fn window_icon() -> Option<iced::window::Icon> {
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let center = SIZE as f32 / 2.0;
    let ring_radius = SIZE as f32 * 0.29;
    let pip_radius = SIZE as f32 * 0.13;
    let corner = SIZE as f32 * 0.22;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let idx = ((y * SIZE + x) * 4) as usize;

            if !inside_rounded_square(fx, fy, SIZE as f32, corner) {
                continue;
            }

            // Base tile, with the purple ring drawn onto it.
            let mut pixel = (0x19u8, 0x1Bu8, 0x20u8);
            let dist_from_center = ((fx - center).powi(2) + (fy - center).powi(2)).sqrt();
            if (dist_from_center - ring_radius).abs() < SIZE as f32 * 0.015 {
                pixel = (0x8B, 0x5C, 0xF6);
            }

            for (i, colour) in PIPS.iter().enumerate() {
                let angle = -std::f32::consts::FRAC_PI_2 + (i as f32) * std::f32::consts::TAU / 5.0;
                let px = center + ring_radius * angle.cos();
                let py = center + ring_radius * angle.sin();
                let d = ((fx - px).powi(2) + (fy - py).powi(2)).sqrt();
                if d < pip_radius {
                    pixel = *colour;
                } else if d < pip_radius + SIZE as f32 * 0.012 {
                    pixel = (0x8B, 0x5C, 0xF6);
                }
            }

            rgba[idx] = pixel.0;
            rgba[idx + 1] = pixel.1;
            rgba[idx + 2] = pixel.2;
            rgba[idx + 3] = 0xFF;
        }
    }

    iced::window::icon::from_rgba(rgba, SIZE, SIZE).ok()
}

fn inside_rounded_square(x: f32, y: f32, size: f32, corner: f32) -> bool {
    let cx = x.min(size - x);
    let cy = y.min(size - y);
    if cx >= corner || cy >= corner {
        return true;
    }
    let dx = corner - cx;
    let dy = corner - cy;
    (dx * dx + dy * dy).sqrt() <= corner
}

use crate::style;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, LineCap, LineJoin, Path, Stroke};
use iced::{Color, Element, Point, Rectangle, Renderer, Size, Theme};

/// Original, font-independent symbols on a shared 24-unit drawing grid.
#[derive(Debug, Clone, Copy)]
pub enum Glyph {
    Back,
    Next,
    Players,
    Decks,
    History,
    Stats,
    Search,
    Add,
    Edit,
    Image,
    Frame,
    Delete,
    Check,
    Pause,
    Play,
    Close,
    Heart,
    Poison,
    Shield,
    Trophy,
    Bracket,
    Salt,
    Rotate(bool),
    Shuffle,
    Mana(char),
}

pub fn view<'a, Msg: 'a>(glyph: Glyph, size: f32, ink: Color) -> Element<'a, Msg> {
    Canvas::new(Symbol { glyph, ink })
        .width(size)
        .height(size)
        .into()
}

pub fn mana<'a, Msg: 'a>(symbol: char, size: f32) -> Element<'a, Msg> {
    view(Glyph::Mana(symbol), size, style::MANA_INK)
}

struct Symbol {
    glyph: Glyph,
    ink: Color,
}

fn line(frame: &mut Frame, points: &[(f32, f32)], ink: Color) {
    let path = Path::new(|p| {
        if let Some(&(x, y)) = points.first() {
            p.move_to(Point::new(x, y));
            for &(x, y) in &points[1..] {
                p.line_to(Point::new(x, y));
            }
        }
    });
    frame.stroke(
        &path,
        Stroke::default()
            .with_color(ink)
            .with_width(1.8)
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round),
    );
}

fn circle(frame: &mut Frame, x: f32, y: f32, r: f32, ink: Color) {
    frame.stroke(
        &Path::circle(Point::new(x, y), r),
        Stroke::default().with_color(ink).with_width(1.8),
    );
}

impl<Msg> canvas::Program<Msg> for Symbol {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        f.scale(bounds.width.min(bounds.height) / 24.0);
        draw(&mut f, self.glyph, self.ink);
        vec![f.into_geometry()]
    }
}

/// Draw into an existing canvas, including player-facing controls.
pub fn draw(f: &mut Frame, glyph: Glyph, c: Color) {
    match glyph {
        Glyph::Bracket => {
            // Three rising tiers, distinct from the statistics bar chart.
            line(
                f,
                &[(3., 7.), (12., 3.), (21., 7.), (12., 11.), (3., 7.)],
                c,
            );
            line(f, &[(3., 12.), (12., 16.), (21., 12.)], c);
            line(f, &[(3., 17.), (12., 21.), (21., 17.)], c);
        }
        Glyph::Salt => {
            // A salt shaker, including its perforated cap and salt grains.
            line(
                f,
                &[(8., 8.), (6., 20.), (18., 20.), (16., 8.), (8., 8.)],
                c,
            );
            line(
                f,
                &[
                    (8., 8.),
                    (8., 5.),
                    (10., 3.),
                    (14., 3.),
                    (16., 5.),
                    (16., 8.),
                ],
                c,
            );
            line(f, &[(8., 5.), (16., 5.)], c);
            for (x, y) in [(10., 13.), (14., 15.), (10., 17.)] {
                f.fill(&Path::circle(Point::new(x, y), 0.8), c);
            }
        }
        Glyph::Back => {
            line(f, &[(14., 5.), (7., 12.), (14., 19.)], c);
            line(f, &[(7., 12.), (21., 12.)], c);
        }
        Glyph::Next => {
            line(f, &[(10., 5.), (17., 12.), (10., 19.)], c);
            line(f, &[(3., 12.), (17., 12.)], c);
        }
        Glyph::Add => {
            line(f, &[(12., 5.), (12., 19.)], c);
            line(f, &[(5., 12.), (19., 12.)], c);
        }
        Glyph::Close => {
            line(f, &[(6., 6.), (18., 18.)], c);
            line(f, &[(18., 6.), (6., 18.)], c);
        }
        Glyph::Check => line(f, &[(4., 12.), (9., 17.), (20., 6.)], c),
        Glyph::Pause => {
            line(f, &[(8., 5.), (8., 19.)], c);
            line(f, &[(16., 5.), (16., 19.)], c);
        }
        Glyph::Play => line(f, &[(7., 4.), (20., 12.), (7., 20.), (7., 4.)], c),
        Glyph::Search => {
            circle(f, 10., 10., 6., c);
            line(f, &[(15., 15.), (21., 21.)], c);
        }
        Glyph::History => {
            circle(f, 12., 12., 8.5, c);
            line(f, &[(12., 7.), (12., 12.), (16., 14.)], c);
        }
        Glyph::Stats => {
            line(f, &[(4., 4.), (4., 20.), (21., 20.)], c);
            for (x, y) in [(8., 13.), (13., 9.), (18., 4.)] {
                line(f, &[(x, y), (x, 16.)], c);
            }
        }
        Glyph::Players => {
            circle(f, 9., 7., 3., c);
            circle(f, 18., 8., 2., c);
            let p = Path::new(|p| {
                p.move_to(Point::new(3., 20.));
                p.line_to(Point::new(3., 18.));
                p.bezier_curve_to(
                    Point::new(3., 11.),
                    Point::new(15., 11.),
                    Point::new(15., 18.),
                );
                p.line_to(Point::new(15., 20.));
            });
            f.stroke(&p, Stroke::default().with_color(c).with_width(1.8));
            line(f, &[(18., 14.), (21., 16.), (21., 20.)], c);
        }
        Glyph::Decks => {
            line(
                f,
                &[(8., 7.), (20., 7.), (20., 21.), (8., 21.), (8., 7.)],
                c,
            );
            line(f, &[(4., 17.), (3., 3.), (15., 2.), (15., 4.)], c);
            line(
                f,
                &[(12., 14.), (14., 11.), (17., 14.), (14., 17.), (12., 14.)],
                c,
            );
        }
        Glyph::Image => {
            line(
                f,
                &[(3., 3.), (21., 3.), (21., 21.), (3., 21.), (3., 3.)],
                c,
            );
            circle(f, 8., 8., 2., c);
            line(
                f,
                &[(3., 18.), (10., 12.), (14., 16.), (17., 13.), (21., 17.)],
                c,
            );
        }
        Glyph::Frame => {
            for points in [
                [(3., 9.), (3., 3.), (9., 3.)],
                [(15., 3.), (21., 3.), (21., 9.)],
                [(21., 15.), (21., 21.), (15., 21.)],
                [(9., 21.), (3., 21.), (3., 15.)],
            ] {
                line(f, &points, c);
            }
            circle(f, 12., 12., 2., c);
        }
        Glyph::Edit => {
            line(
                f,
                &[
                    (4., 16.),
                    (16., 4.),
                    (20., 8.),
                    (8., 20.),
                    (3., 21.),
                    (4., 16.),
                    (8., 20.),
                ],
                c,
            );
            line(f, &[(13., 7.), (17., 11.)], c);
        }
        Glyph::Delete => {
            line(f, &[(4., 6.), (20., 6.)], c);
            line(f, &[(9., 6.), (9., 3.), (15., 3.), (15., 6.)], c);
            line(f, &[(6., 6.), (7., 21.), (17., 21.), (18., 6.)], c);
            line(f, &[(10., 10.), (10., 17.)], c);
            line(f, &[(14., 10.), (14., 17.)], c);
        }
        Glyph::Heart => {
            let p = Path::new(|p| {
                p.move_to(Point::new(12., 21.));
                p.bezier_curve_to(
                    Point::new(-5., 10.),
                    Point::new(6., -3.),
                    Point::new(12., 7.),
                );
                p.bezier_curve_to(
                    Point::new(18., -3.),
                    Point::new(29., 10.),
                    Point::new(12., 21.),
                );
            });
            f.stroke(&p, Stroke::default().with_color(c).with_width(1.8));
        }
        Glyph::Poison => {
            line(
                f,
                &[
                    (9., 3.),
                    (15., 3.),
                    (15., 9.),
                    (21., 19.),
                    (20., 21.),
                    (4., 21.),
                    (3., 19.),
                    (9., 9.),
                    (9., 3.),
                ],
                c,
            );
            line(f, &[(7., 15.), (17., 15.)], c);
            circle(f, 12., 18., 0.7, c);
        }
        Glyph::Shield => {
            line(
                f,
                &[
                    (12., 2.),
                    (21., 6.),
                    (20., 15.),
                    (12., 22.),
                    (4., 15.),
                    (3., 6.),
                    (12., 2.),
                ],
                c,
            );
            line(f, &[(12., 7.), (12., 13.)], c);
            circle(f, 12., 17., 0.6, c);
        }
        Glyph::Trophy => {
            line(
                f,
                &[
                    (7., 3.),
                    (17., 3.),
                    (17., 11.),
                    (14., 15.),
                    (10., 15.),
                    (7., 11.),
                    (7., 3.),
                ],
                c,
            );
            line(f, &[(7., 5.), (3., 5.), (3., 10.), (7., 12.)], c);
            line(f, &[(17., 5.), (21., 5.), (21., 10.), (17., 12.)], c);
            line(f, &[(12., 15.), (12., 21.), (7., 21.), (17., 21.)], c);
        }
        Glyph::Rotate(clockwise) => {
            f.with_save(|f| {
                if !clockwise {
                    f.translate(iced::Vector::new(24., 0.));
                    f.scale_nonuniform(iced::Vector::new(-1., 1.));
                }
                let p = Path::new(|p| {
                    p.move_to(Point::new(19., 8.));
                    p.bezier_curve_to(
                        Point::new(12., -3.),
                        Point::new(-1., 7.),
                        Point::new(5., 17.),
                    );
                    p.bezier_curve_to(
                        Point::new(9., 23.),
                        Point::new(19., 21.),
                        Point::new(20., 14.),
                    );
                });
                f.stroke(&p, Stroke::default().with_color(c).with_width(1.8));
                line(f, &[(19., 2.), (19., 8.), (13., 8.)], c);
            });
        }
        Glyph::Shuffle => {
            line(
                f,
                &[(3., 6.), (7., 6.), (17., 18.), (21., 18.), (18., 15.)],
                c,
            );
            line(f, &[(18., 21.), (21., 18.)], c);
            line(
                f,
                &[(3., 18.), (7., 18.), (17., 6.), (21., 6.), (18., 3.)],
                c,
            );
            line(f, &[(18., 9.), (21., 6.)], c);
        }
        Glyph::Mana(symbol) => draw_mana(f, symbol, c),
    }
}

fn draw_mana(f: &mut Frame, symbol: char, ink: Color) {
    // Pale discs preserve mana recognition independently of the app theme.
    f.fill(
        &Path::circle(Point::new(12., 12.), 11.5),
        style::mana_color(symbol),
    );
    match symbol {
        'W' => {
            f.fill(&Path::circle(Point::new(12., 12.), 4.), ink);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.;
                line(
                    f,
                    &[
                        (12. + a.cos() * 6., 12. + a.sin() * 6.),
                        (12. + a.cos() * 8.5, 12. + a.sin() * 8.5),
                    ],
                    ink,
                );
            }
        }
        'U' => {
            let p = Path::new(|p| {
                p.move_to(Point::new(12., 3.5));
                p.bezier_curve_to(
                    Point::new(10., 7.),
                    Point::new(6., 11.),
                    Point::new(6., 14.),
                );
                p.bezier_curve_to(
                    Point::new(6., 22.),
                    Point::new(18., 22.),
                    Point::new(18., 14.),
                );
                p.bezier_curve_to(
                    Point::new(18., 11.),
                    Point::new(14., 7.),
                    Point::new(12., 3.5),
                );
                p.close();
            });
            f.fill(&p, ink);
            line(f, &[(9., 14.), (10., 16.)], style::mana_color('U'));
        }
        'B' => {
            f.fill(&Path::circle(Point::new(12., 10.5), 6.5), ink);
            f.fill_rectangle(Point::new(8., 13.), Size::new(8., 6.), ink);
            for x in [9., 15.] {
                f.fill(
                    &Path::circle(Point::new(x, 10.5), 2.),
                    style::mana_color('B'),
                );
            }
            for x in [10.5, 13.5] {
                line(f, &[(x, 17.), (x, 20.)], style::mana_color('B'));
            }
        }
        'R' => {
            let p = Path::new(|p| {
                p.move_to(Point::new(14., 3.));
                p.bezier_curve_to(Point::new(3., 7.), Point::new(17., 9.), Point::new(7., 13.));
                p.line_to(Point::new(6., 9.));
                p.bezier_curve_to(
                    Point::new(0., 20.),
                    Point::new(16., 24.),
                    Point::new(19., 15.),
                );
                p.bezier_curve_to(
                    Point::new(21., 10.),
                    Point::new(14., 8.),
                    Point::new(14., 3.),
                );
                p.close();
            });
            f.fill(&p, ink);
            let p = Path::new(|p| {
                p.move_to(Point::new(12., 12.));
                p.bezier_curve_to(
                    Point::new(6., 20.),
                    Point::new(17., 21.),
                    Point::new(12., 12.),
                );
                p.close();
            });
            f.fill(&p, style::mana_color('R'));
        }
        'G' => {
            for (x, y, r) in [(12., 7., 4.), (8., 11., 4.), (16., 11., 4.), (12., 12., 4.)] {
                f.fill(&Path::circle(Point::new(x, y), r), ink);
            }
            line(f, &[(12., 10.), (12., 20.), (8., 20.)], ink);
            line(f, &[(12., 20.), (16., 20.)], ink);
        }
        _ => line(
            f,
            &[(12., 4.), (20., 12.), (12., 20.), (4., 12.), (12., 4.)],
            ink,
        ),
    }
}
