use iced::widget::{column, container, row, scrollable, text};
use iced::{Element, Length};
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

/// One line of a stats table: a label that takes the slack, plus fixed-width
/// value columns so everything lines up down the screen.
fn stat_row<'a>(label: String, sub: Option<String>, values: Vec<(String, f32)>) -> Element<'a, Message> {
    let mut left = column![text(label).size(style::T_SUBHEAD)].spacing(2);
    if let Some(sub) = sub {
        left = left.push(text(sub).size(style::T_CAPTION));
    }

    let mut line = row![container(left).width(Length::Fill)]
        .spacing(16)
        .align_y(iced::Alignment::Center);

    for (value, width) in values {
        line = line.push(text(value).size(style::T_ACTION).width(Length::Fixed(width)));
    }

    container(line)
        .padding([12, 20])
        .width(Length::Fill)
        .style(style::panel)
        .into()
}

fn empty_note<'a>(note: &'a str) -> Element<'a, Message> {
    container(text(note).size(style::T_LABEL))
        .padding(24)
        .width(Length::Fill)
        .style(style::panel)
        .into()
}

fn section<'a>(title: &'a str, rows: Vec<Element<'a, Message>>, note: &'a str) -> Element<'a, Message> {
    let body: Element<Message> = if rows.is_empty() {
        empty_note(note)
    } else {
        column(rows).spacing(10).into()
    };
    column![text(title).size(style::T_SUBHEAD), body].spacing(12).into()
}

pub fn view(state: &StatsState) -> Element<'_, Message> {
    let header = container(
        row![
            text("Stats").size(style::T_TITLE),
            iced::widget::horizontal_space(),
            style::touch_button("Back", style::T_LABEL)
                .width(Length::Fixed(200.0))
                .style(style::secondary)
                .on_press(Message::GoHome),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(16)
    .width(Length::Fill)
    .style(style::header);

    let tabs = row(StatsTab::ALL
        .iter()
        .map(|t| {
            let selected = state.tab == *t;
            style::touch_button(t.label(), 22)
                .width(Length::Fill)
                .style(if selected {
                    style::primary
                } else {
                    style::secondary
                })
                .on_press(Message::Stats(StatsMessage::SwitchTab(*t)))
                .into()
        })
        .collect::<Vec<Element<Message>>>())
    .spacing(12);

    let body = match state.tab {
        StatsTab::Players => players_tab(state),
        StatsTab::Matchups => matchups_tab(state),
        StatsTab::Hate => hate_tab(state),
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

fn players_tab(state: &StatsState) -> Element<'_, Message> {
    let rows: Vec<Element<Message>> = state
        .players
        .iter()
        .map(|p| {
            let pct = if p.games > 0 {
                (p.wins as f64 / p.games as f64) * 100.0
            } else {
                0.0
            };
            stat_row(
                p.player_name.clone(),
                None,
                vec![
                    (format!("{} games", p.games), 150.0),
                    (format!("{} wins", p.wins), 140.0),
                    (format!("{pct:.0}%"), 90.0),
                ],
            )
        })
        .collect();

    let win_rows: Vec<Element<Message>> = state
        .win_reasons
        .iter()
        .map(|w| {
            stat_row(
                w.reason.label().to_string(),
                None,
                vec![(format!("{} games", w.games), 150.0)],
            )
        })
        .collect();

    let pace = match state.average_turns {
        Some(avg) => format!("Games run about {avg:.1} turns"),
        None => "No finished games yet".to_string(),
    };

    column![
        section("Win rates", rows, "No finished games yet."),
        section("How games end", win_rows, "No finished games yet."),
        container(text(pace).size(style::T_LABEL))
            .padding([12, 20])
            .width(Length::Fill)
            .style(style::panel),
    ]
    .spacing(style::GAP)
    .into()
}

fn matchups_tab(state: &StatsState) -> Element<'_, Message> {
    let rows: Vec<Element<Message>> = state
        .matchups
        .iter()
        .map(|m| {
            stat_row(
                format!("{} vs {}", m.commander_a, m.commander_b),
                None,
                vec![
                    (format!("{}-{}", m.a_wins, m.b_wins), 110.0),
                    (format!("{} games", m.games), 150.0),
                ],
            )
        })
        .collect();

    section(
        "Commander matchups",
        rows,
        "Play a few games and head-to-head records show up here.",
    )
}

fn hate_tab(state: &StatsState) -> Element<'_, Message> {
    let haters: Vec<Element<Message>> = state
        .haters
        .iter()
        .map(|h| {
            stat_row(
                h.player_name.clone(),
                Some(format!(
                    "{} kills \u{00b7} {} wipes \u{00b7} {} counters",
                    h.kills, h.wipes, h.counters
                )),
                vec![
                    (format!("{} total", h.total), 140.0),
                    (format!("{:.1}/game", h.per_game()), 130.0),
                ],
            )
        })
        .collect();

    let hated: Vec<Element<Message>> = state
        .hated
        .iter()
        .map(|c| {
            stat_row(
                c.commander_name.clone(),
                Some(format!(
                    "{} kills \u{00b7} {} wipes \u{00b7} {} counters",
                    c.kills, c.wipes, c.counters
                )),
                vec![
                    (format!("{} total", c.total), 140.0),
                    (format!("{:.1}/game", c.per_appearance()), 130.0),
                ],
            )
        })
        .collect();

    let grudges: Vec<Element<Message>> = state
        .grudges
        .iter()
        .map(|g| {
            stat_row(
                format!("{} \u{203a} {}", g.hater_name, g.commander_name),
                Some(format!("piloted by {}", g.victim_name)),
                vec![(format!("{}x", g.total), 110.0)],
            )
        })
        .collect();

    column![
        section(
            "Biggest haters",
            haters,
            "Nobody has thrown any hate yet - log it from a seat tile during a game.",
        ),
        section(
            "Most hated commanders",
            hated,
            "No commander has drawn heat yet.",
        ),
        section(
            "Grudges",
            grudges,
            "Repeat targets show up here once the hate starts flowing.",
        ),
    ]
    .spacing(style::GAP)
    .into()
}
