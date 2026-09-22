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
use crate::style;

/// One line of a chip, with its own size.
#[derive(Debug, Clone)]
pub struct Line {
    pub content: String,
    pub size: f32,
}

impl Line {
    pub fn new(content: impl Into<String>, size: f32) -> Self {
        Self {
            content: content.into(),
            size,
        }
    }

    /// Rough rendered width. Canvas text can't be measured before it's
    /// drawn, and this only has to size the chip behind it, so an average
    /// advance width for the UI font is close enough.
    fn width(&self) -> f32 {
        self.content.chars().count() as f32 * self.size * AVERAGE_ADVANCE
    }

    fn height(&self) -> f32 {
        self.size * 1.25
    }
}

struct Chip<Msg> {
    lines: Vec<Line>,
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

impl<Msg> Chip<Msg> {
    /// The chip's fitted lines, its own box, and where that box's centre
    /// lands. Drawing and hit-testing both go through this, so a tap can
    /// never land somewhere the chip isn't actually drawn.
    fn placement(&self, bounds: Size) -> (Vec<Line>, Size, Point) {
        // A turned chip is limited by the tile's *other* dimension, since
        // its width runs across the tile's height.
        let span = if self.facing.is_sideways() {
            bounds.height
        } else {
            bounds.width
        };
        let mut available = (span - EDGE_MARGIN * 2.0 - self.padding * 2.0).max(0.0);
        if self.hug_edge {
            // Only chips sharing an edge are rationed. The life counter sits
            // in the middle of the tile on its own and gets the whole span,
            // which it needs - a three-digit total must never be truncated.
            available = available.min((span * max_share(self.align) - self.padding * 2.0).max(0.0));
        }

        let lines: Vec<Line> = self
            .lines
            .iter()
            .map(|line| Line {
                content: fit_to_width(&line.content, line.size, available),
                size: line.size,
            })
            .collect();

        let text_w = lines.iter().map(Line::width).fold(0.0_f32, f32::max);
        let text_h: f32 = lines.iter().map(Line::height).sum();
        let box_size = Size::new(text_w + self.padding * 2.0, text_h + self.padding * 2.0);
        let center = chip_center(bounds, box_size, self.facing, self.hug_edge, self.align);

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
    matches!(
        facing,
        SeatOrientation::Upright | SeatOrientation::LeftHead
    )
}

/// The chip's centre *along* its edge. A chip too big to sit off to one side
/// collapses back to the middle rather than hanging off the end of the tile.
fn along_edge(
    bounds: Size,
    footprint: Size,
    facing: SeatOrientation,
    align: EdgeAlign,
) -> f32 {
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
        _cursor: iced::mouse::Cursor,
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
            frame.fill(&chip, self.background);
            frame.stroke(
                &chip,
                canvas::Stroke {
                    style: canvas::Style::Solid(self.border),
                    width: 1.0,
                    ..Default::default()
                },
            );

            let mut y = -text_h / 2.0;
            for line in &lines {
                frame.fill_text(Text {
                    content: line.content.clone(),
                    position: Point::new(0.0, y + line.height() / 2.0),
                    color: style::TEXT,
                    size: line.size.into(),
                    horizontal_alignment: alignment::Horizontal::Center,
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

/// A frosted label turned to face `facing`, centred on the edge of the tile
/// that player is on.
pub fn edge_chip<'a, Msg: Clone + 'a>(
    lines: Vec<Line>,
    facing: SeatOrientation,
) -> Element<'a, Msg> {
    chip_canvas(
        lines,
        facing,
        true,
        EdgeAlign::Center,
        18.0,
        style::glass_paint(),
        None,
    )
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

// --- The seat action menu --------------------------------------------------

/// Height of one row of a turned menu, and the gap between rows. Both are
/// sized for a finger rather than a cursor.
const MENU_ROW_H: f32 = 84.0;
const MENU_ROW_GAP: f32 = 10.0;
const MENU_TEXT: f32 = 22.0;
/// How much of the tile a menu row spans.
const MENU_WIDTH_SHARE: f32 = 0.7;

/// A list of tappable rows, centred on the tile and turned to face its
/// player. One canvas draws and hit-tests the lot - the rows have to read
/// the right way up for whoever the tile belongs to, and iced can't rotate
/// a column of real buttons.
struct Menu<Msg> {
    items: Vec<(String, Msg)>,
    facing: SeatOrientation,
}

impl<Msg> Menu<Msg> {
    /// The row width, and each row's centre offset along the *text's* own
    /// down axis - so the list reads top to bottom for its player whichever
    /// way the tile is turned.
    fn layout(&self, bounds: Size) -> (f32, Vec<f32>) {
        let span = if self.facing.is_sideways() {
            bounds.height
        } else {
            bounds.width
        };
        let width = (span * MENU_WIDTH_SHARE).max(0.0);
        let count = self.items.len() as f32;
        let total = count * MENU_ROW_H + (count - 1.0).max(0.0) * MENU_ROW_GAP;
        let offsets = (0..self.items.len())
            .map(|i| -total / 2.0 + MENU_ROW_H / 2.0 + i as f32 * (MENU_ROW_H + MENU_ROW_GAP))
            .collect();
        (width, offsets)
    }

    /// A row's offset turned into screen space. Rows only ever sit along the
    /// text's down axis, so each quarter turn is a straight swap of axes.
    fn screen_offset(&self, local_y: f32) -> (f32, f32) {
        match self.facing {
            SeatOrientation::Upright => (0.0, local_y),
            SeatOrientation::UpsideDown => (0.0, -local_y),
            SeatOrientation::LeftHead => (-local_y, 0.0),
            SeatOrientation::RightHead => (local_y, 0.0),
        }
    }

    fn row_rect(&self, bounds: Size, width: f32, local_y: f32) -> Rectangle {
        let footprint = screen_footprint(Size::new(width, MENU_ROW_H), self.facing);
        let (dx, dy) = self.screen_offset(local_y);
        let cx = bounds.width / 2.0 + dx;
        let cy = bounds.height / 2.0 + dy;
        Rectangle::new(
            Point::new(cx - footprint.width / 2.0, cy - footprint.height / 2.0),
            footprint,
        )
    }
}

impl<Msg: Clone> canvas::Program<Msg> for Menu<Msg> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> (canvas::event::Status, Option<Msg>) {
        let Some(position) = press_position(&event, bounds, cursor) else {
            return (canvas::event::Status::Ignored, None);
        };
        let (width, offsets) = self.layout(bounds.size());
        for ((_, message), &local_y) in self.items.iter().zip(offsets.iter()) {
            if self.row_rect(bounds.size(), width, local_y).contains(position) {
                return (canvas::event::Status::Captured, Some(message.clone()));
            }
        }
        // The menu covers the tile, so swallow presses that miss a row
        // rather than letting them fall through to the counter underneath.
        (canvas::event::Status::Captured, None)
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (width, offsets) = self.layout(bounds.size());
        let paint = style::glass_strong_paint();

        frame.with_save(|frame| {
            frame.translate(iced::Vector::new(
                bounds.width / 2.0,
                bounds.height / 2.0,
            ));
            frame.rotate(iced::Radians(self.facing.radians()));

            for ((label, _), &local_y) in self.items.iter().zip(offsets.iter()) {
                let top_left = Point::new(-width / 2.0, local_y - MENU_ROW_H / 2.0);
                let row = Path::rounded_rectangle(
                    top_left,
                    Size::new(width, MENU_ROW_H),
                    paint.radius.into(),
                );
                frame.fill(&row, paint.background);
                frame.stroke(
                    &row,
                    canvas::Stroke {
                        style: canvas::Style::Solid(paint.border),
                        width: 1.0,
                        ..Default::default()
                    },
                );
                frame.fill_text(Text {
                    content: fit_to_width(label, MENU_TEXT, width - 32.0),
                    position: Point::new(0.0, local_y),
                    color: style::TEXT,
                    size: MENU_TEXT.into(),
                    horizontal_alignment: alignment::Horizontal::Center,
                    vertical_alignment: alignment::Vertical::Center,
                    ..Text::default()
                });
            }
        });

        vec![frame.into_geometry()]
    }
}

/// The swipe-up seat menu, turned to face the player whose tile it is.
pub fn menu<'a, Msg: Clone + 'a>(
    items: Vec<(String, Msg)>,
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
        let up = chip_center(TILE, CHIP, SeatOrientation::Upright, true, EdgeAlign::Center);
        let down = chip_center(TILE, CHIP, SeatOrientation::UpsideDown, true, EdgeAlign::Center);
        let left = chip_center(TILE, CHIP, SeatOrientation::LeftHead, true, EdgeAlign::Center);
        let right = chip_center(TILE, CHIP, SeatOrientation::RightHead, true, EdgeAlign::Center);

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
        a.x < b.x + b.width
            && b.x < a.x + a.width
            && a.y < b.y + b.height
            && b.y < a.y + a.height
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
            assert!(!overlaps(start, middle), "{facing:?}: start overlaps centre");
            assert!(!overlaps(middle, end), "{facing:?}: centre overlaps end");
            assert!(!overlaps(start, end), "{facing:?}: start overlaps end");
        }
    }

    /// Start is the player's left hand and End their right, whichever way
    /// their tile is turned - so the two are never in the same place, and
    /// they swap sides on screen for the seats facing the other way.
    #[test]
    fn start_and_end_follow_the_player_not_the_screen() {
        let upright_start = chip_center(TILE, CHIP, SeatOrientation::Upright, true, EdgeAlign::Start);
        let upright_end = chip_center(TILE, CHIP, SeatOrientation::Upright, true, EdgeAlign::End);
        assert!(upright_start.x < upright_end.x, "upright reads left to right");

        let flipped_start =
            chip_center(TILE, CHIP, SeatOrientation::UpsideDown, true, EdgeAlign::Start);
        let flipped_end =
            chip_center(TILE, CHIP, SeatOrientation::UpsideDown, true, EdgeAlign::End);
        assert!(
            flipped_start.x > flipped_end.x,
            "upside down reads right to left on screen"
        );

        let left_start = chip_center(TILE, CHIP, SeatOrientation::LeftHead, true, EdgeAlign::Start);
        let left_end = chip_center(TILE, CHIP, SeatOrientation::LeftHead, true, EdgeAlign::End);
        assert!(left_start.y < left_end.y, "left-head reads down the screen");

        let right_start =
            chip_center(TILE, CHIP, SeatOrientation::RightHead, true, EdgeAlign::Start);
        let right_end = chip_center(TILE, CHIP, SeatOrientation::RightHead, true, EdgeAlign::End);
        assert!(right_start.y > right_end.y, "right-head reads up the screen");
    }

    /// A chip too wide to sit off to one side collapses to the middle
    /// instead of hanging off the end of the tile.
    #[test]
    fn an_oversized_chip_stays_on_the_tile() {
        let huge = Size::new(TILE.width * 2.0, 90.0);
        for align in [EdgeAlign::Start, EdgeAlign::Center, EdgeAlign::End] {
            let center = chip_center(TILE, huge, SeatOrientation::Upright, true, align);
            assert_eq!(center.x, TILE.width / 2.0, "{align:?} should collapse to centre");
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
