use iced::widget::{button, column, container, row, scrollable, text, Column};
use iced::{Alignment, Color, Element, Length};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::keyboard;
use crate::model::{GameDetail, GameDetailSeat, GameSummary, WinReason};
use crate::style;

pub struct HistoryState {
    pub feedback_panel: Option<crate::feedback::Panel>,
    pub games: Vec<GameSummary>,
    pub error: Option<String>,
    query: String,
    days: Option<i64>,
    terms: std::collections::HashMap<i64, String>,
    kb: keyboard::Keyboard<()>,
    editing: bool,
    confirming: bool,
    winner: Option<i64>,
    reason: Option<WinReason>,
    pub selected: Option<GameDetail>,
}

impl HistoryState {
    pub fn load(conn: &Connection) -> Self {
        let (games, terms, error) = match db::list_games(conn)
            .and_then(|g| db::history_search_terms(conn).map(|t| (g, t)))
        {
            Ok((g, t)) => (g, t, None),
            Err(e) => (
                Vec::new(),
                Default::default(),
                Some(format!("Couldn’t load history: {e}")),
            ),
        };
        Self {
            feedback_panel: None,
            games,
            terms,
            error,
            selected: None,
            query: String::new(),
            days: None,
            kb: keyboard::Keyboard::default(),
            editing: false,
            confirming: false,
            winner: None,
            reason: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum HistoryMessage {
    OpenFeedback(i64),
    CloseFeedback,
    ViewGame(i64),
    Retry,
    Query(String),
    Days(Option<i64>),
    Search,
    Key(keyboard::Key),
    Edit,
    Winner(Option<i64>),
    Reason(WinReason),
    Review,
    BackToEdit,
    SaveCorrection,
    CancelEdit,
    Back,
}

pub fn update(state: &mut HistoryState, conn: &Connection, message: HistoryMessage) {
    match message {
        HistoryMessage::OpenFeedback(_) => {}
        HistoryMessage::CloseFeedback => state.feedback_panel = None,
        HistoryMessage::Retry => *state = HistoryState::load(conn),
        HistoryMessage::Query(q) => state.query = q,
        HistoryMessage::Days(days) => state.days = days,
        HistoryMessage::Search => state.kb.open((), &state.query),
        HistoryMessage::Key(key) => {
            if state.kb.press(key, &mut state.query) == keyboard::Outcome::Submit {
                state.kb.close();
            }
        }
        HistoryMessage::Edit => {
            if let Some(detail) = &state.selected {
                state.winner = detail
                    .seats
                    .iter()
                    .find(|s| s.won)
                    .map(|s| s.game_player_id);
                state.reason = detail.win_reason;
                state.editing = true;
                state.confirming = false;
            }
        }
        HistoryMessage::Winner(winner) => {
            state.winner = winner;
            if winner.is_none() {
                state.reason = None;
            }
        }
        HistoryMessage::Reason(reason) => state.reason = Some(reason),
        HistoryMessage::Review => state.confirming = true,
        HistoryMessage::BackToEdit => state.confirming = false,
        HistoryMessage::CancelEdit => {
            state.editing = false;
            state.confirming = false;
            state.error = None;
        }
        HistoryMessage::SaveCorrection => {
            if state.confirming {
                if let Some(id) = state.selected.as_ref().map(|d| d.id) {
                    match db::correct_game_result(conn, id, state.winner, state.reason) {
                        Ok(()) => {
                            let query = state.query.clone();
                            let days = state.days;
                            *state = HistoryState::load(conn);
                            state.query = query;
                            state.days = days;
                            update(state, conn, HistoryMessage::ViewGame(id));
                        }
                        Err(e) => state.error = Some(e),
                    }
                }
            }
        }
        HistoryMessage::ViewGame(id) => match db::game_detail(conn, id) {
            Ok(detail) => {
                state.selected = Some(detail);
                state.error = None;
                state.kb.close();
            }
            Err(e) => state.error = Some(format!("Couldn’t open this game: {e}")),
        },
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

// Fixed column widths for the game list. A history screen is a table, so the
// columns are pinned rather than left to each row's content - dates, results
// and turn counts line up down the page and can be read in one sweep.
const WHEN_W: f32 = 160.0;
const HOW_W: f32 = 200.0;
const COUNT_W: f32 = 80.0;

// The same idea for a seat inside one game's box score.
const OUT_W: f32 = 320.0;
const TOTAL_W: f32 = 96.0;

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
    _back_label: &'a str,
    back: Message,
) -> Element<'a, Message> {
    style::page_header(title, sub, back)
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
    if let Some(panel) = &state.feedback_panel {
        return crate::feedback::view(panel, Message::History(HistoryMessage::CloseFeedback));
    }

    iced::widget::responsive(move |size| view_sized(state, size.width < 1100.)).into()
}

fn view_sized(state: &HistoryState, compact: bool) -> Element<'_, Message> {
    if let Some(detail) = &state.selected {
        if state.editing {
            return correction_view(state, detail);
        }
        return detail_view(detail, compact);
    }

    let count = match state.games.len() {
        1 => "1 game recorded".to_string(),
        n => format!("{n} games recorded"),
    };

    let header = page_header("Game History".to_string(), count, "Back", Message::GoHome);

    let body: Element<Message> = if state.games.is_empty() {
        empty_state(
            "No games yet",
            "Finish a game and its box score lands here.",
        )
    } else if !state.games.iter().any(|game| state.matches(game)) {
        empty_state(
            "No matching games",
            "Try another player or commander, or choose All dates.",
        )
    } else {
        scrollable(
            column(
                state
                    .games
                    .iter()
                    .filter(|game| state.matches(game))
                    .map(|game| game_row(game, compact))
                    .collect::<Vec<Element<Message>>>(),
            )
            .spacing(style::GAP_SM)
            // Keeps the last row clear of the screen edge when scrolled.
            .padding(iced::Padding::ZERO.right(f32::from(style::GAP))),
        )
        .height(Length::Fill)
        .into()
    };

    let input = iced::widget::mouse_area(
        iced::widget::text_input("Search any player or commander", &state.query)
            .on_input(|q| Message::History(HistoryMessage::Query(q)))
            .size(style::T_LABEL)
            .style(style::input)
            .padding(16),
    )
    .on_press(Message::History(HistoryMessage::Search));
    let dates = row([
        ("All dates", None),
        ("Last 30 days", Some(30)),
        ("Last 90 days", Some(90)),
        ("Last year", Some(365)),
    ]
    .into_iter()
    .map(|(label, days)| {
        style::touch_button(label, style::T_LABEL)
            .style(if state.days == days {
                style::primary
            } else {
                style::ghost
            })
            .on_press(Message::History(HistoryMessage::Days(days)))
            .into()
    }))
    .spacing(style::GAP_SM);
    let matched = state.games.iter().filter(|g| state.matches(g)).count();
    let mut page = column![
        header,
        input,
        dates,
        text(format!("{matched} matching games")).color(style::TEXT_MUTED)
    ]
    .spacing(style::GAP);
    if let Some(e) = &state.error {
        page = page.push(text(e).color(style::DANGER)).push(
            style::touch_button("Retry", style::T_LABEL)
                .on_press(Message::History(HistoryMessage::Retry)),
        );
    }
    page = page.push(body);
    if state.kb.field().is_some() {
        page = page.push(keyboard::view(
            &state.kb,
            |k| Message::History(HistoryMessage::Key(k)),
            Some("Filter"),
        ));
    }
    container(page.padding(style::GAP))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// One past game, read left to right: when it was, who took it and with
/// what, how it was won, how long it ran, how many sat down.
fn game_row(g: &GameSummary, compact: bool) -> Element<'_, Message> {
    let (winner, winner_sub, winner_ink) = match (&g.winner_name, &g.winner_commander) {
        (Some(name), Some(commander)) => (name.clone(), commander.clone(), style::ACCENT_BRIGHT),
        (Some(name), None) => (
            name.clone(),
            "Commander not recorded".to_string(),
            style::ACCENT_BRIGHT,
        ),
        _ => (
            "No winner".to_string(),
            "Game left unfinished".to_string(),
            style::TEXT_MUTED,
        ),
    };

    let winner = stacked(
        winner,
        style::T_SUBHEAD,
        winner_ink,
        winner_sub,
        Length::Fill,
        Alignment::Start,
    );
    let date = stacked(
        g.started_at.format("%-d %b %Y").to_string(),
        style::T_LABEL,
        style::TEXT,
        g.started_at.format("%H:%M").to_string(),
        Length::Fixed(WHEN_W),
        Alignment::Start,
    );
    let result = stacked(
        g.win_reason
            .map(|r| r.label())
            .unwrap_or("Unrecorded")
            .to_string(),
        style::T_BODY,
        style::TEXT,
        "win condition".into(),
        Length::Fixed(HOW_W),
        Alignment::Start,
    );
    let counts = row![
        count_cell(g.ending_turn.to_string(), "turns", COUNT_W),
        count_cell(g.pod_size.to_string(), "players", COUNT_W)
    ]
    .spacing(style::GAP);
    let content: Element<Message> = if compact {
        column![
            winner,
            row![date, result, counts].spacing(style::GAP).wrap()
        ]
        .spacing(style::GAP)
        .into()
    } else {
        row![date, winner, result, counts]
            .spacing(style::GAP)
            .align_y(Alignment::Center)
            .into()
    };
    button(content)
        .padding([style::GAP, style::GAP + style::GAP_SM])
        .width(Length::Fill)
        .style(style::row_button)
        .on_press(Message::History(HistoryMessage::ViewGame(g.id)))
        .into()
}

// ---------------------------------------------------------------------------
// One game's box score
// ---------------------------------------------------------------------------

fn detail_view(detail: &GameDetail, compact: bool) -> Element<'_, Message> {
    let header = page_header(
        detail.started_at.format("%-d %b %Y").to_string(),
        format!(
            "Started {} \u{00b7} ran {}",
            detail.started_at.format("%H:%M"),
            duration_label(
                detail
                    .elapsed_seconds
                    .map(|s| s as i64 / 60)
                    .unwrap_or_else(|| detail
                        .ended_at
                        .signed_duration_since(detail.started_at)
                        .num_minutes())
            ),
        ),
        "Back to list",
        Message::History(HistoryMessage::Back),
    );

    let header = row![
        header,
        style::touch_button("Refresh", style::T_LABEL)
            .width(120)
            .style(style::secondary)
            .on_press(Message::History(HistoryMessage::ViewGame(detail.id))),
        style::touch_button("Correct result", style::T_LABEL)
            .width(190)
            .style(style::secondary)
            .on_press(Message::History(HistoryMessage::Edit))
    ]
    .spacing(style::GAP)
    .align_y(Alignment::Center);

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
                if compact {
                    Length::Fixed(600.0)
                } else {
                    Length::Fill
                },
                Alignment::Start,
            ),
            stacked(
                detail
                    .win_reason
                    .map(|r| r.label())
                    .unwrap_or("Unrecorded")
                    .to_string(),
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
        .align_y(Alignment::Center)
        .wrap(),
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

    let mut responses =
        column![
            text("Player-reported impressions, separate from the recorded result.")
                .size(style::T_BODY)
                .color(style::TEXT_MUTED)
        ]
        .spacing(style::GAP);
    if detail.feedback.is_empty() {
        responses = responses.push(
            text("No feedback submitted yet. Refresh to check for new responses.")
                .size(style::T_BODY),
        );
    }
    if !detail.feedback_players.is_empty() {
        let links = row(detail
            .feedback_players
            .iter()
            .map(|(id, name)| {
                style::touch_button(format!("Feedback link · {name}"), style::T_BODY)
                    .width(Length::Shrink)
                    .style(style::secondary)
                    .on_press(Message::History(HistoryMessage::OpenFeedback(*id)))
                    .into()
            })
            .collect::<Vec<Element<Message>>>())
        .spacing(style::GAP_SM)
        .wrap();
        responses = responses.push(links);
    }
    for response in &detail.feedback {
        let mut entry = column![
            text(format!("{} · {}/5", response.player_name, response.rating))
                .size(style::T_SUBHEAD),
            text(format!(
                "Problem player: {}",
                response
                    .problem_player
                    .as_deref()
                    .unwrap_or("None / not sure")
            ))
            .size(style::T_BODY),
            text(format!(
                "Kingmaker: {}",
                response.kingmaker.as_deref().unwrap_or("None / not sure")
            ))
            .size(style::T_BODY),
        ]
        .spacing(style::GAP_SM);
        if !response.notes.is_empty() {
            entry = entry.push(text(&response.notes).size(style::T_BODY));
        }
        entry = entry.push(
            text(format!(
                "Updated {}",
                chrono::DateTime::parse_from_rfc3339(&response.updated_at)
                    .map(|time| time
                        .with_timezone(&chrono::Local)
                        .format("%b %-d · %H:%M")
                        .to_string())
                    .unwrap_or_else(|_| response.updated_at.clone())
            ))
            .size(style::T_CAPTION)
            .color(style::TEXT_MUTED),
        );
        responses = responses.push(
            container(entry)
                .padding(style::GAP)
                .width(Length::Fill)
                .style(style::panel),
        );
    }

    let seats = column(
        detail
            .seats
            .iter()
            .map(|seat| seat_row(seat, compact))
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(style::GAP_SM)
    .padding(iced::Padding::ZERO.right(f32::from(style::GAP)));

    container(
        column![
            header,
            scrollable(
                column![
                    section("RESULT", summary),
                    section("COMMANDER HATE", hate),
                    section("PLAYER FEEDBACK", responses),
                    section("SEATS", seats)
                ]
                .spacing(style::GAP)
            )
            .height(Length::Fill),
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
fn seat_row(s: &GameDetailSeat, compact: bool) -> Element<'_, Message> {
    let (out_main, out_sub) = match &s.out {
        Some(out) => (
            format!(
                "{} on turn {}",
                sentence_case(out.cause.past_tense()),
                out.turn
            ),
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
            if s.won {
                style::ACCENT_BRIGHT
            } else {
                style::TEXT
            },
            s.commander_name.clone(),
            if compact {
                Length::Fixed(600.0)
            } else {
                Length::Fill
            },
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
    .align_y(Alignment::Center)
    .wrap();

    container(
        column![
            top,
            text(damage).size(style::T_CAPTION).color(style::TEXT_MUTED),
        ]
        .spacing(style::GAP_SM),
    )
    .padding([style::GAP, style::GAP + style::GAP_SM])
    .width(Length::Fill)
    .style(if s.won {
        style::panel_active
    } else {
        style::panel
    })
    .into()
}

impl HistoryState {
    fn matches(&self, game: &GameSummary) -> bool {
        let query = self.query.trim().to_lowercase();
        let matches_name =
            query.is_empty() || self.terms.get(&game.id).is_some_and(|t| t.contains(&query));
        matches_name
            && self.days.is_none_or(|days| {
                game.started_at >= chrono::Utc::now() - chrono::Duration::days(days)
            })
    }
}
fn correction_view<'a>(state: &'a HistoryState, detail: &'a GameDetail) -> Element<'a, Message> {
    let header = style::page_header(
        if state.confirming {
            "Review corrected result"
        } else {
            "Correct game result"
        },
        "Choose the winner and how the game ended",
        Message::History(HistoryMessage::CancelEdit),
    );
    let mut body = column![text("Your statistics will use this result. The original result is kept in case it needs to be checked.").size(style::T_BODY).color(style::TEXT_MUTED)].spacing(style::GAP);
    if state.confirming {
        let winner = state
            .winner
            .and_then(|id| detail.seats.iter().find(|s| s.game_player_id == id))
            .map(|s| s.player_name.as_str())
            .unwrap_or("No winner / unresolved");
        body = body.push(
            container(
                column![
                    text("CORRECTED RESULT")
                        .size(style::T_CAPTION)
                        .color(style::ACCENT_BRIGHT),
                    text(winner).size(style::T_TITLE),
                    text(state.reason.map(|r| r.label()).unwrap_or("Unresolved"))
                        .size(style::T_SUBHEAD)
                ]
                .spacing(style::GAP),
            )
            .padding(24)
            .width(Length::Fill)
            .style(style::panel),
        );
    } else {
        body = body.push(text("Winner").size(style::T_SUBHEAD));
        let mut choices: Vec<Element<'a, Message>> = detail
            .seats
            .iter()
            .map(|seat| {
                button(
                    column![
                        text(&seat.player_name).size(style::T_SUBHEAD),
                        text(&seat.commander_name)
                            .size(style::T_CAPTION)
                            .color(style::TEXT_MUTED)
                    ]
                    .spacing(6),
                )
                .padding(16)
                .width(Length::Fill)
                .height(100)
                .style(if state.winner == Some(seat.game_player_id) {
                    style::tile_selected
                } else {
                    style::secondary
                })
                .on_press(Message::History(HistoryMessage::Winner(Some(
                    seat.game_player_id,
                ))))
                .into()
            })
            .collect();
        choices.push(
            style::touch_button("No winner / unresolved", style::T_LABEL)
                .width(Length::Fill)
                .height(100)
                .style(if state.winner.is_none() {
                    style::tile_selected
                } else {
                    style::secondary
                })
                .on_press(Message::History(HistoryMessage::Winner(None)))
                .into(),
        );
        let mut choices = choices.into_iter();
        loop {
            let batch: Vec<_> = choices.by_ref().take(2).collect();
            if batch.is_empty() {
                break;
            }
            body = body.push(row(batch).spacing(style::GAP));
        }
        if state.winner.is_some() {
            body = body.push(text("Ending reason").size(style::T_SUBHEAD));
            for reasons in WinReason::ALL.chunks(2) {
                body = body.push(
                    row(reasons.iter().map(|&reason| {
                        style::touch_button(reason.label(), style::T_LABEL)
                            .width(Length::Fill)
                            .height(64)
                            .style(if state.reason == Some(reason) {
                                style::tile_selected
                            } else {
                                style::secondary
                            })
                            .on_press(Message::History(HistoryMessage::Reason(reason)))
                            .into()
                    }))
                    .spacing(style::GAP),
                );
            }
        }
    }
    if let Some(e) = &state.error {
        body = body.push(text(e).color(style::DANGER));
    }
    let back = style::touch_button(
        if state.confirming {
            "Change selection"
        } else {
            "Cancel"
        },
        style::T_ACTION,
    )
    .width(Length::Fill)
    .style(style::secondary)
    .on_press(Message::History(if state.confirming {
        HistoryMessage::BackToEdit
    } else {
        HistoryMessage::CancelEdit
    }));
    let mut next = style::touch_button(
        if state.confirming {
            "Save corrected result"
        } else {
            "Review change"
        },
        style::T_ACTION,
    )
    .width(Length::Fill)
    .style(style::primary);
    if state.winner.is_none() || state.reason.is_some() {
        next = next.on_press(Message::History(if state.confirming {
            HistoryMessage::SaveCorrection
        } else {
            HistoryMessage::Review
        }));
    }
    container(
        column![
            header,
            scrollable(body.padding(iced::Padding::ZERO.right(12))).height(Length::Fill),
            row![back, next].spacing(style::GAP)
        ]
        .spacing(style::GAP)
        .padding(24)
        .max_width(1100),
    )
    .center_x(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn corrections_update_stats_keep_audit_and_search_losing_players() {
        let (mut conn, mut game) = crate::session::tests::fixture();
        game.pending_winner = Some(0);
        game.pending_reason = Some(WinReason::CombatDamage);
        let _ = crate::screens::game::update(
            &mut game,
            &mut conn,
            crate::screens::game::GameMessage::ConfirmEndGame,
        );
        let mut state = HistoryState::load(&conn);
        let id = state.games[0].id;
        update(
            &mut state,
            &conn,
            HistoryMessage::Query("Bo commander".into()),
        );
        assert!(state.matches(&state.games[0]));
        update(&mut state, &conn, HistoryMessage::ViewGame(id));
        let winner = state.selected.as_ref().unwrap().seats[1].game_player_id;
        update(&mut state, &conn, HistoryMessage::Edit);
        update(&mut state, &conn, HistoryMessage::Winner(Some(winner)));
        update(&mut state, &conn, HistoryMessage::Reason(WinReason::Poison));
        update(&mut state, &conn, HistoryMessage::SaveCorrection);
        assert_eq!(
            db::list_games(&conn).unwrap()[0].winner_name.as_deref(),
            Some("Ada"),
            "confirmation is required"
        );
        update(&mut state, &conn, HistoryMessage::Review);
        update(&mut state, &conn, HistoryMessage::SaveCorrection);
        assert_eq!(
            state.selected.as_ref().unwrap().win_reason,
            Some(WinReason::Poison)
        );
        let stats = crate::screens::stats::data::Data::load(&conn).unwrap();
        assert_eq!(
            stats
                .summary(crate::screens::stats::data::Scope::Players)
                .records
                .iter()
                .find(|r| r.identity.name == "Bo")
                .unwrap()
                .wins,
            1
        );
        let audit: String = conn
            .query_row("SELECT before_json FROM game_result_edits", [], |r| {
                r.get(0)
            })
            .unwrap();
        let before: GameDetail = serde_json::from_str(&audit).unwrap();
        assert!(before.seats[0].won);
        assert!(db::correct_game_result(&conn, id, Some(99999), Some(WinReason::Other)).is_err());
        assert_eq!(
            db::list_games(&conn).unwrap()[0].winner_name.as_deref(),
            Some("Bo")
        );
        update(&mut state, &conn, HistoryMessage::Back);
        update(&mut state, &conn, HistoryMessage::Days(Some(30)));
        assert!(state.matches(&state.games[0]));
        state.games[0].started_at = chrono::Utc::now() - chrono::Duration::days(31);
        assert!(!state.matches(&state.games[0]));
    }
    #[test]
    fn load_errors_are_explicit() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(HistoryState::load(&conn).error.is_some());
        assert!(crate::screens::home::HomeState::load(&conn).error.is_some());
        assert!(crate::screens::players::PlayersState::load(&conn)
            .error
            .is_some());
    }
}
