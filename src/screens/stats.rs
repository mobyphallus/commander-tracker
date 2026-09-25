use iced::alignment::Horizontal;
use iced::widget::{column, container, row, scrollable, text, Space};
use iced::{Alignment, Element, Length};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::model::{
    GrudgeStat, HatedCommanderStat, HaterStat, MatchupStat, PlayerStat, WinReasonStat,
};
use crate::style;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsTab {
    Players,
    Matchups,
    Hate,
}

impl StatsTab {
    const ALL: [StatsTab; 3] = [Self::Players, Self::Matchups, Self::Hate];

    fn label(&self) -> &'static str {
        match self {
            Self::Players => "Players",
            Self::Matchups => "Matchups",
            Self::Hate => "Commander Hate",
        }
    }
}

pub struct StatsState {
    pub tab: StatsTab,
    pub players: Vec<PlayerStat>,
    pub matchups: Vec<MatchupStat>,
    pub haters: Vec<HaterStat>,
    pub hated: Vec<HatedCommanderStat>,
    pub grudges: Vec<GrudgeStat>,
    pub win_reasons: Vec<WinReasonStat>,
    pub average_turns: Option<f64>,
}

impl StatsState {
    pub fn load(conn: &Connection) -> Self {
        Self {
            tab: StatsTab::Players,
            players: db::player_stats(conn).unwrap_or_default(),
            matchups: db::commander_matchup_stats(conn).unwrap_or_default(),
            haters: db::hater_stats(conn).unwrap_or_default(),
            hated: db::hated_commander_stats(conn).unwrap_or_default(),
            grudges: db::grudge_stats(conn).unwrap_or_default(),
            win_reasons: db::win_reason_stats(conn).unwrap_or_default(),
            average_turns: db::average_game_turns(conn).ok().flatten(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum StatsMessage {
    SwitchTab(StatsTab),
}

pub fn update(state: &mut StatsState, message: StatsMessage) {
    match message {
        StatsMessage::SwitchTab(tab) => state.tab = tab,
    }
}

// ---------------------------------------------------------------------------
// Table geometry
//
// Every table on this screen shares one set of column widths and one spacing
// rhythm, so a number in the third column is in the same place whichever tab
// you're on and figures can be compared straight down the page.
// ---------------------------------------------------------------------------

/// The comparison bar's column.
const W_BAR: f32 = 140.0;
/// A plain count ("14", "1.4").
const W_NUM: f32 = 72.0;
/// The headline figure of a row - a percentage or a win-loss record.
const W_HEAD: f32 = 100.0;
/// Bar thickness. Thin on purpose: it's a comparison aid, not a chart.
const BAR_H: f32 = 10.0;

/// Row padding: snug vertically, a full gap in from the edge.
const ROW_PAD: [u16; 2] = [style::GAP_SM, style::GAP];

// ---------------------------------------------------------------------------
// Table parts
// ---------------------------------------------------------------------------

/// A column heading. Muted and small - the units live up here ("Games",
/// "Per game") so the cells underneath can be bare figures.
fn head<'a>(label: &'a str, width: f32, align: Horizontal) -> Element<'a, Message> {
    text(label)
        .size(style::T_CAPTION)
        .color(style::TEXT_MUTED)
        .width(Length::Fixed(width))
        .align_x(align)
        .into()
}

/// The heading over the first, name-carrying column.
fn head_name<'a>(label: &'a str) -> Element<'a, Message> {
    text(label)
        .size(style::T_CAPTION)
        .color(style::TEXT_MUTED)
        .width(Length::Fill)
        .into()
}

/// A figure, right-aligned so digits stack into a comparable column.
fn figure<'a>(value: String, width: f32, size: u16) -> Element<'a, Message> {
    text(value)
        .size(size)
        .color(style::TEXT)
        .width(Length::Fixed(width))
        .align_x(Horizontal::Right)
        .into()
}

/// The number the row is actually about.
fn headline<'a>(value: String) -> Element<'a, Message> {
    figure(value, W_HEAD, style::T_LEAD)
}

/// A count that supports the headline rather than competing with it.
fn count<'a>(value: String) -> Element<'a, Message> {
    figure(value, W_NUM, style::T_LABEL)
}

/// The name a row belongs to, with an optional second line of detail.
fn name_cell<'a>(label: String, sub: Option<String>) -> Element<'a, Message> {
    let mut cell =
        column![text(label).size(style::T_SUBHEAD).color(style::TEXT)].spacing(style::GAP_XS);
    if let Some(sub) = sub {
        cell = cell.push(text(sub).size(style::T_CAPTION).color(style::TEXT_MUTED));
    }
    cell.width(Length::Fill).into()
}

/// A proportional bar, for eyeballing one row against the rest of the column.
/// `frac` is clamped into the track, never past either end of it.
fn bar<'a>(frac: f64) -> Element<'a, Message> {
    // FillPortion splits by ratio, so both halves have to stay non-zero;
    // a thousandth of the track is far under a pixel either way.
    let filled = (frac.clamp(0.0, 1.0) * 1000.0).round().clamp(1.0, 999.0) as u16;
    container(
        row![
            container(Space::new(Length::Fill, Length::Fill))
                .width(Length::FillPortion(filled))
                .height(Length::Fill)
                .style(style::meter_fill),
            Space::new(Length::FillPortion(1000 - filled), Length::Fill),
        ]
        .height(Length::Fill),
    )
    .width(Length::Fixed(W_BAR))
    .height(Length::Fixed(BAR_H))
    .clip(true)
    .style(style::meter_track)
    .into()
}

/// One line of a table: the name takes the slack, the fixed-width cells
/// after it line up down the screen.
fn stat_row<'a>(
    compact: bool,
    name: Element<'a, Message>,
    cells: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let line: Element<Message> = if compact {
        column![
            name,
            row(cells).spacing(style::GAP).align_y(Alignment::Center)
        ]
        .spacing(style::GAP_SM)
        .into()
    } else {
        row(std::iter::once(name).chain(cells))
            .spacing(style::GAP)
            .align_y(Alignment::Center)
            .into()
    };
    container(line)
        .padding(ROW_PAD)
        .width(Length::Fill)
        .style(style::table_row)
        .into()
}

/// Something deliberate to look at when there's no data - centred, so it
/// reads as a state of the screen rather than as a row that failed to draw.
fn empty_state<'a>(headline: &'a str, note: &'a str) -> Element<'a, Message> {
    container(
        column![
            text(headline).size(style::T_LEAD).color(style::TEXT),
            text(note).size(style::T_BODY).color(style::TEXT_MUTED),
        ]
        .spacing(style::GAP_XS)
        .align_x(Alignment::Center),
    )
    .padding(style::GAP * 2)
    .center_x(Length::Fill)
    .style(style::panel)
    .into()
}

/// A titled block: a heading, a row of column headings, and the rows - or a
/// centred note where the rows would be.
fn table<'a>(
    compact: bool,
    title: &'a str,
    heads: Vec<Element<'a, Message>>,
    rows: Vec<Element<'a, Message>>,
    empty_headline: &'a str,
    empty_note: &'a str,
) -> Element<'a, Message> {
    let body: Element<Message> = if rows.is_empty() {
        empty_state(empty_headline, empty_note)
    } else {
        column![
            container(if compact {
                let mut heads = heads.into_iter();
                column![heads.next().unwrap(), row(heads).spacing(style::GAP)]
                    .spacing(style::GAP_SM)
                    .into()
            } else {
                Element::from(row(heads).spacing(style::GAP).align_y(Alignment::Center))
            })
            .padding(ROW_PAD),
            column(rows).spacing(style::GAP_SM),
        ]
        .into()
    };

    column![text(title).size(style::T_LEAD).color(style::TEXT), body]
        .spacing(style::GAP_SM)
        .into()
}

/// A single figure that doesn't belong to a table - big number, quiet label.
fn callout<'a>(value: String, caption: &'a str) -> Element<'a, Message> {
    container(
        column![
            text(value).size(style::T_DISPLAY).color(style::TEXT),
            text(caption)
                .size(style::T_CAPTION)
                .color(style::TEXT_MUTED),
        ]
        .spacing(style::GAP_XS),
    )
    .padding(style::GAP)
    .width(Length::Fill)
    .style(style::panel)
    .into()
}

// ---------------------------------------------------------------------------
// Screen
// ---------------------------------------------------------------------------

pub fn view(state: &StatsState) -> Element<'_, Message> {
    iced::widget::responsive(move |size| view_sized(state, size.width < 1100.)).into()
}

fn view_sized(state: &StatsState, compact: bool) -> Element<'_, Message> {
    let header = style::page_header("Table Stats", "Every game tells a story", Message::GoHome);

    // A segmented control rather than three loose buttons: one panel holds
    // the set, and only the selected segment is filled, so which tab you're
    // on is unmistakable from across the table.
    let tabs = container(
        row(StatsTab::ALL
            .iter()
            .map(|t| {
                let selected = state.tab == *t;
                style::touch_button(t.label(), style::T_ACTION)
                    .width(Length::Fill)
                    .style(if selected {
                        style::primary
                    } else {
                        style::ghost
                    })
                    .on_press(Message::Stats(StatsMessage::SwitchTab(*t)))
                    .into()
            })
            .collect::<Vec<Element<Message>>>())
        .spacing(style::GAP_XS),
    )
    .padding(style::GAP_XS)
    .width(Length::Fill)
    .style(style::panel);

    let body = match state.tab {
        StatsTab::Players => players_tab(state, compact),
        StatsTab::Matchups => matchups_tab(state, compact),
        StatsTab::Hate => hate_tab(state, compact),
    };

    container(
        column![header, tabs, scrollable(body).height(Length::Fill)]
            .spacing(style::GAP)
            .padding(style::GAP),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn players_tab(state: &StatsState, compact: bool) -> Element<'_, Message> {
    if state.players.is_empty() && state.win_reasons.is_empty() {
        return empty_state(
            "No games recorded yet",
            "Finish a game and win rates, endings and pace all land here.",
        );
    }

    let rows: Vec<Element<Message>> = state
        .players
        .iter()
        .map(|p| {
            let rate = if p.games > 0 {
                p.wins as f64 / p.games as f64
            } else {
                0.0
            };
            stat_row(
                compact,
                name_cell(p.player_name.clone(), None),
                vec![
                    bar(rate),
                    count(p.wins.to_string()),
                    count((p.games - p.wins).to_string()),
                    count(p.games.to_string()),
                    headline(format!("{:.0}%", rate * 100.0)),
                ],
            )
        })
        .collect();

    let total_endings: i64 = state.win_reasons.iter().map(|w| w.games).sum();
    let win_rows: Vec<Element<Message>> = state
        .win_reasons
        .iter()
        .map(|w| {
            let share = if total_endings > 0 {
                w.games as f64 / total_endings as f64
            } else {
                0.0
            };
            stat_row(
                compact,
                name_cell(w.reason.label().to_string(), None),
                vec![
                    bar(share),
                    count(format!("{:.0}%", share * 100.0)),
                    headline(w.games.to_string()),
                ],
            )
        })
        .collect();

    let pace: Element<Message> = match state.average_turns {
        Some(avg) => callout(format!("{avg:.1}"), "average turns per finished game"),
        None => empty_state(
            "No finished games yet",
            "Game length shows up once a game has been played out to a winner.",
        ),
    };

    column![
        table(
            compact,
            "Win rates",
            vec![
                head_name("Player"),
                head("Win rate", W_BAR, Horizontal::Left),
                head("Won", W_NUM, Horizontal::Right),
                head("Lost", W_NUM, Horizontal::Right),
                head("Games", W_NUM, Horizontal::Right),
                head("Win rate", W_HEAD, Horizontal::Right),
            ],
            rows,
            "Nobody has a record yet",
            "Win rates appear as soon as somebody wins a game.",
        ),
        table(
            compact,
            "How games end",
            vec![
                head_name("Ending"),
                head("Share", W_BAR, Horizontal::Left),
                head("Share", W_NUM, Horizontal::Right),
                head("Games", W_HEAD, Horizontal::Right),
            ],
            win_rows,
            "No endings recorded yet",
            "Each finished game files itself under how it was won.",
        ),
        column![text("Pace").size(style::T_LEAD).color(style::TEXT), pace].spacing(style::GAP_SM),
    ]
    .spacing(style::GAP)
    .into()
}

fn matchups_tab(state: &StatsState, compact: bool) -> Element<'_, Message> {
    let rows: Vec<Element<Message>> = state
        .matchups
        .iter()
        .map(|m| {
            let decided = m.a_wins + m.b_wins;
            let share = if decided > 0 {
                m.a_wins as f64 / decided as f64
            } else {
                0.0
            };
            stat_row(
                compact,
                name_cell(m.commander_a.clone(), Some(format!("vs {}", m.commander_b))),
                vec![
                    bar(share),
                    count(m.games.to_string()),
                    headline(format!("{}\u{2013}{}", m.a_wins, m.b_wins)),
                ],
            )
        })
        .collect();

    table(
        compact,
        "Commander matchups",
        vec![
            head_name("Matchup"),
            head("Split", W_BAR, Horizontal::Left),
            head("Games", W_NUM, Horizontal::Right),
            head("Record", W_HEAD, Horizontal::Right),
        ],
        rows,
        "No head-to-heads yet",
        "Play a few games and every pair of commanders keeps its own record here.",
    )
}

fn hate_tab(state: &StatsState, compact: bool) -> Element<'_, Message> {
    if state.haters.is_empty() && state.hated.is_empty() && state.grudges.is_empty() {
        return empty_state(
            "No hate logged yet",
            "Log a kill, a wipe or a counter from a seat tile during a game and it all adds up here.",
        );
    }

    // Bars here compare against the busiest row rather than against a
    // percentage: the question is who throws the most hate, not what share.
    let hater_max = state
        .haters
        .iter()
        .map(|h| h.total)
        .max()
        .unwrap_or(0)
        .max(1) as f64;
    let haters: Vec<Element<Message>> = state
        .haters
        .iter()
        .map(|h| {
            stat_row(
                compact,
                name_cell(
                    h.player_name.clone(),
                    Some(format!(
                        "{} kills \u{00b7} {} wipes \u{00b7} {} counters",
                        h.kills, h.wipes, h.counters
                    )),
                ),
                vec![
                    bar(h.total as f64 / hater_max),
                    count(format!("{:.1}", h.per_game())),
                    headline(h.total.to_string()),
                ],
            )
        })
        .collect();

    let hated_max = state
        .hated
        .iter()
        .map(|c| c.total)
        .max()
        .unwrap_or(0)
        .max(1) as f64;
    let hated: Vec<Element<Message>> = state
        .hated
        .iter()
        .map(|c| {
            stat_row(
                compact,
                name_cell(
                    c.commander_name.clone(),
                    Some(format!(
                        "{} kills \u{00b7} {} wipes \u{00b7} {} counters",
                        c.kills, c.wipes, c.counters
                    )),
                ),
                vec![
                    bar(c.total as f64 / hated_max),
                    count(format!("{:.1}", c.per_appearance())),
                    headline(c.total.to_string()),
                ],
            )
        })
        .collect();

    let grudge_max = state
        .grudges
        .iter()
        .map(|g| g.total)
        .max()
        .unwrap_or(0)
        .max(1) as f64;
    let grudges: Vec<Element<Message>> = state
        .grudges
        .iter()
        .map(|g| {
            stat_row(
                compact,
                name_cell(
                    format!("{} \u{203a} {}", g.hater_name, g.commander_name),
                    Some(format!("piloted by {}", g.victim_name)),
                ),
                vec![
                    bar(g.total as f64 / grudge_max),
                    headline(g.total.to_string()),
                ],
            )
        })
        .collect();

    column![
        table(
            compact,
            "Biggest haters",
            vec![
                head_name("Player"),
                head("Share", W_BAR, Horizontal::Left),
                head("Per game", W_NUM, Horizontal::Right),
                head("Total", W_HEAD, Horizontal::Right),
            ],
            haters,
            "Nobody has thrown any hate yet",
            "Log it from a seat tile during a game and the tally builds up here.",
        ),
        table(
            compact,
            "Most hated commanders",
            vec![
                head_name("Commander"),
                head("Share", W_BAR, Horizontal::Left),
                head("Per game", W_NUM, Horizontal::Right),
                head("Total", W_HEAD, Horizontal::Right),
            ],
            hated,
            "No commander has drawn heat yet",
            "The table will tell you which one deserves it soon enough.",
        ),
        table(
            compact,
            "Grudges",
            vec![
                head_name("Grudge"),
                head("Share", W_BAR, Horizontal::Left),
                head("Times", W_HEAD, Horizontal::Right),
            ],
            grudges,
            "No repeat targets yet",
            "Anyone who keeps picking on the same commander shows up here.",
        ),
    ]
    .spacing(style::GAP)
    .into()
}
