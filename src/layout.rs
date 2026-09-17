//! Seating arrangements for a pod. A table isn't always an even grid -
//! two players sit one on top of the other rather than side by side, three
//! often has one person alone on one side and two clustered on the other,
//! and bigger pods can have one or two people at either head of the table
//! instead of everyone facing straight across. Each `TableLayout` is a list
//! of columns (left to right); each column stacks its seats top to bottom.

use iced::widget::{column, row};
use iced::{Element, Length};

#[derive(Debug, Clone, PartialEq)]
pub struct TableLayout {
    pub name: String,
    pub columns: Vec<Vec<usize>>,
}

impl TableLayout {
    fn new(name: &str, columns: Vec<Vec<usize>>) -> Self {
        Self {
            name: name.to_string(),
            columns,
        }
    }
}

/// The curated layout choices for a given pod size, most "normal" first.
pub fn options_for(pod_size: usize) -> Vec<TableLayout> {
    match pod_size {
        2 => vec![TableLayout::new("Stacked", vec![vec![0, 1]])],
        3 => vec![
            TableLayout::new("Left Big", vec![vec![0], vec![1, 2]]),
            TableLayout::new("Right Big", vec![vec![1, 2], vec![0]]),
        ],
        4 => vec![
            TableLayout::new("Two Sides", vec![vec![0, 1], vec![2, 3]]),
            TableLayout::new("Head Left", vec![vec![0], vec![1, 2, 3]]),
            TableLayout::new("Head Right", vec![vec![1, 2, 3], vec![0]]),
        ],
        5 => vec![
            TableLayout::new("Two Sides (2/3)", vec![vec![0, 1], vec![2, 3, 4]]),
            TableLayout::new("Two Sides (3/2)", vec![vec![0, 1, 2], vec![3, 4]]),
            TableLayout::new("Head + Two Sides", vec![vec![0], vec![1, 2], vec![3, 4]]),
        ],
        6 => vec![
            TableLayout::new("Two Sides", vec![vec![0, 1, 2], vec![3, 4, 5]]),
            TableLayout::new(
                "Two Heads",
                vec![vec![0], vec![1, 2], vec![3, 4], vec![5]],
            ),
            TableLayout::new("Head + Five", vec![vec![0], vec![1, 2, 3, 4, 5]]),
        ],
        7 => vec![
            TableLayout::new("Two Sides (3/4)", vec![vec![0, 1, 2], vec![3, 4, 5, 6]]),
            TableLayout::new("Two Sides (4/3)", vec![vec![0, 1, 2, 3], vec![4, 5, 6]]),
            TableLayout::new(
                "Head + Two Sides",
                vec![vec![0], vec![1, 2, 3], vec![4, 5, 6]],
            ),
        ],
        8 => vec![
            TableLayout::new("Two Sides", vec![vec![0, 1, 2, 3], vec![4, 5, 6, 7]]),
            TableLayout::new(
                "Two Heads (1+3+3+1)",
                vec![vec![0], vec![1, 2, 3], vec![4, 5, 6], vec![7]],
            ),
            TableLayout::new(
                "Two Heads (2+2+2+2)",
                vec![vec![0, 1], vec![2, 3], vec![4, 5], vec![6, 7]],
            ),
        ],
        _ => {
            let half = pod_size / 2;
            vec![TableLayout::new(
                "Two Sides",
                vec![(0..half).collect(), (half..pod_size).collect()],
            )]
        }
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
