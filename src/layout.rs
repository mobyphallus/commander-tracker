//! Seating arrangements for a pod. Never more than two rows: every column
//! holds either a pair of seats stacked top/bottom, or a single seat that
//! spans both rows as a tall "head of the table" box. A table isn't always
//! an even grid - two players sit one on top of the other rather than side
//! by side, three often has one person alone on one side and two clustered
//! on the other, and bigger pods can have one or two people at either head
//! of the table instead of everyone facing straight across. Each
//! `TableLayout` is a list of columns (left to right).

use iced::widget::{column, row};
use iced::{Element, Length};

#[derive(Debug, Clone, PartialEq)]
pub struct TableLayout {
    pub name: String,
    pub columns: Vec<Vec<usize>>,
}

/// Which way a seat's contents have to be turned so they read right-way-up
/// for the person sitting at that edge of the table.
///
/// The tablet lies flat in the middle of the pod, so a player's "up" points
/// away from them, across the table. Someone at the bottom edge reads the
/// screen as-is; someone opposite them reads it upside down; someone at a
/// head of the table reads it side-on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeatOrientation {
    /// Bottom row: the screen's own orientation.
    Upright,
    /// Top row: sitting opposite, so a half turn.
    UpsideDown,
    /// Head of the table on the left, facing right across it.
    LeftHead,
    /// Head of the table on the right, facing left across it.
    RightHead,
}

impl SeatOrientation {
    /// Clockwise rotation in degrees, in screen space (y pointing down).
    ///
    /// A player at the left edge faces right, so their "up" is screen-right
    /// and their reading direction is screen-down - which is the screen's
    /// own frame turned a quarter turn clockwise. The right head is the
    /// mirror of that.
    pub fn degrees(self) -> f32 {
        match self {
            SeatOrientation::Upright => 0.0,
            SeatOrientation::LeftHead => 90.0,
            SeatOrientation::UpsideDown => 180.0,
            SeatOrientation::RightHead => 270.0,
        }
    }

    pub fn radians(self) -> f32 {
        self.degrees().to_radians()
    }

    /// True when the seat is turned on its side, so the tile's width and
    /// height swap from the player's point of view.
    pub fn is_sideways(self) -> bool {
        matches!(self, SeatOrientation::LeftHead | SeatOrientation::RightHead)
    }

    /// Whether the player's left hand is on the screen's right, which is
    /// what decides which half of a split counter subtracts.
    pub fn flips_horizontal(self) -> bool {
        matches!(self, SeatOrientation::UpsideDown)
    }

    /// Re-expresses a drag measured in screen pixels as the player sitting
    /// here actually experienced it, returning `(away, across)`.
    ///
    /// `away` is positive when the drag moved away from the player, across
    /// the table - which is what they mean by swiping "up". `across` is how
    /// far it moved sideways in their frame, unsigned.
    ///
    /// This matters because a gesture is judged in the player's frame, not
    /// the screen's. Someone sitting opposite swipes away from themselves by
    /// dragging *down* the screen; someone at a head of the table does it by
    /// dragging sideways. Classifying their drag with the screen's own axes
    /// gets the gesture wrong for every seat except the bottom row, and for
    /// the heads it swaps the two gestures over entirely.
    ///
    /// Done as exact quarter turns rather than with sin/cos, so a drag
    /// straight along an axis can't leak a rounding error into the other
    /// one and tip a near-threshold gesture the wrong way.
    pub fn drag_in_player_frame(self, dx: f32, dy: f32) -> (f32, f32) {
        match self {
            SeatOrientation::Upright => (-dy, dx.abs()),
            SeatOrientation::UpsideDown => (dy, dx.abs()),
            SeatOrientation::LeftHead => (dx, dy.abs()),
            SeatOrientation::RightHead => (-dx, dy.abs()),
        }
    }
}

/// Which way turns pass around the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnDirection {
    Clockwise,
    CounterClockwise,
}

impl TurnDirection {
    pub fn label(self) -> &'static str {
        match self {
            TurnDirection::Clockwise => "Clockwise",
            TurnDirection::CounterClockwise => "Counter-clockwise",
        }
    }
}

impl TableLayout {
    fn new(name: &str, columns: Vec<Vec<usize>>) -> Self {
        Self {
            name: name.to_string(),
            columns,
        }
    }

    /// How `seat`'s tile must be turned to face the player sitting there.
    ///
    /// A column holding two seats is a side of the table: the first is the
    /// top row (someone sitting opposite, upside down) and the second the
    /// bottom row (facing the screen the normal way up). A column holding
    /// one seat is a head of the table, and which head it is depends on
    /// whether it sits in the left or right half of the board.
    pub fn seat_orientation(&self, seat: usize) -> SeatOrientation {
        let Some((index, column)) = self
            .columns
            .iter()
            .enumerate()
            .find(|(_, col)| col.contains(&seat))
        else {
            return SeatOrientation::Upright;
        };

        if column.len() == 1 {
            // Compare against the midpoint of the columns rather than a
            // fixed index, so this holds for any pod size.
            let midpoint = (self.columns.len() as f32 - 1.0) / 2.0;
            return if (index as f32) < midpoint {
                SeatOrientation::LeftHead
            } else {
                SeatOrientation::RightHead
            };
        }

        if column.first() == Some(&seat) {
            SeatOrientation::UpsideDown
        } else {
            SeatOrientation::Upright
        }
    }

    /// Seat indices in physical clockwise order around the table.
    ///
    /// Seat *index* order is not table order. In a "Two Sides" four-pod the
    /// columns are `[[0,1],[2,3]]`, so seats 0 and 1 share the left side
    /// facing 2 and 3 - going clockwise from the top left crosses the table
    /// rather than walking down it: 0, 2, 3, 1. So the ring is the top row
    /// read left to right, then the bottom row read right to left. A column
    /// holding a single seat is a head of the table spanning both rows, so
    /// it appears once, in the top pass.
    pub fn ring_order(&self) -> Vec<usize> {
        let mut ring: Vec<usize> = self
            .columns
            .iter()
            .filter_map(|col| col.first().copied())
            .collect();
        ring.extend(
            self.columns
                .iter()
                .rev()
                .filter_map(|col| col.get(1).copied()),
        );
        ring
    }

    /// Roughly how wide `seat`'s tile is relative to its height in this
    /// layout. Used to shape the framing preview so what you line up there
    /// is what the game actually shows. A lone seat in a column is a head of
    /// the table spanning both rows, so it's half as wide for its height.
    pub fn tile_aspect(&self, seat: usize) -> f32 {
        // The board area is about twice as wide as it is tall on this
        // screen; exact enough for a preview that only needs the shape.
        const BOARD_ASPECT: f32 = 2.0;
        let columns = self.columns.len().max(1) as f32;
        let full_height = self
            .columns
            .iter()
            .find(|col| col.contains(&seat))
            .is_some_and(|col| col.len() == 1);
        let height_fraction = if full_height { 1.0 } else { 0.5 };
        let on_screen = (BOARD_ASPECT / columns) / height_fraction;

        // A head of the table sees the tile turned on its side, so what
        // they're framing is the tile's shape swapped round.
        if self.seat_orientation(seat).is_sideways() {
            1.0 / on_screen
        } else {
            on_screen
        }
    }

    /// The seats in the order they'll take turns: start at `first`, then
    /// walk the ring in `direction`.
    pub fn turn_order(&self, first: usize, direction: TurnDirection) -> Vec<usize> {
        let mut ring = self.ring_order();
        if direction == TurnDirection::CounterClockwise {
            ring.reverse();
        }
        if let Some(at) = ring.iter().position(|&seat| seat == first) {
            ring.rotate_left(at);
        }
        ring
    }
}

/// The curated layout choices for a given pod size, most "normal" first.
/// Every option here keeps every column at 1 or 2 seats - a lone seat is a
/// tall head-of-table box, never a third row.
pub fn options_for(pod_size: usize) -> Vec<TableLayout> {
    match pod_size {
        2 => vec![TableLayout::new("Stacked", vec![vec![0, 1]])],
        3 => vec![
            TableLayout::new("Head Left", vec![vec![0], vec![1, 2]]),
            TableLayout::new("Head Right", vec![vec![1, 2], vec![0]]),
        ],
        4 => vec![
            TableLayout::new("Two Sides", vec![vec![0, 1], vec![2, 3]]),
            TableLayout::new("Two Heads", vec![vec![0], vec![1, 2], vec![3]]),
        ],
        5 => vec![
            TableLayout::new("Head Left", vec![vec![0], vec![1, 2], vec![3, 4]]),
            TableLayout::new("Head Right", vec![vec![1, 2], vec![3, 4], vec![0]]),
        ],
        6 => vec![
            TableLayout::new("Three Pairs", vec![vec![0, 1], vec![2, 3], vec![4, 5]]),
            TableLayout::new("Two Heads", vec![vec![0], vec![1, 2], vec![3, 4], vec![5]]),
        ],
        7 => vec![
            TableLayout::new(
                "Head Left",
                vec![vec![0], vec![1, 2], vec![3, 4], vec![5, 6]],
            ),
            TableLayout::new(
                "Head Right",
                vec![vec![1, 2], vec![3, 4], vec![5, 6], vec![0]],
            ),
        ],
        8 => vec![
            TableLayout::new(
                "Four Pairs",
                vec![vec![0, 1], vec![2, 3], vec![4, 5], vec![6, 7]],
            ),
            TableLayout::new(
                "Two Heads",
                vec![vec![0], vec![1, 2], vec![3, 4], vec![5, 6], vec![7]],
            ),
        ],
        _ => Vec::new(),
    }
}

/// Renders `layout` by calling `tile(seat_index)` for every seat, arranging
/// the results into the layout's columns/stacks. `tile` is called in
/// left-to-right, top-to-bottom order.
pub fn render_table<'a, Msg: 'a>(
    layout: &TableLayout,
    mut tile: impl FnMut(usize) -> Element<'a, Msg>,
) -> Element<'a, Msg> {
    let columns: Vec<Element<'a, Msg>> = layout
        .columns
        .iter()
        .map(|seats| {
            column(seats.iter().map(|&idx| tile(idx)).collect::<Vec<_>>())
                .spacing(12)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        })
        .collect();

    row(columns)
        .spacing(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ring has to cross the table, not walk down one side.
    #[test]
    fn ring_crosses_the_table() {
        let two_sides = TableLayout::new("Two Sides", vec![vec![0, 1], vec![2, 3]]);
        assert_eq!(two_sides.ring_order(), vec![0, 2, 3, 1]);
    }

    /// A lone seat is a head of the table and appears exactly once.
    #[test]
    fn heads_appear_once() {
        let two_heads = TableLayout::new("Two Heads", vec![vec![0], vec![1, 2], vec![3]]);
        assert_eq!(two_heads.ring_order(), vec![0, 1, 3, 2]);

        let six = TableLayout::new("Two Heads", vec![vec![0], vec![1, 2], vec![3, 4], vec![5]]);
        let ring = six.ring_order();
        assert_eq!(ring, vec![0, 1, 3, 5, 4, 2]);
        assert_eq!(ring.len(), 6);
    }

    /// Every layout we offer must put every seat in the ring exactly once,
    /// or someone silently never gets a turn.
    #[test]
    fn every_layout_seats_everyone_once() {
        for pod in MIN_POD_TEST..=MAX_POD_TEST {
            for table in options_for(pod) {
                let mut ring = table.ring_order();
                ring.sort_unstable();
                assert_eq!(
                    ring,
                    (0..pod).collect::<Vec<_>>(),
                    "layout {} for pod {pod}",
                    table.name
                );
            }
        }
    }

    const MIN_POD_TEST: usize = 2;
    const MAX_POD_TEST: usize = 8;

    #[test]
    fn turn_order_starts_at_the_chosen_seat() {
        let table = TableLayout::new("Two Sides", vec![vec![0, 1], vec![2, 3]]);
        // Ring is 0,2,3,1. Starting at 3 clockwise wraps round to 3,1,0,2.
        assert_eq!(
            table.turn_order(3, TurnDirection::Clockwise),
            vec![3, 1, 0, 2]
        );
        // Counter-clockwise is the ring reversed (1,3,2,0), rotated to 3.
        assert_eq!(
            table.turn_order(3, TurnDirection::CounterClockwise),
            vec![3, 2, 0, 1]
        );
    }
}

#[cfg(test)]
mod orientation_tests {
    use super::*;

    /// Two players facing each other across the table read the screen from
    /// opposite sides.
    #[test]
    fn opposite_sides_are_half_a_turn_apart() {
        let table = TableLayout::new("Two Sides", vec![vec![0, 1], vec![2, 3]]);
        assert_eq!(table.seat_orientation(0), SeatOrientation::UpsideDown);
        assert_eq!(table.seat_orientation(1), SeatOrientation::Upright);
        assert_eq!(table.seat_orientation(2), SeatOrientation::UpsideDown);
        assert_eq!(table.seat_orientation(3), SeatOrientation::Upright);
    }

    /// A lone seat in a column is a head of the table, and which side it is
    /// on decides whether it turns a quarter turn one way or the other.
    #[test]
    fn heads_face_in_from_their_own_side() {
        let two_heads = TableLayout::new("Two Heads", vec![vec![0], vec![1, 2], vec![3]]);
        assert_eq!(two_heads.seat_orientation(0), SeatOrientation::LeftHead);
        assert_eq!(two_heads.seat_orientation(3), SeatOrientation::RightHead);
        assert_eq!(two_heads.seat_orientation(1), SeatOrientation::UpsideDown);
        assert_eq!(two_heads.seat_orientation(2), SeatOrientation::Upright);

        let head_left = TableLayout::new("Head Left", vec![vec![0], vec![1, 2]]);
        assert_eq!(head_left.seat_orientation(0), SeatOrientation::LeftHead);
        let head_right = TableLayout::new("Head Right", vec![vec![1, 2], vec![0]]);
        assert_eq!(head_right.seat_orientation(0), SeatOrientation::RightHead);
    }

    /// The angles are the ones the table actually needs: a quarter turn
    /// clockwise puts the screen's top at the left player's far side.
    #[test]
    fn angles_match_the_seating() {
        assert_eq!(SeatOrientation::Upright.degrees(), 0.0);
        assert_eq!(SeatOrientation::LeftHead.degrees(), 90.0);
        assert_eq!(SeatOrientation::UpsideDown.degrees(), 180.0);
        assert_eq!(SeatOrientation::RightHead.degrees(), 270.0);
    }

    /// Every layout we offer must give every seat a defined orientation,
    /// and the two heads of a table must never both face the same way.
    #[test]
    fn every_layout_orients_every_seat() {
        for pod in 2..=8usize {
            for table in options_for(pod) {
                let heads: Vec<SeatOrientation> = (0..pod)
                    .map(|s| table.seat_orientation(s))
                    .filter(|o| o.is_sideways())
                    .collect();
                assert!(
                    heads.len() <= 2,
                    "layout {} for pod {pod} has {} heads",
                    table.name,
                    heads.len()
                );
                if heads.len() == 2 {
                    assert_ne!(
                        heads[0], heads[1],
                        "both heads of {} face the same way",
                        table.name
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod gesture_tests {
    use super::SeatOrientation::*;
    use super::*;

    /// Pixels of drag used by the tests, well past any threshold.
    const D: f32 = 100.0;

    /// Each player swipes away from their own body to open the seat menu.
    /// Which screen direction that is depends entirely on where they sit.
    #[test]
    fn swiping_away_is_positive_for_every_seat() {
        // Bottom row: away is up the screen.
        assert_eq!(Upright.drag_in_player_frame(0.0, -D).0, D);
        // Opposite: away is *down* the screen.
        assert_eq!(UpsideDown.drag_in_player_frame(0.0, D).0, D);
        // Left head: away is to the right.
        assert_eq!(LeftHead.drag_in_player_frame(D, 0.0).0, D);
        // Right head: away is to the left.
        assert_eq!(RightHead.drag_in_player_frame(-D, 0.0).0, D);
    }

    /// Dragging toward yourself must never read as dragging away, or the
    /// menu would open when you pull the art the other way.
    #[test]
    fn swiping_toward_yourself_is_negative() {
        assert!(Upright.drag_in_player_frame(0.0, D).0 < 0.0);
        assert!(UpsideDown.drag_in_player_frame(0.0, -D).0 < 0.0);
        assert!(LeftHead.drag_in_player_frame(-D, 0.0).0 < 0.0);
        assert!(RightHead.drag_in_player_frame(D, 0.0).0 < 0.0);
    }

    /// A sideways swipe (the commander-damage gesture) is sideways *to the
    /// player*. For a head of the table that is a vertical drag on screen,
    /// which the old screen-axis logic read as "away" instead.
    #[test]
    fn sideways_is_sideways_in_the_players_frame() {
        for (facing, dx, dy) in [
            (Upright, D, 0.0),
            (UpsideDown, D, 0.0),
            (LeftHead, 0.0, D),
            (RightHead, 0.0, D),
        ] {
            let (away, across) = facing.drag_in_player_frame(dx, dy);
            assert_eq!(across, D, "{facing:?} should read as fully sideways");
            assert_eq!(away, 0.0, "{facing:?} should have no away component");
        }
    }

    /// Whichever way a seat faces, a drag's total travel is unchanged - a
    /// rotation can't lengthen or shorten it, so the same thresholds hold
    /// for every seat at the table.
    #[test]
    fn rotating_preserves_the_length_of_a_drag() {
        for facing in [Upright, UpsideDown, LeftHead, RightHead] {
            for (dx, dy) in [(D, 0.0), (0.0, D), (30.0, -40.0), (-70.0, 24.0)] {
                let (away, across) = facing.drag_in_player_frame(dx, dy);
                let before = (dx * dx + dy * dy).sqrt();
                let after = (away * away + across * across).sqrt();
                assert!(
                    (before - after).abs() < 0.001,
                    "{facing:?} changed drag length {before} -> {after}"
                );
            }
        }
    }

    /// The bottom row must behave exactly as it does today, so fixing the
    /// other seats can't regress the one that was already right.
    #[test]
    fn the_bottom_row_is_unchanged() {
        for (dx, dy) in [(D, 0.0), (0.0, D), (30.0, -40.0), (-70.0, 24.0)] {
            let (away, across) = Upright.drag_in_player_frame(dx, dy);
            assert_eq!(away, -dy);
            assert_eq!(across, dx.abs());
        }
    }
}
