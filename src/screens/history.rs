use iced::widget::{button, column, container, row, scrollable, text, Column};
use iced::{Alignment, Color, Element, Length};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::model::{GameDetail, GameDetailSeat, GameSummary};
use crate::style;

pub struct HistoryState {
    pub games: Vec<GameSummary>,
    pub selected: Option<GameDetail>,
}

impl HistoryState {
    pub fn load(conn: &Connection) -> Self {
        Self {
            games: db::list_games(conn).unwrap_or_default(),
            selected: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum HistoryMessage {
    ViewGame(i64),
    Back,
}

pub fn update(state: &mut HistoryState, conn: &Connection, message: HistoryMessage) {
    match message {
        HistoryMessage::ViewGame(id) => {
            state.selected = db::game_detail(conn, id).ok();
        }
        HistoryMessage::Back => {
            state.selected = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Rhythm and columns
// ---------------------------------------------------------------------------

/// Sub-units of [`style::GAP`]. Nothing on this screen is inset or spaced by
/// anything else.

/// Width of the "Back" control in both headers, so the two views don't shift
/// under your thumb when you move between them.
const BACK_W: f32 = 220.0;

// Fixed column widths for the game list. A history screen is a table, so the
// columns are pinned rather than left to each row's content - dates, results
// and turn counts line up down the page and can be read in one sweep.
const WHEN_W: f32 = 260.0;
const HOW_W: f32 = 340.0;
const COUNT_W: f32 = 130.0;

// The same idea for a seat inside one game's box score.
const OUT_W: f32 = 520.0;
const TOTAL_W: f32 = 140.0;

// ---------------------------------------------------------------------------
// Small parts
// ---------------------------------------------------------------------------

/// A labelled block: a small muted heading tight against the panel it names,
/// so a panel's contents never have to explain what they are, and so the
/// gap between sections is always bigger than the gap inside one.
fn section<'a>(label: &'a str, body: impl Into<Element<'a, Message>>) -> Column<'a, Message> {
    column![
        text(label).size(style::T_CAPTION).color(style::TEXT_MUTED),
        body.into(),
    ]
    .spacing(style::GAP_XS)
}

/// A stacked pair - the thing, then the quieter note under it - which is the
/// only text shape used in a row. Every column is one of these, so the rows
/// share two baselines all the way down.
fn stacked<'a>(
    main: String,
    main_size: u16,
    main_color: Color,
    sub: String,
    width: Length,
    align: Alignment,
) -> Element<'a, Message> {
    container(
        column![
            text(main).size(main_size).color(main_color),
            text(sub).size(style::T_CAPTION).color(style::TEXT_MUTED),
        ]
        .spacing(style::GAP_XS)
        .align_x(align),
    )
    .width(width)
    .into()
}

/// A right-aligned number with the word for what it counts under it.
fn count_cell<'a>(value: String, label: &'a str, width: f32) -> Element<'a, Message> {
    container(
        column![
            text(value).size(style::T_LEAD),
            text(label).size(style::T_CAPTION).color(style::TEXT_MUTED),
        ]
        .spacing(style::GAP_XS)
        .align_x(Alignment::End),
    )
    .width(Length::Fixed(width))
    .into()
}

/// A centred "there is nothing here yet, and that's expected" panel.
fn empty_state<'a>(headline: &'a str, hint: &'a str) -> Element<'a, Message> {
    container(
        container(
            column![
                text(headline).size(style::T_LEAD),
                text(hint).size(style::T_BODY).color(style::TEXT_MUTED),
            ]
            .spacing(style::GAP_SM)
            .align_x(Alignment::Center),
        )
        .padding(style::GAP * 3)
        .style(style::panel),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// A screen header: title on the left, the way out on the right.
fn page_header<'a>(
    title: String,
    sub: String,
    back_label: &'a str,
    back: Message,
) -> Element<'a, Message> {
    container(
        row![
            column![
                text(title).size(style::T_TITLE),
                text(sub).size(style::T_CAPTION).color(style::TEXT_MUTED),
            ]
            .spacing(style::GAP_XS),
            iced::widget::horizontal_space(),
            style::touch_button(back_label, style::T_LABEL)
                .width(Length::Fixed(BACK_W))
                .style(style::ghost)
                .on_press(back),
        ]
        .align_y(Alignment::Center),
    )
    .padding([style::GAP, style::GAP + style::GAP_SM])
    .width(Length::Fill)
    .style(style::header)
    .into()
}

/// How long the game ran, in the shortest form that still reads as a length
/// of time rather than as a number.
fn duration_label(minutes: i64) -> String {
    let minutes = minutes.max(0);
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{}h {:02}m", minutes / 60, minutes % 60)
    }
}

/// `"conceded"` -> `"Conceded"`. The model phrases outcomes mid-sentence;
/// here they start a line of their own.
fn sentence_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// The list of games
// ---------------------------------------------------------------------------

pub fn view(state: &HistoryState) -> Element<'_, Message> {
    if let Some(detail) = &state.selected {
        return detail_view(detail);
    }

    let count = match state.games.len() {
        1 => "1 game recorded".to_string(),
        n => format!("{n} games recorded"),
    };

    let header = page_header(
        "Game History".to_string(),
        count,
        "Back",
        Message::GoHome,
    );

    let body: Element<Message> = if state.games.is_empty() {
        empty_state(
            "No games yet",
            "Finish a game and its box score lands here.",
        )
    } else {
        scrollable(
            column(
                state
                    .games
                    .iter()
                    .map(game_row)
                    .collect::<Vec<Element<Message>>>(),
            )
            .spacing(style::GAP_SM)
            // Keeps the last row clear of the screen edge when scrolled.
            .padding(iced::Padding::ZERO.right(f32::from(style::GAP))),
        )
        .height(Length::Fill)
        .into()
    };

    container(
        column![header, body]
            .spacing(style::GAP)
            .padding(style::GAP),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// One past game, read left to right: when it was, who took it and with
/// what, how it was won, how long it ran, how many sat down.
fn game_row(g: &GameSummary) -> Element<'_, Message> {
    let (winner, winner_sub, winner_ink) = match (&g.winner_name, &g.winner_commander) {
        (Some(name), Some(commander)) => (name.clone(), commander.clone(), style::ACCENT_BRIGHT),
        (Some(name), None) => (name.clone(), "Commander not recorded".to_string(), style::ACCENT_BRIGHT),
        _ => (
            "No winner".to_string(),
            "Game left unfinished".to_string(),
            style::TEXT_MUTED,
        ),
    };

    button(
        row![
            stacked(
                g.started_at.format("%-d %b %Y").to_string(),
                style::T_LABEL,
                style::TEXT,
                g.started_at.format("%H:%M").to_string(),
                Length::Fixed(WHEN_W),
                Alignment::Start,
            ),
            stacked(
                winner,
                style::T_SUBHEAD,
                winner_ink,
                winner_sub,
                Length::Fill,
                Alignment::Start,
            ),
            stacked(
                g.win_reason.map(|r| r.label()).unwrap_or("Unrecorded").to_string(),
                style::T_BODY,
                style::TEXT,
                "win condition".to_string(),
                Length::Fixed(HOW_W),
                Alignment::Start,
            ),
            count_cell(g.ending_turn.to_string(), "turns", COUNT_W),
            count_cell(g.pod_size.to_string(), "players", COUNT_W),
        ]
        .spacing(style::GAP)
        .align_y(Alignment::Center),
    )
    .padding([style::GAP, style::GAP + style::GAP_SM])
    .width(Length::Fill)
    .style(style::row_button)
    .on_press(Message::History(HistoryMessage::ViewGame(g.id)))
    .into()
}

// ---------------------------------------------------------------------------
// One game's box score
// ---------------------------------------------------------------------------

fn detail_view(detail: &GameDetail) -> Element<'_, Message> {
    let header = page_header(
        detail.started_at.format("%-d %b %Y").to_string(),
        format!(
            "Started {} \u{00b7} ran {}",
            detail.started_at.format("%H:%M"),
            duration_label(
                detail
                    .ended_at
                    .signed_duration_since(detail.started_at)
                    .num_minutes()
            ),
        ),
        "Back to list",
        Message::History(HistoryMessage::Back),
    );

    let winner = detail
        .seats
        .iter()
        .find(|s| s.won)
        .map(|s| (s.player_name.clone(), s.commander_name.clone()));

    let (winner_main, winner_sub, winner_ink) = match winner {
        Some((name, commander)) => (name, commander, style::ACCENT_BRIGHT),
        None => (
            "No winner".to_string(),
            "Game left unfinished".to_string(),
            style::TEXT_MUTED,
        ),
    };

    // The four facts that describe the game itself, in one panel, so the
    // seats below are read as detail rather than as more of the same.
    let summary = container(
        row![
            stacked(
                winner_main,
                style::T_HEADING,
                winner_ink,
                winner_sub,
                Length::Fill,
                Alignment::Start,
            ),
            stacked(
                detail.win_reason.map(|r| r.label()).unwrap_or("Unrecorded").to_string(),
                style::T_BODY,
                style::TEXT,
                "win condition".to_string(),
                Length::Fixed(HOW_W),
                Alignment::Start,
            ),
            count_cell(detail.ending_turn.to_string(), "turns", COUNT_W),
            count_cell(detail.seats.len().to_string(), "players", COUNT_W),
        ]
        .spacing(style::GAP)
        .align_y(Alignment::Center),
    )
    .padding([style::GAP, style::GAP + style::GAP_SM])
    .width(Length::Fill)
    .style(style::panel);

    let kill_lines: Vec<Element<Message>> = if detail.kills.is_empty() {
        vec![text("No commander hate logged this game.")
            .size(style::T_BODY)
            .color(style::TEXT_MUTED)
            .into()]
    } else {
        detail
            .kills
            .iter()
            .map(|k| {
                let tail = match &k.killer {
                    Some(killer) => format!("{} {}", k.kind.past_tense(), killer),
                    None => format!("{} nobody in particular", k.kind.past_tense()),
                };
                row![
                    container(text(k.victim.clone()).size(style::T_BODY))
                        .width(Length::Fixed(OUT_W / 2.0)),
                    text(tail).size(style::T_BODY).color(style::TEXT_MUTED),
                ]
                .spacing(style::GAP)
                .align_y(Alignment::Center)
                .into()
            })
            .collect()
    };

    let hate = container(column(kill_lines).spacing(style::GAP_SM))
        .padding([style::GAP, style::GAP + style::GAP_SM])
        .width(Length::Fill)
        .style(style::panel);

    let seats = column(
        detail
            .seats
            .iter()
            .map(seat_row)
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(style::GAP_SM)
    .padding(iced::Padding::ZERO.right(f32::from(style::GAP)));

    container(
        column![
            header,
            section("RESULT", summary),
            section("COMMANDER HATE", hate),
            section("SEATS", scrollable(seats).height(Length::Fill)).height(Length::Fill),
        ]
        .spacing(style::GAP)
        .padding(style::GAP),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// One seat's line in the box score: who they were, how they went out, and
/// what they finished on. The winner's panel is the only lit thing here.
fn seat_row(s: &GameDetailSeat) -> Element<'_, Message> {
    let (out_main, out_sub) = match &s.out {
        Some(out) => (
            format!("{} on turn {}", sentence_case(out.cause.past_tense()), out.turn),
            match &out.killer_name {
                Some(killer) => format!("credited to {killer}"),
                None => "nobody credited".to_string(),
            },
        ),
        None if s.won => ("Took the game".to_string(), "last one standing".to_string()),
        None => ("Survived".to_string(), "still in at the end".to_string()),
    };

    let damage = if s.damage_taken.is_empty() {
        "No commander damage taken".to_string()
    } else {
        format!(
            "Commander damage: {}",
            s.damage_taken
                .iter()
                .map(|(name, amt)| format!("{name} {amt}"))
                .collect::<Vec<_>>()
                .join("  \u{00b7}  ")
        )
    };

    let top = row![
        stacked(
            s.player_name.clone(),
            style::T_SUBHEAD,
            if s.won { style::ACCENT_BRIGHT } else { style::TEXT },
            s.commander_name.clone(),
            Length::Fill,
            Alignment::Start,
        ),
        stacked(
            out_main,
            style::T_BODY,
            style::TEXT,
            out_sub,
            Length::Fixed(OUT_W),
            Alignment::Start,
        ),
        count_cell(s.final_life.to_string(), "life", TOTAL_W),
        count_cell(s.final_poison.to_string(), "poison", TOTAL_W),
    ]
    .spacing(style::GAP)
    .align_y(Alignment::Center);

    container(
        column![
            top,
            text(damage).size(style::T_CAPTION).color(style::TEXT_MUTED),
        ]
        .spacing(style::GAP_SM),
    )
    .padding([style::GAP, style::GAP + style::GAP_SM])
    .width(Length::Fill)
    .style(if s.won { style::panel_active } else { style::panel })
    .into()
}
