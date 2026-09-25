//! Text that can face any direction, for seats on the far side of the table.
//!
//! iced 0.13 has no rotation for text widgets at all - no field, and
//! `Transformation` exposes only translate and scale, so a generic "rotate
//! this subtree" wrapper can't be built either. Canvas text is the way
//! through: when the current transform isn't a plain scale+translate,
//! `fill_text` falls back to tessellating the glyphs into filled paths,
//! which respect the full transform and are clipped properly.

use iced::widget::canvas::{self, Canvas, Geometry, Path, Text};
use iced::{alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use crate::layout::SeatOrientation;

/// Average glyph advance as a fraction of font size, for the UI font.
const AVERAGE_ADVANCE: f32 = 0.56;
use crate::icon::{self, Glyph};
use crate::style;

/// One line of a chip, with its own size.
#[derive(Debug, Clone)]
pub struct Line {
    pub content: String,
    pub size: f32,
    pub icon: Option<Glyph>,
    pub mana: Vec<char>,
    pub muted: bool,
}

impl Line {
    pub fn new(content: impl Into<String>, size: f32) -> Self {
        Self {
            content: content.into(),
            size,
            icon: None,
            mana: Vec::new(),
            muted: false,
        }
    }

    pub fn with_icon(mut self, icon: Glyph) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn secondary(mut self) -> Self {
        self.muted = true;
        self
    }
    pub fn mana(identity: &str) -> Self {
        let mut line = Self::new("", 20.0);
        line.mana = "WUBRG".chars().filter(|c| identity.contains(*c)).collect();
        if line.mana.is_empty() {
            line.mana.push('C');
        }
        line
    }

    /// Rough rendered width. Canvas text can't be measured before it's
    /// drawn, and this only has to size the chip behind it, so an average
    /// advance width for the UI font is close enough.
    fn width(&self) -> f32 {
        if !self.mana.is_empty() {
            return self.mana.len() as f32 * (self.size + 4.0) - 4.0;
        }
        self.content.chars().count() as f32 * self.size * AVERAGE_ADVANCE
            + if self.icon.is_some() { 28.0 } else { 0.0 }
    }

    fn height(&self) -> f32 {
        self.size * 1.25
    }
}

struct Chip<Msg> {
    lines: Vec<Line>,
    identity: bool,
    avoid: Option<Rectangle>,
    action: bool,
    facing: SeatOrientation,
    /// When set, the chip sits against the edge of the box nearest its
    /// player instead of dead centre.
    hug_edge: bool,
    /// Which end of that edge, in the player's own frame.
    align: EdgeAlign,
    padding: f32,
    background: Color,
    border: Color,
    radius: f32,
    /// `None` leaves the chip transparent to input, which is what lets taps
    /// fall straight through to the counter zones underneath it.
    on_press: Option<Msg>,
}

/// Move an identity label around the actual center control, staying inside
/// its tile. Prefer sliding along the edge before moving toward the counter.
fn clear_center(
    bounds: Size,
    footprint: Size,
    original: Point,
    avoid: Rectangle,
    facing: SeatOrientation,
) -> Option<Point> {
    let gap = if bounds.height < 400.0 { 4.0 } else { 12.0 };
    let avoid = Rectangle {
        x: avoid.x - gap,
        y: avoid.y - gap,
        width: avoid.width + gap * 2.0,
        height: avoid.height + gap * 2.0,
    };
    let rect = |p: Point| {
        Rectangle::new(
            Point::new(p.x - footprint.width / 2.0, p.y - footprint.height / 2.0),
            footprint,
        )
    };
    let counter_size = screen_footprint(Size::new(160.0, 147.0), facing);
    let counter = Rectangle::new(
        Point::new(
            (bounds.width - counter_size.width) / 2.0,
            (bounds.height - counter_size.height) / 2.0,
        ),
        counter_size,
    );
    let span = if facing.is_sideways() {
        bounds.height
    } else {
        bounds.width
    };
    let action_size = Size::new(((span - EDGE_MARGIN * 2.0) * 0.44).min(176.0), 108.0);
    let action_rects = [EdgeAlign::Start, EdgeAlign::End].map(|align| {
        let center = chip_center(bounds, action_size, facing, true, align);
        let footprint = screen_footprint(action_size, facing);
        Rectangle::new(
            Point::new(
                center.x - footprint.width / 2.0,
                center.y - footprint.height / 2.0,
            ),
            footprint,
        )
    });
    let clear = |r: Rectangle| {
        !r.intersects(&avoid)
            && !r.intersects(&counter)
            && action_rects.iter().all(|a| !r.intersects(a))
    };
    if clear(rect(original)) {
        return Some(original);
    }
    let xs = [
        original.x,
        counter.x - footprint.width / 2.0 - 1.0,
        counter.x + counter.width + footprint.width / 2.0 + 1.0,
        avoid.x - footprint.width / 2.0,
        avoid.x + avoid.width + footprint.width / 2.0,
    ];
    let ys = [
        original.y,
        counter.y - footprint.height / 2.0 - 1.0,
        counter.y + counter.height + footprint.height / 2.0 + 1.0,
        avoid.y - footprint.height / 2.0,
        avoid.y + avoid.height + footprint.height / 2.0,
    ];
    xs.into_iter()
        .flat_map(|x| ys.map(|y| Point::new(x, y)))
        .filter(|&p| {
            let r = rect(p);
            r.x >= 0.0
                && r.y >= 0.0
                && r.x + r.width <= bounds.width
                && r.y + r.height <= bounds.height
                && clear(r)
        })
        .min_by(|a, b| {
            let distance = |p: &Point| (p.x - original.x).powi(2) + (p.y - original.y).powi(2);
            distance(a).total_cmp(&distance(b))
        })
}

impl<Msg> Chip<Msg> {
    /// The chip's fitted lines, its own box, and where that box's centre
    /// lands. Drawing and hit-testing both go through this, so a tap can
    /// never land somewhere the chip isn't actually drawn.
    fn effective_padding(&self, bounds: Size) -> f32 {
        if self.identity && bounds.height < 400.0 {
            4.0
        } else {
            self.padding
        }
    }

    fn placement(&self, bounds: Size) -> (Vec<Line>, Size, Point) {
        self.placement_lines(bounds, &self.lines)
    }

    fn placement_lines(&self, bounds: Size, source: &[Line]) -> (Vec<Line>, Size, Point) {
        let padding = self.effective_padding(bounds);
        // A turned chip is limited by the tile's *other* dimension, since
        // its width runs across the tile's height.
        let span = if self.facing.is_sideways() {
            bounds.height
        } else {
            bounds.width
        };
        let mut available = (span - EDGE_MARGIN * 2.0 - padding * 2.0).max(0.0);
        if self.hug_edge && !self.identity {
            // Only chips sharing an edge are rationed. The life counter sits
            // in the middle of the tile on its own and gets the whole span,
            // which it needs - a three-digit total must never be truncated.
            available = available.min(
                ((if self.action {
                    (span - EDGE_MARGIN * 2.0).max(0.0)
                } else {
                    span
                }) * if self.action {
                    0.44
                } else {
                    max_share(self.align)
                } - padding * 2.0)
                    .max(0.0),
            );
        }

        if self.identity {
            available = available.min(260.0);
        }
        let narrow_head = self.identity && self.facing.is_sideways() && bounds.width < 320.0;
        if narrow_head {
            available = available.min(200.0);
        }
        let lines: Vec<Line> = source
            .iter()
            .map(|line| {
                let icon_width = if line.icon.is_some() { 28.0 } else { 0.0 };
                let text_space = (available - icon_width).max(0.0);
                let size = if !line.mana.is_empty() {
                    line.size
                        .min((available + 4.0) / line.mana.len() as f32 - 4.0)
                        .max(1.0)
                } else if self.action && !line.content.is_empty() {
                    line.size
                        .min(text_space / (line.content.chars().count() as f32 * AVERAGE_ADVANCE))
                        .max(12.0)
                } else if self.identity && !self.facing.is_sideways() && bounds.height < 400.0 {
                    line.size.min(18.0)
                } else {
                    line.size
                };
                Line {
                    content: fit_to_width(&line.content, size, text_space),
                    icon: line.icon,
                    mana: line.mana.clone(),
                    muted: line.muted,
                    size,
                }
            })
            .collect();

        let text_w = lines.iter().map(Line::width).fold(0.0_f32, f32::max);
        let text_h: f32 = lines.iter().map(Line::height).sum();
        let box_size = Size::new(
            if self.action {
                (available + padding * 2.0).min(176.0)
            } else {
                text_w + padding * 2.0
            },
            (text_h + padding * 2.0).max(if self.on_press.is_some() {
                if self.action {
                    108.0
                } else {
                    style::TOUCH_H
                }
            } else {
                0.0
            }),
        );
        let edge = if self.identity {
            match self.facing {
                SeatOrientation::Upright => SeatOrientation::UpsideDown,
                SeatOrientation::UpsideDown => SeatOrientation::Upright,
                SeatOrientation::LeftHead => SeatOrientation::RightHead,
                SeatOrientation::RightHead => SeatOrientation::LeftHead,
            }
        } else {
            self.facing
        };
        let mut center = chip_center(bounds, box_size, edge, self.hug_edge, self.align);

        if narrow_head {
            center.y = bounds.height
                * if reads_forward(self.facing) {
                    0.25
                } else {
                    0.75
                };
        }
        if let Some(avoid) = self.avoid {
            if let Some(clear) = clear_center(
                bounds,
                screen_footprint(box_size, self.facing),
                center,
                avoid,
                self.facing,
            ) {
                center = clear;
            } else if source.len() > 1 {
                // Keep the player's name readable when a compact seat cannot
                // accommodate secondary details between the timer and controls.
                return self.placement_lines(bounds, &source[..source.len() - 1]);
            } else if self.identity && source[0].content.chars().count() > 4 {
                // Preserve readable type in dense layouts: shorten the name
                // until its label fits beside the center disc and counters.
                let mut short = source[0].clone();
                short.content = format!(
                    "{}…",
                    short
                        .content
                        .chars()
                        .take(short.content.chars().count() - 2)
                        .collect::<String>()
                );
                return self.placement_lines(bounds, &[short]);
            }
        }
        (lines, box_size, center)
    }

    /// The chip's axis-aligned footprint on screen. A quarter turn swaps the
    /// box's sides, so this is the turned bounding box - exactly the
    /// rectangle a finger has to land in.
    fn hit_rect(&self, bounds: Size) -> Rectangle {
        let (_, box_size, center) = self.placement(bounds);
        let footprint = screen_footprint(box_size, self.facing);
        Rectangle::new(
            Point::new(
                center.x - footprint.width / 2.0,
                center.y - footprint.height / 2.0,
            ),
            footprint,
        )
    }
}

/// Margin between a hugging chip and the edge of the tile.
const EDGE_MARGIN: f32 = 16.0;

/// Where a press landed, in canvas-local coordinates, from either a mouse
/// click or a finger.
///
/// Touch carries its own position and has to be matched explicitly: on a
/// touchscreen the mouse cursor is often unavailable, so a mouse-only match
/// silently does nothing. That is exactly how the first version of these
/// chips shipped - they drew perfectly and ignored every tap. iced's own
/// `button` and `mouse_area` both match the two together for this reason.
fn press_position(
    event: &canvas::Event,
    bounds: Rectangle,
    cursor: iced::mouse::Cursor,
) -> Option<Point> {
    match event {
        canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
            cursor.position_in(bounds)
        }
        canvas::Event::Touch(iced::touch::Event::FingerPressed { position, .. }) => bounds
            .contains(*position)
            .then(|| Point::new(position.x - bounds.x, position.y - bounds.y)),
        _ => None,
    }
}

/// Where along the player's edge a chip sits, in *their* frame: `Start` is
/// their left hand, `End` their right. Which way that runs on screen depends
/// on which side of the table they're sitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeAlign {
    Start,
    Center,
    End,
}

/// The chip's footprint on screen once turned. A quarter turn swaps its
/// width and height, which is what decides how far it can sit from the
/// middle before a corner leaves the tile.
fn screen_footprint(box_size: Size, facing: SeatOrientation) -> Size {
    if facing.is_sideways() {
        Size::new(box_size.height, box_size.width)
    } else {
        box_size
    }
}

/// Shortens `content` until its estimated width fits `available`, ending it
/// with an ellipsis. Canvas text is drawn with unbounded width - it never
/// wraps and never truncates itself - so a long label would otherwise run
/// straight off the tile.
fn fit_to_width(content: &str, size: f32, available: f32) -> String {
    let advance = size * AVERAGE_ADVANCE;
    if advance <= 0.0 {
        return content.to_string();
    }
    let max_chars = (available / advance).floor() as usize;
    if content.chars().count() <= max_chars {
        return content.to_string();
    }
    if max_chars <= 3 {
        return content.chars().take(max_chars).collect();
    }
    let kept: String = content.chars().take(max_chars - 3).collect();
    format!("{}...", kept.trim_end())
}

/// How much of the player's edge a chip may claim, by where it sits. On the
/// active seat three chips share that edge, so none of them may grow into
/// the others' room however long a player's name or commander is. The
/// fractions leave a little slack over 1.0 for the gaps between them.
fn max_share(align: EdgeAlign) -> f32 {
    match align {
        EdgeAlign::Center => 0.36,
        EdgeAlign::Start | EdgeAlign::End => 0.28,
    }
}

/// Does the player's left-to-right run the same way as the screen axis their
/// edge lies along? Upright reads along +x and the left-head seat reads down
/// +y; the other two read back against their axis.
fn reads_forward(facing: SeatOrientation) -> bool {
    matches!(facing, SeatOrientation::Upright | SeatOrientation::LeftHead)
}

/// The chip's centre *along* its edge. A chip too big to sit off to one side
/// collapses back to the middle rather than hanging off the end of the tile.
fn along_edge(bounds: Size, footprint: Size, facing: SeatOrientation, align: EdgeAlign) -> f32 {
    let (span, extent) = if facing.is_sideways() {
        (bounds.height, footprint.height)
    } else {
        (bounds.width, footprint.width)
    };
    let middle = span / 2.0;
    if align == EdgeAlign::Center {
        return middle;
    }
    let low = (extent / 2.0 + EDGE_MARGIN).min(middle);
    let high = (span - extent / 2.0 - EDGE_MARGIN).max(middle);
    if (align == EdgeAlign::Start) == reads_forward(facing) {
        low
    } else {
        high
    }
}

/// Where the chip's centre goes: the middle of the tile, or hard against
/// the edge the player is sitting at, at one end of it or the other.
fn chip_center(
    bounds: Size,
    box_size: Size,
    facing: SeatOrientation,
    hug_edge: bool,
    align: EdgeAlign,
) -> Point {
    let mut center = Point::new(bounds.width / 2.0, bounds.height / 2.0);
    if !hug_edge {
        return center;
    }
    let footprint = screen_footprint(box_size, facing);
    match facing {
        SeatOrientation::Upright => {
            center.y = bounds.height - footprint.height / 2.0 - EDGE_MARGIN;
        }
        SeatOrientation::UpsideDown => {
            center.y = footprint.height / 2.0 + EDGE_MARGIN;
        }
        SeatOrientation::LeftHead => {
            center.x = footprint.width / 2.0 + EDGE_MARGIN;
        }
        SeatOrientation::RightHead => {
            center.x = bounds.width - footprint.width / 2.0 - EDGE_MARGIN;
        }
    }
    let at = along_edge(bounds, footprint, facing, align);
    if facing.is_sideways() {
        center.y = at;
    } else {
        center.x = at;
    }
    center
}

impl<Msg: Clone> canvas::Program<Msg> for Chip<Msg> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> (canvas::event::Status, Option<Msg>) {
        let Some(message) = self.on_press.clone() else {
            return (canvas::event::Status::Ignored, None);
        };
        let Some(position) = press_position(&event, bounds, cursor) else {
            return (canvas::event::Status::Ignored, None);
        };
        // Only a tap on the chip itself counts. Everything else stays
        // ignored so it reaches whatever is stacked below.
        if self.hit_rect(bounds.size()).contains(position) {
            (canvas::event::Status::Captured, Some(message))
        } else {
            (canvas::event::Status::Ignored, None)
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        let (lines, box_size, center) = self.placement(bounds.size());
        let text_h: f32 = lines.iter().map(Line::height).sum();

        frame.with_save(|frame| {
            // Work from the chip's own centre outwards, so the rotation
            // pivots on the chip rather than swinging it across the tile.
            frame.translate(iced::Vector::new(center.x, center.y));
            frame.rotate(iced::Radians(self.facing.radians()));

            let top_left = Point::new(-box_size.width / 2.0, -box_size.height / 2.0);
            let chip = Path::rounded_rectangle(top_left, box_size, self.radius.into());
            let hovered = self.on_press.is_some()
                && cursor
                    .position_in(bounds)
                    .is_some_and(|p| self.hit_rect(bounds.size()).contains(p));
            frame.fill(
                &chip,
                if hovered {
                    style::ACCENT_DEEP
                } else {
                    self.background
                },
            );
            frame.stroke(
                &chip,
                canvas::Stroke {
                    style: canvas::Style::Solid(self.border),
                    width: 1.0,
                    ..Default::default()
                },
            );

            let mut y = -text_h / 2.0;
            for (line_index, line) in lines.iter().enumerate() {
                for (i, symbol) in line.mana.iter().enumerate() {
                    frame.with_save(|frame| {
                        frame.translate(iced::Vector::new(
                            (if self.identity {
                                -box_size.width / 2.0 + self.effective_padding(bounds.size())
                            } else {
                                -line.width() / 2.0
                            }) + i as f32 * (line.size + 4.0),
                            y,
                        ));
                        frame.scale(line.size / 24.0);
                        icon::draw(frame, Glyph::Mana(*symbol), style::MANA_INK);
                    });
                }
                if let Some(glyph) = line.icon {
                    frame.with_save(|frame| {
                        frame.translate(iced::Vector::new(
                            -line.width() / 2.0,
                            y + line.height() / 2.0 - 10.0,
                        ));
                        frame.scale(20.0_f32 / 24.0);
                        icon::draw(frame, glyph, style::ACCENT_BRIGHT);
                    });
                }
                frame.fill_text(Text {
                    content: line.content.clone(),
                    position: Point::new(
                        if self.identity {
                            -box_size.width / 2.0 + self.effective_padding(bounds.size())
                        } else if line.icon.is_some() {
                            14.0
                        } else {
                            0.0
                        },
                        y + line.height() / 2.0,
                    ),
                    color: if line.muted {
                        style::TEXT_MUTED
                    } else {
                        style::TEXT
                    },
                    size: line.size.into(),
                    font: iced::Font {
                        weight: if self.identity && line_index == 0 {
                            iced::font::Weight::Semibold
                        } else {
                            iced::font::Weight::Normal
                        },
                        ..iced::Font::DEFAULT
                    },
                    horizontal_alignment: if self.identity {
                        alignment::Horizontal::Left
                    } else {
                        alignment::Horizontal::Center
                    },
                    vertical_alignment: alignment::Vertical::Center,
                    ..Text::default()
                });
                y += line.height();
            }
        });

        vec![frame.into_geometry()]
    }
}

/// Every chip is a canvas filling its whole tile that places itself inside
/// those bounds - it can't be squeezed by a fixed-size strip and clipped
/// when turned, and several can stack on one tile without fighting for room.
fn chip_canvas<'a, Msg: Clone + 'a>(
    lines: Vec<Line>,
    facing: SeatOrientation,
    hug_edge: bool,
    align: EdgeAlign,
    padding: f32,
    paint: style::ChipPaint,
    on_press: Option<Msg>,
) -> Element<'a, Msg> {
    Canvas::new(Chip {
        lines,
        identity: false,
        avoid: None,
        action: false,
        facing,
        hug_edge,
        align,
        padding,
        background: paint.background,
        border: paint.border,
        radius: paint.radius,
        on_press,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// A chip that can be tapped, sitting at one end of the player's own edge
/// and turned to face them. This is the only way to put a control on a seat
/// tile that reads the right way up for the person it belongs to - iced 0.13
/// can't rotate a real button, so it has to be drawn and hit-tested by hand.
pub fn edge_button<'a, Msg: Clone + 'a>(
    lines: Vec<Line>,
    facing: SeatOrientation,
    align: EdgeAlign,
    paint: style::ChipPaint,
    on_press: Msg,
) -> Element<'a, Msg> {
    chip_canvas(lines, facing, true, align, 16.0, paint, Some(on_press))
}

/// Identity owns the opposite edge so names never compete with action buttons.
pub fn identity_chip<'a, Msg: Clone + 'a>(
    lines: Vec<Line>,
    facing: SeatOrientation,
    avoid: Rectangle,
) -> Element<'a, Msg> {
    Canvas::new(Chip {
        lines,
        identity: true,
        avoid: Some(avoid),
        action: false,
        facing,
        hug_edge: true,
        align: EdgeAlign::Center,
        padding: 12.0,
        background: Color {
            a: 0.94,
            ..style::SURFACE_0
        },
        border: style::HAIRLINE,
        radius: style::R_MD,
        on_press: None,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

pub fn action_button<'a, Msg: Clone + 'a>(
    glyph: Glyph,
    title: &str,
    subtitle: &str,
    facing: SeatOrientation,
    align: EdgeAlign,
    primary: bool,
    message: Msg,
) -> Element<'a, Msg> {
    let mut lines = vec![Line::new("", 22.0).with_icon(glyph), Line::new(title, 20.0)];
    if !subtitle.is_empty() {
        lines.push(Line::new(subtitle, 14.0).secondary());
    }
    Canvas::new(Chip {
        lines,
        identity: false,
        avoid: None,
        action: true,
        facing,
        hug_edge: true,
        align,
        padding: 12.0,
        background: if primary {
            style::ACCENT_DEEP
        } else {
            style::SURFACE_1
        },
        border: if primary {
            style::ACCENT_BRIGHT
        } else {
            style::SURFACE_3
        },
        radius: style::R_MD,
        on_press: Some(message),
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

// --- The seat action menu --------------------------------------------------

/// A compact grid keeps every action inside the player's tile, including
/// eight-player boards. Drawing and touch use the same local rectangles.
struct Menu<Msg> {
    items: Vec<(Glyph, String, Msg)>,
    facing: SeatOrientation,
}

impl<Msg> Menu<Msg> {
    fn layout(&self, bounds: Size) -> Vec<Rectangle> {
        let local = if self.facing.is_sideways() {
            Size::new(bounds.height, bounds.width)
        } else {
            bounds
        };
        let columns = if local.width >= 260.0 { 2 } else { 1 };
        let rows = self.items.len().div_ceil(columns);
        let gap = 8.0;
        let width = ((local.width - 32.0 - gap * (columns - 1) as f32) / columns as f32)
            .max(0.0)
            .min(240.0);
        let height = ((local.height - 32.0 - gap * rows.saturating_sub(1) as f32)
            / rows.max(1) as f32)
            .clamp(0.0, 88.0);
        let total_w = columns as f32 * width + (columns - 1) as f32 * gap;
        let total_h = rows as f32 * height + rows.saturating_sub(1) as f32 * gap;
        (0..self.items.len())
            .map(|i| {
                Rectangle::new(
                    Point::new(
                        -total_w / 2.0 + (i % columns) as f32 * (width + gap),
                        -total_h / 2.0 + (i / columns) as f32 * (height + gap),
                    ),
                    Size::new(width, height),
                )
            })
            .collect()
    }

    fn screen_rect(&self, bounds: Size, rect: Rectangle) -> Rectangle {
        let p = rect.center();
        let (sin, cos) = self.facing.radians().sin_cos();
        let center = Point::new(
            bounds.width / 2.0 + p.x * cos - p.y * sin,
            bounds.height / 2.0 + p.x * sin + p.y * cos,
        );
        let size = screen_footprint(rect.size(), self.facing);
        Rectangle::new(
            Point::new(center.x - size.width / 2.0, center.y - size.height / 2.0),
            size,
        )
    }
}

impl<Msg: Clone> canvas::Program<Msg> for Menu<Msg> {
    type State = ();
    fn update(
        &self,
        _: &mut (),
        event: canvas::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> (canvas::event::Status, Option<Msg>) {
        let Some(position) = press_position(&event, bounds, cursor) else {
            return (canvas::event::Status::Ignored, None);
        };
        for ((_, _, message), rect) in self.items.iter().zip(self.layout(bounds.size())) {
            if self.screen_rect(bounds.size(), rect).contains(position) {
                return (canvas::event::Status::Captured, Some(message.clone()));
            }
        }
        (canvas::event::Status::Captured, None)
    }
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.with_save(|frame| {
            frame.translate(iced::Vector::new(bounds.width / 2.0, bounds.height / 2.0));
            frame.rotate(iced::Radians(self.facing.radians()));
            for ((glyph, label, _), rect) in self.items.iter().zip(self.layout(bounds.size())) {
                let hovered = cursor
                    .position_in(bounds)
                    .is_some_and(|p| self.screen_rect(bounds.size(), rect).contains(p));
                let path =
                    Path::rounded_rectangle(rect.position(), rect.size(), style::R_MD.into());
                frame.fill(
                    &path,
                    if hovered {
                        style::ACCENT_DEEP
                    } else {
                        style::SURFACE_2
                    },
                );
                frame.stroke(
                    &path,
                    canvas::Stroke::default()
                        .with_color(if hovered {
                            style::ACCENT_BRIGHT
                        } else {
                            style::SURFACE_3
                        })
                        .with_width(1.0),
                );
                let center = rect.center();
                frame.with_save(|frame| {
                    frame.translate(iced::Vector::new(center.x - 12.0, center.y - 27.0));
                    icon::draw(frame, *glyph, style::ACCENT_BRIGHT);
                });
                frame.fill_text(Text {
                    content: fit_to_width(label, 18.0, rect.width - 16.0),
                    position: Point::new(center.x, center.y + 17.0),
                    color: style::TEXT,
                    size: 18.0.into(),
                    horizontal_alignment: alignment::Horizontal::Center,
                    vertical_alignment: alignment::Vertical::Center,
                    ..Text::default()
                });
            }
        });
        vec![frame.into_geometry()]
    }
}

/// The swipe seat menu, turned to face its player.
pub fn menu<'a, Msg: Clone + 'a>(
    items: Vec<(Glyph, String, Msg)>,
    facing: SeatOrientation,
) -> Element<'a, Msg> {
    Canvas::new(Menu { items, facing })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The heavier chip used for the big life number, which sits in the middle
/// of the tile directly on the art and has to stay readable over it.
pub fn strong_chip<'a, Msg: Clone + 'a>(
    lines: Vec<Line>,
    facing: SeatOrientation,
) -> Element<'a, Msg> {
    chip_canvas(
        lines,
        facing,
        false,
        EdgeAlign::Center,
        26.0,
        style::glass_strong_paint(),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mana_keeps_standard_order_and_colorless_symbol() {
        assert_eq!(Line::mana("GRWUG").mana, vec!['W', 'U', 'R', 'G']);
        assert_eq!(Line::mana("").mana, vec!['C']);
    }

    #[test]
    fn names_clear_center_controls_in_every_supported_layout() {
        for screen in [
            Size::new(1875.0, 1205.0),
            Size::new(1280.0, 800.0),
            Size::new(800.0, 1280.0),
        ] {
            let board = Size::new(screen.width - 32.0, screen.height - 32.0);
            for players in 2..=8 {
                for layout in crate::layout::options_for(players) {
                    for control in [Size::new(
                        crate::screens::game::center::diameter(screen),
                        crate::screens::game::center::diameter(screen),
                    )] {
                        for seat in 0..players {
                            let tile = layout.seat_bounds(seat, board);
                            let size = Size::new(tile.width - 6.0, tile.height - 6.0);
                            let avoid = Rectangle {
                                x: (board.width - control.width) / 2.0 - tile.x - 3.0,
                                y: (board.height - control.height) / 2.0 - tile.y - 3.0,
                                width: control.width,
                                height: control.height,
                            };
                            for (name, context) in [
                                ("Ben", None),
                                ("Alexandria With A Very Long Player Name", None),
                                ("Ben", Some("Poison (lethal at 10)")),
                                (
                                    "Alexandria With A Very Long Player Name",
                                    Some("Commander damage to another player"),
                                ),
                            ] {
                                let mut chip = chip_at(
                                    vec![
                                        Line::new(name, style::T_PLAYER_NAME as f32),
                                        Line::mana("WUBRG"),
                                    ],
                                    layout.seat_orientation(seat),
                                    EdgeAlign::Center,
                                );
                                if let Some(context) = context {
                                    chip.lines.insert(1, Line::new(context, 16.0).secondary());
                                }
                                chip.identity = true;
                                chip.padding = 12.0;
                                chip.avoid = Some(avoid);
                                let rect = chip.hit_rect(size);
                                let mut counter = chip_at(
                                    vec![Line::new("40", 76.0)],
                                    layout.seat_orientation(seat),
                                    EdgeAlign::Center,
                                );
                                counter.hug_edge = false;
                                counter.padding = 26.0;
                                for align in [EdgeAlign::Start, EdgeAlign::End] {
                                    let mut action = chip_at(
                                        vec![
                                            Line::new("", 22.0).with_icon(Glyph::Shield),
                                            Line::new("Commander hate", 20.0),
                                        ],
                                        layout.seat_orientation(seat),
                                        align,
                                    );
                                    action.action = true;
                                    action.padding = 12.0;
                                    action.on_press = Some(());
                                    assert!(
                                        !overlaps(rect, action.hit_rect(size)),
                                        "name hits action: {players} {} seat {seat} {screen:?}",
                                        layout.name
                                    );
                                }
                                assert!(!overlaps(rect, counter.hit_rect(size)), "name hits counter: {players} {} seat {seat} {screen:?} {control:?}", layout.name);
                                assert!(!overlaps(rect, avoid), "{players} {} seat {seat} {screen:?} {control:?}: {rect:?} vs {avoid:?}", layout.name);
                                assert!(
                                    rect.x >= -0.01
                                        && rect.y >= -0.01
                                        && rect.x + rect.width <= size.width + 0.01
                                        && rect.y + rect.height <= size.height + 0.01
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn identity_and_action_buttons_fit_separate_edges() {
        for facing in ALL {
            for size in [
                Size::new(900.0, 580.0),
                Size::new(360.0, 580.0),
                Size::new(360.0, 1160.0),
                Size::new(240.0, 800.0),
            ] {
                let mut identity = chip_at(
                    vec![
                        Line::new("A long player name", 22.0),
                        Line::new("A commander with a very long name", 16.0),
                        Line::mana("WUBRG"),
                    ],
                    facing,
                    EdgeAlign::Center,
                );
                identity.identity = true;
                identity.padding = 8.0;
                let mut start = chip_at(
                    vec![
                        Line::new("", 22.0).with_icon(Glyph::Shield),
                        Line::new("Commander hate", 20.0),
                    ],
                    facing,
                    EdgeAlign::Start,
                );
                start.action = true;
                start.on_press = Some(());
                start.padding = 12.0;
                let mut end = chip_at(start.lines.clone(), facing, EdgeAlign::End);
                end.action = true;
                end.on_press = Some(());
                end.padding = 12.0;
                let mut counter = chip_at(vec![Line::new("40", 76.0)], facing, EdgeAlign::Center);
                counter.hug_edge = false;
                counter.padding = 26.0;
                let rects = [
                    identity.hit_rect(size),
                    start.hit_rect(size),
                    end.hit_rect(size),
                    counter.hit_rect(size),
                ];
                for (i, rect) in rects.iter().enumerate() {
                    assert!(rect.x >= 0.0 && rect.y >= 0.0);
                    assert!(rect.x + rect.width <= size.width + 0.01);
                    assert!(rect.y + rect.height <= size.height + 0.01);
                    for other in &rects[i + 1..] {
                        assert!(!overlaps(*rect, *other), "{facing:?} {size:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn menu_actions_fit_and_touch_the_correct_item_in_every_orientation() {
        use iced::widget::canvas::Program;
        for facing in ALL {
            for size in [
                Size::new(450.0, 560.0),
                Size::new(300.0, 360.0),
                Size::new(310.0, 1100.0),
            ] {
                let menu = Menu {
                    items: (0..5)
                        .map(|i| (Glyph::Heart, format!("Action {i}"), i))
                        .collect(),
                    facing,
                };
                let bounds = Rectangle::new(Point::new(100.0, 80.0), size);
                let rects: Vec<_> = menu
                    .layout(size)
                    .into_iter()
                    .map(|r| menu.screen_rect(size, r))
                    .collect();
                for (i, rect) in rects.iter().enumerate() {
                    assert!(rect.x >= -0.01 && rect.y >= -0.01, "{facing:?}: {rect:?}");
                    assert!(rect.x + rect.width <= size.width + 0.01);
                    assert!(rect.y + rect.height <= size.height + 0.01);
                    for other in &rects[i + 1..] {
                        assert!(!overlaps(*rect, *other));
                    }
                    let center = rect.center();
                    let event = canvas::Event::Touch(iced::touch::Event::FingerPressed {
                        id: iced::touch::Finger(1),
                        position: Point::new(bounds.x + center.x, bounds.y + center.y),
                    });
                    let (status, action) =
                        menu.update(&mut (), event, bounds, iced::mouse::Cursor::Unavailable);
                    assert_eq!(status, canvas::event::Status::Captured);
                    assert_eq!(action, Some(i));
                }
            }
        }
    }

    const TILE: Size = Size {
        width: 600.0,
        height: 400.0,
    };
    /// A caption chip: much wider than it is tall, which is what makes a
    /// quarter turn dangerous.
    const CHIP: Size = Size {
        width: 240.0,
        height: 90.0,
    };

    const ALL: [SeatOrientation; 4] = [
        SeatOrientation::Upright,
        SeatOrientation::UpsideDown,
        SeatOrientation::LeftHead,
        SeatOrientation::RightHead,
    ];

    /// A turned chip must stay inside its own tile. A wide caption rotated a
    /// quarter turn is 240px *tall*, so placing it by its unturned height
    /// pushes it off the tile and the canvas clips it away.
    #[test]
    fn a_hugging_chip_stays_inside_its_tile() {
        for facing in ALL {
            let center = chip_center(TILE, CHIP, facing, true, EdgeAlign::Center);
            let footprint = screen_footprint(CHIP, facing);
            let left = center.x - footprint.width / 2.0;
            let top = center.y - footprint.height / 2.0;
            assert!(left >= -0.01, "{facing:?} overflows left: {left}");
            assert!(top >= -0.01, "{facing:?} overflows top: {top}");
            assert!(
                left + footprint.width <= TILE.width + 0.01,
                "{facing:?} overflows right"
            );
            assert!(
                top + footprint.height <= TILE.height + 0.01,
                "{facing:?} overflows bottom"
            );
        }
    }

    /// Each chip hugs the edge its own player is sitting at, and no two
    /// orientations end up in the same place.
    #[test]
    fn each_orientation_hugs_its_own_edge() {
        let up = chip_center(
            TILE,
            CHIP,
            SeatOrientation::Upright,
            true,
            EdgeAlign::Center,
        );
        let down = chip_center(
            TILE,
            CHIP,
            SeatOrientation::UpsideDown,
            true,
            EdgeAlign::Center,
        );
        let left = chip_center(
            TILE,
            CHIP,
            SeatOrientation::LeftHead,
            true,
            EdgeAlign::Center,
        );
        let right = chip_center(
            TILE,
            CHIP,
            SeatOrientation::RightHead,
            true,
            EdgeAlign::Center,
        );

        assert!(up.y > TILE.height / 2.0, "upright should sit low");
        assert!(down.y < TILE.height / 2.0, "upside down should sit high");
        assert!(left.x < TILE.width / 2.0, "left head should sit left");
        assert!(right.x > TILE.width / 2.0, "right head should sit right");
    }

    /// A real chip, so the assertions run through fitting and rationing
    /// rather than a synthetic box that skips both.
    fn chip_at(lines: Vec<Line>, facing: SeatOrientation, align: EdgeAlign) -> Chip<()> {
        Chip {
            lines,
            identity: false,
            avoid: None,
            action: false,
            facing,
            hug_edge: true,
            align,
            padding: 16.0,
            background: Color::BLACK,
            border: Color::WHITE,
            radius: 20.0,
            on_press: None,
        }
    }

    fn overlaps(a: Rectangle, b: Rectangle) -> bool {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    }

    /// The regression this whole alignment exists for: the hate button used
    /// to be pinned to the tile's bottom-right in *screen* space, which put
    /// it straight on top of the name chip for every upright seat. Three
    /// chips sharing one edge have to keep out of each other's way.
    #[test]
    fn chips_on_the_same_edge_do_not_overlap() {
        // Deliberately overlong: the name chip is the one that grows, and
        // it is what used to run into everything else.
        let long = "Kalamax, the Stormsire \u{00b7} lethal at 21 commander damage";
        for facing in ALL {
            let start = chip_at(
                vec![Line::new("COMMANDER HATE", 16.0)],
                facing,
                EdgeAlign::Start,
            )
            .hit_rect(TILE);
            let middle = chip_at(
                vec![Line::new(long, 26.0), Line::new(long, 16.0)],
                facing,
                EdgeAlign::Center,
            )
            .hit_rect(TILE);
            let end = chip_at(
                vec![Line::new("END TURN", 20.0), Line::new("Turn 12", 16.0)],
                facing,
                EdgeAlign::End,
            )
            .hit_rect(TILE);
            assert!(
                !overlaps(start, middle),
                "{facing:?}: start overlaps centre"
            );
            assert!(!overlaps(middle, end), "{facing:?}: centre overlaps end");
            assert!(!overlaps(start, end), "{facing:?}: start overlaps end");
        }
    }

    /// Start is the player's left hand and End their right, whichever way
    /// their tile is turned - so the two are never in the same place, and
    /// they swap sides on screen for the seats facing the other way.
    #[test]
    fn start_and_end_follow_the_player_not_the_screen() {
        let upright_start =
            chip_center(TILE, CHIP, SeatOrientation::Upright, true, EdgeAlign::Start);
        let upright_end = chip_center(TILE, CHIP, SeatOrientation::Upright, true, EdgeAlign::End);
        assert!(
            upright_start.x < upright_end.x,
            "upright reads left to right"
        );

        let flipped_start = chip_center(
            TILE,
            CHIP,
            SeatOrientation::UpsideDown,
            true,
            EdgeAlign::Start,
        );
        let flipped_end = chip_center(
            TILE,
            CHIP,
            SeatOrientation::UpsideDown,
            true,
            EdgeAlign::End,
        );
        assert!(
            flipped_start.x > flipped_end.x,
            "upside down reads right to left on screen"
        );

        let left_start = chip_center(
            TILE,
            CHIP,
            SeatOrientation::LeftHead,
            true,
            EdgeAlign::Start,
        );
        let left_end = chip_center(TILE, CHIP, SeatOrientation::LeftHead, true, EdgeAlign::End);
        assert!(left_start.y < left_end.y, "left-head reads down the screen");

        let right_start = chip_center(
            TILE,
            CHIP,
            SeatOrientation::RightHead,
            true,
            EdgeAlign::Start,
        );
        let right_end = chip_center(TILE, CHIP, SeatOrientation::RightHead, true, EdgeAlign::End);
        assert!(
            right_start.y > right_end.y,
            "right-head reads up the screen"
        );
    }

    /// A chip too wide to sit off to one side collapses to the middle
    /// instead of hanging off the end of the tile.
    #[test]
    fn an_oversized_chip_stays_on_the_tile() {
        let huge = Size::new(TILE.width * 2.0, 90.0);
        for align in [EdgeAlign::Start, EdgeAlign::Center, EdgeAlign::End] {
            let center = chip_center(TILE, huge, SeatOrientation::Upright, true, align);
            assert_eq!(
                center.x,
                TILE.width / 2.0,
                "{align:?} should collapse to centre"
            );
        }
    }

    /// The life counter is never hugged to an edge - it belongs in the
    /// middle of the tile whichever way it faces.
    #[test]
    fn a_centred_chip_ignores_facing() {
        for facing in ALL {
            let center = chip_center(TILE, CHIP, facing, false, EdgeAlign::Center);
            assert_eq!(center.x, TILE.width / 2.0);
            assert_eq!(center.y, TILE.height / 2.0);
        }
    }
}

#[cfg(test)]
mod fitting_tests {
    use super::*;

    /// The commander-damage subtitle is long enough to run off a tile, so
    /// it has to come back shortened rather than overflowing.
    #[test]
    fn long_labels_are_shortened_to_fit() {
        let label = "Reyhan, Last of the Abzan \u{00b7} to Will (lethal at 21)";
        let fitted = fit_to_width(label, 16.0, 200.0);
        assert!(fitted.len() < label.len(), "should have been shortened");
        assert!(fitted.ends_with("..."), "got {fitted:?}");
        assert!(
            Line::new(fitted.clone(), 16.0).width() <= 200.0,
            "still too wide: {fitted:?}"
        );
    }

    /// Anything that already fits is left exactly as it is.
    #[test]
    fn short_labels_are_untouched() {
        assert_eq!(fit_to_width("Tanner", 26.0, 600.0), "Tanner");
        assert_eq!(fit_to_width("40", 76.0, 600.0), "40");
    }

    /// A box too small for even an ellipsis must not panic or underflow.
    #[test]
    fn a_tiny_box_does_not_panic() {
        for available in [0.0, 1.0, 5.0, 20.0] {
            let out = fit_to_width("Winota, Joiner of Forces", 26.0, available);
            assert!(out.chars().count() <= 24);
        }
    }
}

/// Player-facing outcome content. Drawing and hit testing share player-local bounds.
pub struct OutcomeAction<Msg> {
    pub label: String,
    pub message: Option<Msg>,
    pub selected: bool,
}

struct Outcome<Msg> {
    lines: Vec<Line>,
    qr: Option<iced::widget::image::Handle>,
    actions: Vec<OutcomeAction<Msg>>,
    facing: SeatOrientation,
}

impl<Msg> Outcome<Msg> {
    fn layout(&self, bounds: Size) -> (Size, f32, Vec<Rectangle>) {
        let available = screen_footprint(bounds, self.facing);
        let width = available.width.min(800.0).max(1.0);
        let columns = if self.actions.len() > 2 { 2 } else { 1 };
        let rows = self.actions.len().div_ceil(columns);
        let header: f32 = self.lines.iter().map(|l| l.height() + 8.0).sum();
        let top = 16.0 + header + if self.qr.is_some() { 184.0 } else { 0.0 };
        let height = top + rows as f32 * 80.0 + 8.0;
        let scale = (available.height / height).min(1.0);
        let cell = (width - 32.0 - (columns - 1) as f32 * 8.0) / columns as f32;
        let rects = (0..self.actions.len())
            .map(|i| {
                Rectangle::new(
                    Point::new(
                        -width / 2.0 + 16.0 + (i % columns) as f32 * (cell + 8.0),
                        -height / 2.0 + top + (i / columns) as f32 * 80.0,
                    ),
                    Size::new(cell, 72.0),
                )
            })
            .collect();
        (Size::new(width, height), scale, rects)
    }
    fn local_point(&self, bounds: Size, p: Point) -> Point {
        let (_, scale, _) = self.layout(bounds);
        let (sin, cos) = self.facing.radians().sin_cos();
        let x = p.x - bounds.width / 2.0;
        let y = p.y - bounds.height / 2.0;
        Point::new((x * cos + y * sin) / scale, (-x * sin + y * cos) / scale)
    }
}

impl<Msg: Clone> canvas::Program<Msg> for Outcome<Msg> {
    type State = ();
    fn update(
        &self,
        _: &mut (),
        event: canvas::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> (canvas::event::Status, Option<Msg>) {
        let Some(p) = press_position(&event, bounds, cursor) else {
            return (canvas::event::Status::Ignored, None);
        };
        let p = self.local_point(bounds.size(), p);
        let (_, _, rects) = self.layout(bounds.size());
        let message =
            self.actions.iter().zip(rects).find_map(|(action, rect)| {
                rect.contains(p).then(|| action.message.clone()).flatten()
            });
        (canvas::event::Status::Captured, message)
    }
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (size, scale, rects) = self.layout(bounds.size());
        let hovered = cursor
            .position_in(bounds)
            .map(|p| self.local_point(bounds.size(), p));
        frame.translate(iced::Vector::new(bounds.width / 2.0, bounds.height / 2.0));
        frame.rotate(iced::Radians(self.facing.radians()));
        frame.scale(scale);
        let mut y = -size.height / 2.0 + 16.0;
        for line in &self.lines {
            frame.fill_text(Text {
                content: fit_to_width(&line.content, line.size, size.width - 32.0),
                position: Point::new(0.0, y),
                size: line.size.into(),
                color: if line.muted {
                    style::TEXT_MUTED
                } else {
                    style::TEXT
                },
                horizontal_alignment: alignment::Horizontal::Center,
                ..Text::default()
            });
            y += line.height() + 8.0;
        }
        if let Some(qr) = &self.qr {
            frame.draw_image(
                Rectangle::new(Point::new(-80.0, y), Size::new(160.0, 160.0)),
                qr,
            );
        }
        for (action, rect) in self.actions.iter().zip(rects) {
            let active = action.message.is_some();
            let highlighted =
                action.selected || (active && hovered.is_some_and(|p| rect.contains(p)));
            let path = Path::rounded_rectangle(rect.position(), rect.size(), style::R_MD.into());
            frame.fill(
                &path,
                if highlighted {
                    style::ACCENT_DEEP
                } else {
                    style::SURFACE_2
                },
            );
            frame.stroke(
                &path,
                canvas::Stroke::default().with_color(if action.selected {
                    style::ACCENT_BRIGHT
                } else {
                    style::SURFACE_3
                }),
            );
            frame.fill_text(Text {
                content: fit_to_width(&action.label, 22.0, rect.width - 24.0),
                position: rect.center(),
                size: 22.0.into(),
                color: if active {
                    style::TEXT
                } else {
                    style::TEXT_MUTED
                },
                horizontal_alignment: alignment::Horizontal::Center,
                vertical_alignment: alignment::Vertical::Center,
                ..Text::default()
            });
        }
        vec![frame.into_geometry()]
    }
}

pub fn outcome<'a, Msg: Clone + 'a>(
    lines: Vec<Line>,
    qr: Option<iced::widget::image::Handle>,
    actions: Vec<OutcomeAction<Msg>>,
    facing: SeatOrientation,
) -> Element<'a, Msg> {
    Canvas::new(Outcome {
        lines,
        qr,
        actions,
        facing,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[cfg(test)]
mod outcome_tests {
    use super::*;
    #[test]
    fn outcome_buttons_follow_all_seat_orientations_for_mouse_and_touch() {
        for facing in [
            SeatOrientation::Upright,
            SeatOrientation::UpsideDown,
            SeatOrientation::LeftHead,
            SeatOrientation::RightHead,
        ] {
            let panel = Outcome {
                lines: vec![Line::new("Ada wins", 32.0)],
                qr: None,
                actions: (0..8)
                    .map(|i| OutcomeAction {
                        label: i.to_string(),
                        message: (i != 7).then_some(i),
                        selected: i == 2,
                    })
                    .collect(),
                facing,
            };
            let bounds = Rectangle::new(Point::new(30.0, 50.0), Size::new(400.0, 300.0));
            let (_, scale, rects) = panel.layout(bounds.size());
            for (i, rect) in rects.iter().enumerate() {
                let p = rect.center();
                let (sin, cos) = facing.radians().sin_cos();
                let screen = Point::new(
                    bounds.center_x() + scale * (p.x * cos - p.y * sin),
                    bounds.center_y() + scale * (p.x * sin + p.y * cos),
                );
                assert!(bounds.contains(screen));
                for event in [
                    canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(
                        iced::mouse::Button::Left,
                    )),
                    canvas::Event::Touch(iced::touch::Event::FingerPressed {
                        id: iced::touch::Finger(0),
                        position: screen,
                    }),
                ] {
                    let (_, message) = canvas::Program::update(
                        &panel,
                        &mut (),
                        event,
                        bounds,
                        iced::mouse::Cursor::Available(screen),
                    );
                    assert_eq!(message, (i != 7).then_some(i as i32));
                }
            }
        }
    }
}
