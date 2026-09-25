pub mod data;

use crate::{app::Message, keyboard, style};
use data::{Data, Identity, Record, Scope};
use iced::widget::{column, container, mouse_area, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length};
use rusqlite::Connection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsTab {
    Overview,
    Wins,
    Rivalries,
    Hate,
    Trends,
    AllData,
}
impl StatsTab {
    const ALL: [Self; 6] = [
        Self::Overview,
        Self::Wins,
        Self::Rivalries,
        Self::Hate,
        Self::Trends,
        Self::AllData,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Wins => "Wins",
            Self::Rivalries => "Rivalries",
            Self::Hate => "Hate",
            Self::Trends => "Game trends",
            Self::AllData => "All data",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Table,
    Chart,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Wins,
    Losses,
    Rate,
    Games,
}
impl Sort {
    fn label(self) -> &'static str {
        match self {
            Self::Wins => "Most wins",
            Self::Losses => "Most losses",
            Self::Rate => "Win rate",
            Self::Games => "Most played",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dataset {
    Wins,
    Rivalries,
    Hate,
    Games,
    Seats,
    Events,
}
impl Dataset {
    const ALL: [Self; 6] = [
        Self::Wins,
        Self::Rivalries,
        Self::Hate,
        Self::Games,
        Self::Seats,
        Self::Events,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Wins => "Wins",
            Self::Rivalries => "Matchups",
            Self::Hate => "Hate",
            Self::Games => "Games",
            Self::Seats => "Appearances",
            Self::Events => "Hate events",
        }
    }
}
pub struct StatsState {
    pictures: std::collections::HashMap<i64, iced::widget::image::Handle>,
    commanders: std::collections::HashMap<i64, crate::model::Commander>,
    pub tab: StatsTab,
    pub scope: Scope,
    pub display: Display,
    pub dataset: Dataset,
    pub sort: Sort,
    pub minimum: u32,
    pub query: String,
    pub received: bool,
    pub hate_rate: bool,
    pub data: Data,
    pub error: Option<String>,
    kb: keyboard::Keyboard<()>,
}
impl StatsState {
    pub fn load(conn: &Connection) -> Self {
        let (data, error) = match Data::load(conn) {
            Ok(data) => (data, None),
            Err(e) => (
                Data::default(),
                Some(format!("Couldn't load statistics: {e}")),
            ),
        };
        Self {
            pictures: crate::screens::players::load_pictures(conn),
            commanders: crate::db::all_commanders(conn)
                .unwrap_or_default()
                .into_iter()
                .map(|c| (c.id, c))
                .collect(),
            tab: StatsTab::Overview,
            scope: Scope::Players,
            display: Display::Table,
            dataset: Dataset::Wins,
            sort: Sort::Wins,
            minimum: 0,
            query: String::new(),
            received: true,
            hate_rate: false,
            data,
            error,
            kb: keyboard::Keyboard::default(),
        }
    }
    fn matches(&self, text: &str) -> bool {
        text.to_lowercase()
            .contains(&self.query.trim().to_lowercase())
    }
    fn matches_identity(&self, identity: &Identity) -> bool {
        self.matches(&format!("{} {}", identity.name, identity.detail))
    }
    fn records(&self) -> Vec<&Record> {
        let mut records: Vec<_> = self
            .data
            .summary(self.scope)
            .records
            .iter()
            .filter(|r| r.appearances >= self.minimum && self.matches_identity(&r.identity))
            .collect();
        records.sort_by(|a, b| {
            let order = match self.sort {
                Sort::Wins => b.wins.cmp(&a.wins),
                Sort::Losses => b.losses.cmp(&a.losses),
                Sort::Rate => b.rate().total_cmp(&a.rate()),
                Sort::Games => b.appearances.cmp(&a.appearances),
            };
            order
                .then(b.wins.cmp(&a.wins))
                .then(b.appearances.cmp(&a.appearances))
                .then(a.identity.name.cmp(&b.identity.name))
        });
        records
    }
}
#[derive(Debug, Clone)]
pub enum StatsMessage {
    Retry,
    SwitchTab(StatsTab),
    Scope(Scope),
    Leaderboard(Scope),
    Display(Display),
    Dataset(Dataset),
    Sort(Sort),
    Minimum(u32),
    Query(String),
    Search,
    Key(keyboard::Key),
    Received(bool),
    HateRate(bool),
    Explore(Dataset),
}
pub fn update(state: &mut StatsState, message: StatsMessage) {
    match message {
        StatsMessage::Retry => {}
        StatsMessage::SwitchTab(tab) => {
            state.tab = tab;
            state.kb.close();
            state.query.clear();
        }
        StatsMessage::Scope(scope) => state.scope = scope,
        StatsMessage::Leaderboard(scope) => {
            state.scope = scope;
            state.tab = StatsTab::Wins;
            state.sort = Sort::Wins;
            state.minimum = 0;
            state.query.clear();
        }
        StatsMessage::Display(display) => state.display = display,
        StatsMessage::Dataset(dataset) => {
            state.dataset = dataset;
            state.query.clear();
        }
        StatsMessage::Sort(sort) => state.sort = sort,
        StatsMessage::Minimum(minimum) => state.minimum = minimum,
        StatsMessage::Query(query) => state.query = query,
        StatsMessage::Search => state.kb.open((), &state.query),
        StatsMessage::Key(key) => {
            if state.kb.press(key, &mut state.query) == keyboard::Outcome::Submit {
                state.kb.close();
            }
        }
        StatsMessage::Received(received) => state.received = received,
        StatsMessage::HateRate(rate) => state.hate_rate = rate,
        StatsMessage::Explore(dataset) => {
            state.tab = StatsTab::AllData;
            state.dataset = dataset;
            state.query.clear();
            state.minimum = 0;
            state.kb.close();
        }
    }
}
fn choose<'a>(
    label: impl Into<String>,
    selected: bool,
    message: StatsMessage,
) -> Element<'a, Message> {
    let main_tab = matches!(message, StatsMessage::SwitchTab(_));
    style::touch_button(label.into(), style::T_LABEL)
        .width(Length::Fill)
        .height(56)
        .style(if selected && main_tab {
            style::primary
        } else if selected {
            style::tile_selected
        } else {
            style::ghost
        })
        .on_press(Message::Stats(message))
        .into()
}

fn segments<'a>(items: Vec<Element<'a, Message>>, compact: bool) -> Element<'a, Message> {
    let count = if compact && items.len() > 3 {
        3
    } else {
        items.len().max(1)
    };
    let mut rows = Vec::new();
    let mut items = items.into_iter();
    loop {
        let batch: Vec<_> = items.by_ref().take(count).collect();
        if batch.is_empty() {
            break;
        }
        rows.push(row(batch).spacing(4).into());
    }
    container(column(rows).spacing(4))
        .padding(4)
        .width(Length::Fill)
        .style(style::panel)
        .into()
}
fn note<'a>(value: impl Into<String>) -> Element<'a, Message> {
    text(value.into())
        .size(style::T_CAPTION)
        .color(style::TEXT_MUTED)
        .into()
}
fn section<'a>(
    title: impl Into<String>,
    description: impl Into<String>,
    body: Element<'a, Message>,
) -> Element<'a, Message> {
    column![
        text(title.into()).size(style::T_LEAD),
        note(description),
        body
    ]
    .spacing(12)
    .into()
}
fn empty<'a>(title: &str, detail: &str) -> Element<'a, Message> {
    container(column![text(title.to_string()).size(style::T_LEAD), note(detail)].spacing(12))
        .padding(28)
        .width(Length::Fill)
        .style(style::panel)
        .into()
}
fn metric<'a>(value: impl Into<String>, label: impl Into<String>) -> Element<'a, Message> {
    container(
        column![
            text(value.into()).size(34).color(style::ACCENT_BRIGHT),
            note(label)
        ]
        .spacing(8),
    )
    .padding(20)
    .width(Length::Fill)
    .style(style::panel)
    .into()
}
fn metric_grid<'a>(items: Vec<Element<'a, Message>>, compact: bool) -> Element<'a, Message> {
    let count = if compact { 2 } else { items.len().max(1) };
    let mut items = items.into_iter();
    let mut rows = Vec::new();
    loop {
        let batch: Vec<_> = items.by_ref().take(count).collect();
        if batch.is_empty() {
            break;
        }
        rows.push(row(batch).spacing(12).into());
    }
    column(rows).spacing(12).into()
}
fn explore<'a>(label: &str, dataset: Dataset) -> Element<'a, Message> {
    style::touch_button(label.to_string(), style::T_LABEL)
        .style(style::secondary)
        .on_press(Message::Stats(StatsMessage::Explore(dataset)))
        .into()
}
fn identity_text(i: &Identity) -> String {
    if i.detail.is_empty() {
        i.name.clone()
    } else {
        format!("{} · {}", i.name, i.detail)
    }
}
fn record_name<'a>(i: &Identity) -> Element<'a, Message> {
    column![
        text(i.name.clone()).size(style::T_SUBHEAD),
        note(i.detail.clone())
    ]
    .spacing(4)
    .width(Length::Fill)
    .into()
}
fn rate(value: f64) -> String {
    format!("{:.1}%", value * 100.0)
}
fn win_rate(r: &Record) -> String {
    if r.decided() == 0 {
        "—".into()
    } else {
        rate(r.rate())
    }
}
fn meter<'a>(fraction: f64) -> Element<'a, Message> {
    let fill = (fraction.clamp(0.0, 1.0) * 1000.0).round() as u16;
    let mut parts = Vec::new();
    if fill > 0 {
        parts.push(
            container(Space::new(Length::Fill, Length::Fill))
                .width(Length::FillPortion(fill))
                .style(style::meter_fill)
                .into(),
        );
    }
    if fill < 1000 {
        parts.push(Space::new(Length::FillPortion(1000 - fill), Length::Fill).into());
    }
    container(row(parts).height(14))
        .width(Length::Fill)
        .clip(true)
        .style(style::meter_track)
        .into()
}
struct DataRow {
    name: String,
    detail: String,
    values: Vec<String>,
    action: Option<Message>,
}
impl DataRow {
    fn new(name: impl Into<String>, detail: impl Into<String>, values: Vec<String>) -> Self {
        Self {
            name: name.into(),
            detail: detail.into(),
            values,
            action: None,
        }
    }
}
fn table<'a>(heads: &[&str], rows: Vec<DataRow>, compact: bool) -> Element<'a, Message> {
    if rows.is_empty() {
        return empty(
            "No matching records",
            "Try a different grouping, search, or minimum-games filter.",
        );
    }
    let mut content = column![].spacing(8);
    if !compact {
        content = content.push(
            container(
                row(
                    std::iter::once(container(note("Name / context")).width(Length::Fill).into())
                        .chain(heads.iter().map(|h| container(note(*h)).width(84).into())),
                )
                .spacing(12),
            )
            .padding([8, 16]),
        );
    }
    for r in rows {
        let name = column![text(r.name).size(style::T_SUBHEAD), note(r.detail)]
            .spacing(4)
            .width(Length::Fill);
        let cells: Vec<_> = r
            .values
            .into_iter()
            .zip(heads)
            .map(|(v, h)| {
                let value = text(v).size(style::T_LABEL);
                if compact {
                    container(column![note(*h), value].spacing(4))
                        .width(Length::Fill)
                        .into()
                } else {
                    container(value.align_x(iced::alignment::Horizontal::Right))
                        .width(84)
                        .into()
                }
            })
            .collect();
        let line: Element<_> = if compact {
            column![name, row(cells).spacing(8)].spacing(12).into()
        } else {
            row(std::iter::once(name.into()).chain(cells))
                .spacing(12)
                .align_y(Alignment::Center)
                .into()
        };
        let item: Element<_> = if let Some(action) = r.action {
            iced::widget::button(line)
                .padding(16)
                .width(Length::Fill)
                .style(style::row_button)
                .on_press(action)
                .into()
        } else {
            container(line)
                .padding(16)
                .width(Length::Fill)
                .style(style::table_row)
                .into()
        };
        content = content.push(item);
    }
    content.into()
}
fn chart<'a>(rows: Vec<(String, String, f64, String)>) -> Element<'a, Message> {
    chart_scaled(rows, None)
}
fn chart_scaled<'a>(
    rows: Vec<(String, String, f64, String)>,
    fixed_max: Option<f64>,
) -> Element<'a, Message> {
    if rows.is_empty() {
        return empty(
            "Nothing to chart yet",
            "Recorded games will build this view.",
        );
    }
    let max = fixed_max
        .unwrap_or_else(|| rows.iter().map(|r| r.2).fold(0.0_f64, f64::max))
        .max(1.0);
    column(
        rows.into_iter()
            .map(|(name, detail, value, label)| {
                container(
                    column![
                        row![
                            text(name).size(style::T_SUBHEAD).width(Length::Fill),
                            text(label)
                                .size(style::T_ACTION)
                                .color(style::ACCENT_BRIGHT)
                        ]
                        .spacing(16),
                        note(detail),
                        meter(value / max),
                    ]
                    .spacing(12),
                )
                .padding(20)
                .width(Length::Fill)
                .style(style::panel)
                .into()
            })
            .collect::<Vec<_>>(),
    )
    .spacing(12)
    .into()
}

pub fn view<'a>(state: &'a StatsState, images: &'a crate::cards::Images) -> Element<'a, Message> {
    iced::widget::responsive(move |size| view_sized(state, size.width < 1100.0, images)).into()
}
fn view_sized<'a>(
    state: &'a StatsState,
    compact: bool,
    images: &'a crate::cards::Images,
) -> Element<'a, Message> {
    let header = style::page_header(
        "Table Stats",
        "Your pod, across every game",
        Message::GoHome,
    );
    let tabs = segments(
        StatsTab::ALL
            .into_iter()
            .map(|t| choose(t.label(), state.tab == t, StatsMessage::SwitchTab(t)))
            .collect(),
        compact,
    );
    let mut page = column![header, tabs].spacing(16);
    let scoped = matches!(
        state.tab,
        StatsTab::Wins | StatsTab::Rivalries | StatsTab::Hate
    ) || (state.tab == StatsTab::AllData
        && matches!(
            state.dataset,
            Dataset::Wins | Dataset::Rivalries | Dataset::Hate
        ));
    if state.tab == StatsTab::AllData {
        page = page.push(segments(
            Dataset::ALL
                .into_iter()
                .map(|d| choose(d.label(), state.dataset == d, StatsMessage::Dataset(d)))
                .collect(),
            compact,
        ));
    }
    if scoped {
        page = page.push(segments(
            Scope::ALL
                .into_iter()
                .map(|s| choose(s.label(), state.scope == s, StatsMessage::Scope(s)))
                .collect(),
            false,
        ));
    }
    let body = if let Some(error) = &state.error {
        column![
            empty("Statistics unavailable", error),
            choose("Retry", false, StatsMessage::Retry)
        ]
        .spacing(16)
        .into()
    } else if state.data.games.is_empty() {
        empty(
            "The story starts with your first game",
            "Finish a game to see player wins, commander records, rivalries and table trends.",
        )
    } else {
        match state.tab {
            StatsTab::Overview => overview(state, compact, images),
            StatsTab::Wins => wins(state, compact),
            StatsTab::Rivalries => rivalries(state, compact, false),
            StatsTab::Hate => hate(state, compact),
            StatsTab::Trends => trends(state, compact),
            StatsTab::AllData => match state.dataset {
                Dataset::Wins => wins(state, compact),
                Dataset::Rivalries => rivalries(state, compact, true),
                Dataset::Hate => hate(state, compact),
                Dataset::Games | Dataset::Seats | Dataset::Events => ledger(state, compact),
            },
        }
    };
    page = page.push(
        scrollable(
            container(body)
                .padding(iced::Padding::ZERO.right(16))
                .width(Length::Fill),
        )
        .height(Length::Fill),
    );
    if state.kb.field().is_some() {
        page = page.push(keyboard::view(
            &state.kb,
            |k| Message::Stats(StatsMessage::Key(k)),
            Some("Filter"),
        ));
    }
    container(page.padding(24))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
fn filters<'a>(
    state: &StatsState,
    with_minimum: bool,
    with_chart: bool,
    compact: bool,
) -> Element<'a, Message> {
    let input = mouse_area(
        text_input("Filter names or commanders…", &state.query)
            .size(style::T_BODY)
            .padding(18)
            .style(style::input)
            .on_input(|q| Message::Stats(StatsMessage::Query(q)))
            .on_submit(Message::Stats(StatsMessage::Key(keyboard::Key::Done))),
    )
    .on_release(Message::Stats(StatsMessage::Search));
    let mut options = Vec::new();
    if with_minimum {
        options.extend([0, 3, 5].into_iter().map(|n| {
            choose(
                if n == 0 {
                    "Any sample".into()
                } else {
                    format!("{n}+ games")
                },
                state.minimum == n,
                StatsMessage::Minimum(n),
            )
        }));
    }
    if with_chart {
        options.extend([Display::Table, Display::Chart].into_iter().map(|d| {
            choose(
                if d == Display::Table {
                    "Table"
                } else {
                    "Chart"
                },
                state.display == d,
                StatsMessage::Display(d),
            )
        }));
    }
    if options.is_empty() {
        return input.into();
    }
    let controls = segments(options, false);
    if compact {
        column![input, controls].spacing(12).into()
    } else {
        row![
            input,
            container(controls).width(if with_minimum && with_chart { 660 } else { 400 })
        ]
        .spacing(16)
        .align_y(Alignment::Center)
        .into()
    }
}

fn overview<'a>(
    state: &'a StatsState,
    compact: bool,
    images: &'a crate::cards::Images,
) -> Element<'a, Message> {
    let data = &state.data;
    let mut leaders = Vec::new();
    for scope in Scope::ALL {
        let rows = &data.summary(scope).records;
        let card = if let Some(r) = rows.first().filter(|r| r.wins > 0) {
            container(
                column![
                    note(format!("MOST WINS · {}", scope.label().to_uppercase())),
                    row![
                        leader_images(state, &r.identity, images),
                        record_name(&r.identity)
                    ]
                    .spacing(16)
                    .height(80)
                    .align_y(Alignment::Center),
                    text(format!(
                        "{} {}",
                        r.wins,
                        if r.wins == 1 { "win" } else { "wins" }
                    ))
                    .size(36)
                    .color(style::ACCENT_BRIGHT),
                    note(format!(
                        "{} decided games · {} win rate",
                        r.decided(),
                        win_rate(r)
                    )),
                    note(if r.decided() < 5 {
                        "Small sample · fewer than 5 decided games"
                    } else {
                        "Based on this entry’s recorded results"
                    }),
                    choose("Explore wins", false, StatsMessage::Leaderboard(scope))
                ]
                .spacing(14),
            )
            .padding(24)
            .width(Length::Fill)
            .style(style::panel)
            .into()
        } else {
            empty(
                &format!("{} leaders", scope.label()),
                "No wins recorded yet",
            )
        };
        leaders.push(card);
    }
    // Each leader opens the corresponding complete leaderboard.
    let leaders: Element<_> = if compact {
        column(leaders).spacing(12).into()
    } else {
        row(leaders).spacing(16).into()
    };
    let most_played = data
        .summary(Scope::Pilots)
        .records
        .iter()
        .max_by_key(|r| r.appearances);
    let rivals = data
        .summary(Scope::Players)
        .rivalries
        .iter()
        .filter(|r| r.close_rivalry())
        .max_by_key(|r| r.shared);
    let highlights = column![
        text("Around the table").size(style::T_LEAD),
        note(
            most_played
                .map(|r| format!(
                    "Most played pairing: {} · {} appearances",
                    identity_text(&r.identity),
                    r.appearances
                ))
                .unwrap_or_default()
        ),
        note(
            rivals
                .map(|r| format!(
                    "Closest regular rivalry: {} vs {} · {}–{} across {} shared games",
                    r.a.name, r.b.name, r.a_wins, r.b_wins, r.shared
                ))
                .unwrap_or_else(|| {
                    "Rivalries appear after at least three shared games and a win on each side."
                        .into()
                })
        ),
    ]
    .spacing(12);
    column![
        metric_grid(vec![metric(data.games.len().to_string(),"recorded games"),metric(data.resolved_games().to_string(),"games with a winner"),metric(data.summary(Scope::Players).records.len().to_string(),"players with appearances"),metric(data.summary(Scope::Commanders).records.len().to_string(),"commander configurations")],compact),
        leaders,highlights,
        note("Wins are credited to the actual pilot. Partner pairs count as one commander configuration. Win rates use only that entry’s appearances in games with a recorded winner."),
    ].spacing(24).into()
}
fn wins(state: &StatsState, compact: bool) -> Element<'_, Message> {
    let records = state.records();
    let rows = records
        .iter()
        .map(|r| {
            DataRow::new(
                &r.identity.name,
                &r.identity.detail,
                vec![
                    r.wins.to_string(),
                    r.losses.to_string(),
                    r.appearances.to_string(),
                    r.unresolved.to_string(),
                    win_rate(r),
                    format!("{:+.1}", r.wins as f64 - r.expected_wins),
                ],
            )
        })
        .collect();
    let body = if state.display == Display::Table {
        table(
            &[
                "Wins",
                "Losses",
                "Played",
                "No winner",
                "Win rate",
                "vs baseline",
            ],
            rows,
            compact,
        )
    } else {
        chart_scaled(
            records
                .iter()
                .map(|r| {
                    let (value, label) = match state.sort {
                        Sort::Wins => (r.wins as f64, format!("{} wins", r.wins)),
                        Sort::Losses => (r.losses as f64, format!("{} losses", r.losses)),
                        Sort::Rate => (r.rate() * 100.0, win_rate(r)),
                        Sort::Games => (r.appearances as f64, format!("{} played", r.appearances)),
                    };
                    (
                        identity_text(&r.identity),
                        format!(
                            "{} wins · {} losses · {} played · {} no winner",
                            r.wins, r.losses, r.appearances, r.unresolved
                        ),
                        value,
                        label,
                    )
                })
                .collect(),
            if state.sort == Sort::Rate {
                Some(100.0)
            } else {
                None
            },
        )
    };
    section(format!("{} · wins", state.scope.label()), "Compare totals, participation and results for each entry.",
        column![
            filters(state, true, true, compact),
            segments([Sort::Wins, Sort::Losses, Sort::Rate, Sort::Games].into_iter().map(|s| choose(s.label(), state.sort == s, StatsMessage::Sort(s))).collect(), false),
            note(format!("{} matching records · Win rate excludes appearances without a recorded winner.", records.len())),
            body,
            note("Played counts seat appearances; a commander can occupy multiple seats in one game. Baseline is the sum of each seat’s equal-share chance to win, adjusted for pod size. The final column shows actual wins minus that baseline; it is not a skill rating."),
        ].spacing(16).into())
}
fn rivalry_row(r: &data::Rivalry) -> DataRow {
    DataRow::new(
        format!("{}  vs  {}", identity_text(&r.a), identity_text(&r.b)),
        "Only games where both were present",
        vec![
            r.a_wins.to_string(),
            r.b_wins.to_string(),
            r.others.to_string(),
            r.unresolved.to_string(),
            r.shared.to_string(),
        ],
    )
}
fn rivalry_cards<'a>(rows: Vec<&data::Rivalry>, dominance: bool) -> Element<'a, Message> {
    if rows.is_empty() {
        return empty(
            if dominance {
                "No repeated advantage yet"
            } else {
                "No established rivalry yet"
            },
            if dominance {
                "Needs at least three shared games, three wins on the leading side, at least half of all shared games won, and 75% of wins claimed by either side."
            } else {
                "Needs at least three shared games, a win on each side, and a win gap no larger than 34% of their combined wins."
            },
        );
    }
    column(
        rows.into_iter()
            .take(6)
            .map(|r| {
                let (a, b, aw, bw) = if dominance && r.b_wins > r.a_wins {
                    (&r.b, &r.a, r.b_wins, r.a_wins)
                } else {
                    (&r.a, &r.b, r.a_wins, r.b_wins)
                };
                container(
                    column![
                        text(format!("{}  vs  {}", identity_text(a), identity_text(b)))
                            .size(style::T_SUBHEAD),
                        row![
                            text(format!("{aw} – {bw}"))
                                .size(36)
                                .color(style::ACCENT_BRIGHT),
                            note(if dominance {
                                format!("{} more shared-table wins", aw - bw)
                            } else {
                                format!("{} wins apart", aw.abs_diff(bw))
                            })
                        ]
                        .spacing(20)
                        .align_y(Alignment::Center),
                        note(format!(
                            "{} shared games · {} won by someone else · {} unresolved/shared wins",
                            r.shared, r.others, r.unresolved
                        ))
                    ]
                    .spacing(12),
                )
                .padding(24)
                .width(Length::Fill)
                .style(style::panel)
                .into()
            })
            .collect::<Vec<_>>(),
    )
    .spacing(12)
    .into()
}
fn rivalries(state: &StatsState, compact: bool, all: bool) -> Element<'_, Message> {
    let rows: Vec<_> = state
        .data
        .summary(state.scope)
        .rivalries
        .iter()
        .filter(|r| {
            r.shared >= state.minimum
                && (state.matches_identity(&r.a) || state.matches_identity(&r.b))
        })
        .collect();
    let mut content=column![filters(state,true,all,compact),note("These are multiplayer table records, not duels or elimination counts. A win over a rival means winning a game they also played. Wins by a third participant stay separate; repeated commander decks count a shared game only once.")].spacing(20);
    if all {
        if state.display == Display::Table {
            content = content.push(table(
                &["A wins", "B wins", "Other wins", "Unresolved", "Shared"],
                rows.into_iter().map(rivalry_row).collect(),
                compact,
            ));
        } else {
            content=content.push(column(rows.into_iter().map(|r|container(column![text(format!("{} vs {}",identity_text(&r.a),identity_text(&r.b))).size(style::T_SUBHEAD),note(format!("A: {} wins · B: {} wins · {} other winners · {} unresolved · {} shared games",r.a_wins,r.b_wins,r.others,r.unresolved,r.shared)),note("A wins / shared games"),meter(data::ratio(r.a_wins,r.shared)),note("B wins / shared games"),meter(data::ratio(r.b_wins,r.shared))].spacing(10)).padding(20).style(style::panel).into()).collect::<Vec<_>>()).spacing(12));
        }
    } else {
        let mut close: Vec<_> = rows.iter().copied().filter(|r| r.close_rivalry()).collect();
        close.sort_by(|a, b| {
            a.margin()
                .cmp(&b.margin())
                .then(b.decided().cmp(&a.decided()))
        });
        let mut dominance: Vec<_> = rows.iter().copied().filter(|r| r.dominance()).collect();
        dominance.sort_by(|a, b| b.margin().cmp(&a.margin()).then(b.shared.cmp(&a.shared)));
        content=content.push(section("Neck and neck","Regular opponents trading wins.",rivalry_cards(close,false))).push(section("Toughest matchups","Repeated shared-table advantages. The leading entry is shown first; reversing the record shows the other side’s losses.",rivalry_cards(dominance,true))).push(explore("View every matchup",Dataset::Rivalries));
    }
    section(
        format!("{} · rivalries", state.scope.label()),
        format!("{} recorded matchups in this view", rows_len(state)),
        content.into(),
    )
}
fn rows_len(state: &StatsState) -> usize {
    state
        .data
        .summary(state.scope)
        .rivalries
        .iter()
        .filter(|r| {
            r.shared >= state.minimum
                && (state.matches_identity(&r.a) || state.matches_identity(&r.b))
        })
        .count()
}
fn hate(state: &StatsState, compact: bool) -> Element<'_, Message> {
    let counts = |r: &Record| {
        if state.received {
            r.received.clone()
        } else {
            r.given.clone()
        }
    };
    let value = |r: &Record| {
        if state.hate_rate {
            data::ratio(counts(r).total(), r.appearances)
        } else {
            counts(r).total() as f64
        }
    };
    let mut records = state.records();
    records.sort_by(|a, b| {
        value(b)
            .total_cmp(&value(a))
            .then(a.identity.name.cmp(&b.identity.name))
    });
    let body = if state.display == Display::Table {
        table(
            &["Kills", "Wipes", "Counters", "Total", "Played", "Per game"],
            records
                .iter()
                .map(|r| {
                    let c = counts(r);
                    DataRow::new(
                        &r.identity.name,
                        &r.identity.detail,
                        vec![
                            c.kills.to_string(),
                            c.wipes.to_string(),
                            c.counters.to_string(),
                            c.total().to_string(),
                            r.appearances.to_string(),
                            format!("{:.2}", data::ratio(c.total(), r.appearances)),
                        ],
                    )
                })
                .collect(),
            compact,
        )
    } else {
        chart(
            records
                .iter()
                .map(|r| {
                    (
                        identity_text(&r.identity),
                        format!(
                            "{} recorded events across {} appearances",
                            counts(r).total(),
                            r.appearances
                        ),
                        value(r),
                        if state.hate_rate {
                            format!("{:.2} / game", value(r))
                        } else {
                            format!("{} events", counts(r).total())
                        },
                    )
                })
                .collect(),
        )
    };
    let grudges = state
        .data
        .summary(state.scope)
        .grudges
        .iter()
        .filter(|g| {
            g.shared >= state.minimum
                && (state.matches_identity(&g.source) || state.matches_identity(&g.target))
        })
        .map(|g| {
            DataRow::new(
                format!(
                    "{} → {}",
                    identity_text(&g.source),
                    identity_text(&g.target)
                ),
                "Recorded targeting events",
                vec![
                    g.events.to_string(),
                    g.shared.to_string(),
                    format!("{:.2}", data::ratio(g.events, g.shared)),
                ],
            )
        })
        .collect();
    section(format!("{} · commander hate",state.scope.label()),"Compare activity and repeated targets, with participation shown alongside totals.",column![filters(state,true,true,compact),row![segments(vec![choose("Received",state.received,StatsMessage::Received(true)),choose("Given",!state.received,StatsMessage::Received(false))],false),segments(vec![choose("Total events",!state.hate_rate,StatsMessage::HateRate(false)),choose("Per appearance",state.hate_rate,StatsMessage::HateRate(true))],false)].spacing(12),note("Rates include all recorded appearances, including games without a winner. Wipes count each logged target, not distinct spells. Events without a credited source count only toward the recipient."),body,section("Who targets whom","Directed grudges use only the games both entries shared.",table(&["Events","Shared","Per shared"],grudges,compact))].spacing(20).into())
}
fn trends(state: &StatsState, compact: bool) -> Element<'_, Message> {
    let data = &state.data;
    let total = data.games.len().max(1) as f64;
    let turns = data.games.iter().map(|g| g.turns as f64).sum::<f64>() / total;
    let minutes = data.games.iter().map(|g| g.minutes).sum::<f64>() / total;
    let endings = data.endings();
    let ending_view = if state.display == Display::Table {
        table(
            &["Games", "Share"],
            endings
                .iter()
                .map(|(label, n)| {
                    DataRow::new(label, "", vec![n.to_string(), rate(*n as f64 / total)])
                })
                .collect(),
            compact,
        )
    } else {
        chart(
            endings
                .iter()
                .map(|(label, n)| {
                    (
                        label.clone(),
                        format!("{} of {} recorded games", n, data.games.len()),
                        *n as f64,
                        n.to_string(),
                    )
                })
                .collect(),
        )
    };
    let mut pods = std::collections::BTreeMap::<usize, Vec<&data::Game>>::new();
    for game in &data.games {
        let size = data.seats.iter().filter(|s| s.game == game.id).count();
        pods.entry(size).or_default().push(game);
    }
    let pod_rows = pods
        .iter()
        .map(|(size, games)| {
            DataRow::new(
                format!("{size}-player games"),
                "Actual recorded participants",
                vec![
                    games.len().to_string(),
                    format!(
                        "{:.1}",
                        games.iter().map(|g| g.turns as f64).sum::<f64>() / games.len() as f64
                    ),
                    format!(
                        "{:.0}",
                        games.iter().map(|g| g.minutes).sum::<f64>() / games.len() as f64
                    ),
                ],
            )
        })
        .collect();
    let pod_view = if state.display == Display::Table {
        table(&["Games", "Avg turns", "Avg min"], pod_rows, compact)
    } else {
        chart(
            pods.iter()
                .map(|(size, games)| {
                    let turns =
                        games.iter().map(|g| g.turns as f64).sum::<f64>() / games.len() as f64;
                    let minutes = games.iter().map(|g| g.minutes).sum::<f64>() / games.len() as f64;
                    (
                        format!("{size}-player games"),
                        format!("{} games · {minutes:.0} average minutes", games.len()),
                        turns,
                        format!("{turns:.1} average turns"),
                    )
                })
                .collect(),
        )
    };
    column![metric_grid(vec![metric(format!("{turns:.1}"),"average recorded ending turn"),metric(format!("{minutes:.0} min"),"average recorded duration"),metric((data.games.len()-data.resolved_games()).to_string(),"games without a winner")],compact),segments(vec![choose("Tables",state.display==Display::Table,StatsMessage::Display(Display::Table)),choose("Charts",state.display==Display::Chart,StatsMessage::Display(Display::Chart))],false),section("How games end","Every recorded game, including unspecified endings and games without a winner.",ending_view),section("Pod size and pace","Compare similar tables before comparing game length. Averages include every recorded game in the group.",pod_view),explore("Browse every game",Dataset::Games)].spacing(24).into()
}
fn ledger(state: &StatsState, compact: bool) -> Element<'_, Message> {
    let data = &state.data;
    let game_button = |id| {
        Some(Message::Home(crate::screens::home::HomeMessage::ViewGame(
            id,
        )))
    };
    let (heads, rows): (Vec<&str>, Vec<DataRow>) = match state.dataset {
        Dataset::Games => (
            vec!["Players", "Turn", "Minutes"],
            data.games
                .iter()
                .filter_map(|g| {
                    let seats: Vec<_> = data.seats.iter().filter(|s| s.game == g.id).collect();
                    let winners: Vec<_> = seats
                        .iter()
                        .filter(|s| s.won)
                        .map(|s| identity_text(&s.identity(Scope::Pilots)))
                        .collect();
                    let detail = format!(
                        "{} · {}",
                        if winners.is_empty() {
                            "No recorded winner".into()
                        } else {
                            format!("Won by {}", winners.join(", "))
                        },
                        g.reason
                            .as_ref()
                            .map(|r| crate::model::WinReason::from_db_str(r).label())
                            .unwrap_or("Unspecified ending")
                    );
                    let name = format!("Game #{} · {}", g.id, g.date.get(..10).unwrap_or(&g.date));
                    if !state.matches(&format!("{name} {detail}")) {
                        return None;
                    }
                    let mut row = DataRow::new(
                        name,
                        detail,
                        vec![
                            seats.len().to_string(),
                            g.turns.to_string(),
                            format!("{:.0}", g.minutes),
                        ],
                    );
                    row.action = game_button(g.id);
                    Some(row)
                })
                .collect(),
        ),
        Dataset::Seats => (
            vec!["Game", "Outcome"],
            data.seats
                .iter()
                .rev()
                .filter_map(|s| {
                    let label = identity_text(&s.identity(Scope::Pilots));
                    let detail = s
                        .owner_name
                        .as_ref()
                        .map(|o| format!("Borrowed from {o}; result credited to {}", s.player_name))
                        .unwrap_or_else(|| "Played from this pilot's collection".into());
                    if !state.matches(&format!("{label} {detail} {}", s.game)) {
                        return None;
                    }
                    let resolved = data.seats.iter().any(|p| p.game == s.game && p.won);
                    let mut row = DataRow::new(
                        label,
                        detail,
                        vec![
                            s.game.to_string(),
                            if s.won {
                                "Win"
                            } else if resolved {
                                "Loss"
                            } else {
                                "No winner"
                            }
                            .into(),
                        ],
                    );
                    row.action = game_button(s.game);
                    Some(row)
                })
                .collect(),
        ),
        _ => (
            vec!["Game", "Kind"],
            data.events
                .iter()
                .rev()
                .filter_map(|e| {
                    let target = data.seats.iter().find(|s| s.id == e.target)?;
                    let source = e
                        .source
                        .and_then(|id| data.seats.iter().find(|s| s.id == id))
                        .map(|s| identity_text(&s.identity(Scope::Pilots)))
                        .unwrap_or_else(|| "Uncredited source".into());
                    let label = format!(
                        "{source} → {}",
                        identity_text(&target.identity(Scope::Pilots))
                    );
                    if !state.matches(&format!("{label} {} {}", e.game, e.kind.label())) {
                        return None;
                    }
                    let mut row = DataRow::new(
                        label,
                        "One logged target event",
                        vec![e.game.to_string(), e.kind.label().into()],
                    );
                    row.action = game_button(e.game);
                    Some(row)
                })
                .collect(),
        ),
    };
    section(
        format!("{} · underlying records", state.dataset.label()),
        format!(
            "{} matching rows. Tap a row to open that game's full history.",
            rows.len()
        ),
        column![
            filters(state, false, false, compact),
            table(&heads, rows, compact)
        ]
        .spacing(16)
        .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> StatsState {
        let conn = Connection::open_in_memory().unwrap();
        let mut state = StatsState::load(&conn);
        state.error = None;
        state.data = Data::from_rows(
            vec![],
            vec![
                data::Appearance {
                    id: 1,
                    game: 1,
                    player: 1,
                    player_name: "Ava".into(),
                    commander: 10,
                    commander_name: "The Ur-Dragon".into(),
                    partner: None,
                    partner_name: None,
                    won: true,
                    owner_name: None,
                },
                data::Appearance {
                    id: 2,
                    game: 1,
                    player: 2,
                    player_name: "Ben".into(),
                    commander: 20,
                    commander_name: "Atraxa".into(),
                    partner: None,
                    partner_name: None,
                    won: false,
                    owner_name: None,
                },
            ],
            vec![],
        );
        state
    }
    #[test]
    fn leader_links_and_full_data_reset_filters_and_select_the_right_group() {
        let mut state = state();
        state.query = "does not match".into();
        state.minimum = 5;
        update(&mut state, StatsMessage::Leaderboard(Scope::Pilots));
        assert_eq!(state.tab, StatsTab::Wins);
        assert_eq!(state.scope, Scope::Pilots);
        assert_eq!(state.records().len(), 2);
        update(&mut state, StatsMessage::Explore(Dataset::Rivalries));
        assert_eq!(state.tab, StatsTab::AllData);
        assert_eq!(state.dataset, Dataset::Rivalries);
        assert_eq!(state.scope, Scope::Pilots);
    }
    #[test]
    fn pilot_search_and_loss_sort_use_the_complete_records() {
        let mut state = state();
        state.scope = Scope::Pilots;
        update(&mut state, StatsMessage::Sort(Sort::Losses));
        assert_eq!(state.records()[0].identity.name, "Ben");
        update(&mut state, StatsMessage::Query("UR-DRAGON".into()));
        assert_eq!(state.records().len(), 1);
        assert_eq!(state.records()[0].identity.name, "Ava");
        update(&mut state, StatsMessage::Minimum(3));
        assert!(state.records().is_empty());
    }
    #[test]
    fn stats_keyboard_can_filter_and_close_without_mutating_data() {
        let mut state = state();
        update(&mut state, StatsMessage::Search);
        update(&mut state, StatsMessage::Key(keyboard::Key::Char('b')));
        assert_eq!(state.records().len(), 1);
        update(&mut state, StatsMessage::Key(keyboard::Key::Done));
        assert!(state.kb.field().is_none());
        assert_eq!(state.data.seats.len(), 2);
    }
}

impl StatsState {
    pub fn art_task(&self) -> iced::Task<Message> {
        let ids: std::collections::BTreeSet<_> = Scope::ALL
            .into_iter()
            .filter_map(|s| self.data.summary(s).records.first())
            .flat_map(|r| r.identity.key.commanders.iter().copied())
            .collect();
        crate::screens::players::fetch_all(
            ids.into_iter()
                .filter_map(|id| {
                    self.commanders
                        .get(&id)
                        .and_then(|c| c.portrait_url())
                        .map(str::to_owned)
                })
                .collect(),
        )
    }
}
fn leader_images<'a>(
    state: &'a StatsState,
    identity: &'a Identity,
    images: &'a crate::cards::Images,
) -> Element<'a, Message> {
    let mut pictures = row![].spacing(8);
    if let Some(player) = identity.key.player {
        let photo: Element<_> = match state.pictures.get(&player) {
            Some(handle) => iced::widget::image(handle.clone())
                .width(64)
                .height(64)
                .content_fit(iced::ContentFit::Contain)
                .into(),
            None => container(
                text(
                    identity
                        .name
                        .chars()
                        .next()
                        .unwrap_or('?')
                        .to_uppercase()
                        .to_string(),
                )
                .size(style::T_TITLE)
                .color(style::ACCENT_BRIGHT),
            )
            .center_x(64)
            .center_y(64)
            .style(style::badge)
            .into(),
        };
        pictures = pictures.push(photo);
    }
    for id in &identity.key.commanders {
        if let Some(commander) = state.commanders.get(id) {
            pictures = pictures.push(crate::cards::portrait(commander, images, 72.));
        }
    }
    pictures.into()
}
