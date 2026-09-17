use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length, Task};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::model::{Commander, Player, Seat, StartingLife};
use crate::scryfall::{self, ScryfallCard};

pub const MIN_POD: usize = 2;
pub const MAX_POD: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Grid,
    List,
}

impl Layout {
    pub fn label(&self) -> &'static str {
        match self {
            Layout::Grid => "Grid",
            Layout::List => "List (pass the device)",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SeatSetup {
    pub player: Option<Player>,
    pub commander: Option<Commander>,
}

pub struct SetupState {
    pub pod_size: usize,
    pub layout: Layout,
    pub seats: Vec<SeatSetup>,
    pub active_seat: usize,
    pub new_player_name: String,
    pub commander_query: String,
    pub commander_results: Vec<ScryfallCard>,
    pub searching: bool,
    pub error: Option<String>,
}

impl SetupState {
    pub fn new() -> Self {
        let pod_size = 4;
        Self {
            pod_size,
            layout: Layout::Grid,
            seats: vec![SeatSetup::default(); pod_size],
            active_seat: 0,
            new_player_name: String::new(),
            commander_query: String::new(),
            commander_results: Vec::new(),
            searching: false,
            error: None,
        }
    }

    pub fn all_seats_ready(&self) -> bool {
        !self.seats.is_empty()
            && self
                .seats
                .iter()
                .all(|s| s.player.is_some() && s.commander.is_some())
    }
}

#[derive(Debug, Clone)]
pub enum SetupMessage {
    ChangePodSize(usize),
    ChangeLayout(Layout),
    SelectSeat(usize),
    NewPlayerNameChanged(String),
    CreatePlayer,
    PickExistingPlayer(Player),
    ClearSeatPlayer,
    CommanderQueryChanged(String),
    SearchCommanders,
    SearchResults(Result<Vec<ScryfallCard>, String>),
    PickCommander(ScryfallCard),
    PickCachedCommander(Commander),
    ClearSeatCommander,
    StartGame,
}

pub enum Action {
    StartGame(Vec<Seat>, Layout),
}

pub fn update(
    state: &mut SetupState,
    conn: &Connection,
    commanders_cache: &mut Vec<Commander>,
    players_cache: &mut Vec<Player>,
    message: SetupMessage,
) -> (Task<Message>, Option<Action>) {
    state.error = None;
    match message {
        SetupMessage::ChangePodSize(n) => {
            state.pod_size = n;
            state.seats.resize_with(n, SeatSetup::default);
            if state.active_seat >= n {
                state.active_seat = n - 1;
            }
            (Task::none(), None)
        }
        SetupMessage::ChangeLayout(layout) => {
            state.layout = layout;
            (Task::none(), None)
        }
        SetupMessage::SelectSeat(i) => {
            state.active_seat = i;
            state.commander_query.clear();
            state.commander_results.clear();
            (Task::none(), None)
        }
        SetupMessage::NewPlayerNameChanged(s) => {
            state.new_player_name = s;
            (Task::none(), None)
        }
        SetupMessage::CreatePlayer => {
            let name = state.new_player_name.trim().to_string();
            if name.is_empty() {
                return (Task::none(), None);
            }
            match db::create_player(conn, &name) {
                Ok(player) => {
                    players_cache.push(player.clone());
                    players_cache.sort_by_key(|p| p.name.to_lowercase());
                    state.seats[state.active_seat].player = Some(player);
                    state.new_player_name.clear();
                }
                Err(e) => state.error = Some(format!("Couldn't create player: {e}")),
            }
            (Task::none(), None)
        }
        SetupMessage::PickExistingPlayer(player) => {
            state.seats[state.active_seat].player = Some(player);
            (Task::none(), None)
        }
        SetupMessage::ClearSeatPlayer => {
            state.seats[state.active_seat].player = None;
            (Task::none(), None)
        }
        SetupMessage::CommanderQueryChanged(s) => {
            state.commander_query = s;
            (Task::none(), None)
        }
        SetupMessage::SearchCommanders => {
            let query = state.commander_query.clone();
            if query.trim().is_empty() {
                return (Task::none(), None);
            }
            state.searching = true;
            (
                Task::perform(scryfall::search_commanders(query), |res| {
                    Message::Setup(SetupMessage::SearchResults(res))
                }),
                None,
            )
        }
        SetupMessage::SearchResults(res) => {
            state.searching = false;
            match res {
                Ok(list) => state.commander_results = list,
                Err(e) => state.error = Some(format!("Scryfall search failed: {e}")),
            }
            (Task::none(), None)
        }
        SetupMessage::PickCommander(card) => {
            match db::upsert_commander(
                conn,
                &card.scryfall_id,
                &card.name,
                card.image_url.as_deref(),
                card.art_crop_url.as_deref(),
                &card.color_identity,
            ) {
                Ok(commander) => {
                    if !commanders_cache.iter().any(|c| c.id == commander.id) {
                        commanders_cache.push(commander.clone());
                        commanders_cache.sort_by_key(|c| c.name.to_lowercase());
                    }
                    let art_url = commander
                        .art_crop_url
                        .clone()
                        .or_else(|| commander.image_url.clone());
                    let id = commander.scryfall_id.clone();
                    state.seats[state.active_seat].commander = Some(commander);
                    state.commander_results.clear();
                    state.commander_query.clear();
                    let task = match art_url {
                        Some(url) => Task::perform(scryfall::fetch_image(url), move |res| {
                            Message::ArtLoaded(id.clone(), res)
                        }),
                        None => Task::none(),
                    };
                    (task, None)
                }
                Err(e) => {
                    state.error = Some(format!("Couldn't save commander: {e}"));
                    (Task::none(), None)
                }
            }
        }
        SetupMessage::PickCachedCommander(commander) => {
            let art_url = commander
                .art_crop_url
                .clone()
                .or_else(|| commander.image_url.clone());
            let id = commander.scryfall_id.clone();
            state.seats[state.active_seat].commander = Some(commander);
            let task = match art_url {
                Some(url) => Task::perform(scryfall::fetch_image(url), move |res| {
                    Message::ArtLoaded(id.clone(), res)
                }),
                None => Task::none(),
            };
            (task, None)
        }
        SetupMessage::ClearSeatCommander => {
            state.seats[state.active_seat].commander = None;
            (Task::none(), None)
        }
        SetupMessage::StartGame => {
            if state.all_seats_ready() {
                let seats: Vec<Seat> = state
                    .seats
                    .iter()
                    .map(|s| {
                        Seat::new(
                            s.player.clone().unwrap(),
                            s.commander.clone().unwrap(),
                            StartingLife::VALUE,
                        )
                    })
                    .collect();
                (Task::none(), Some(Action::StartGame(seats, state.layout)))
            } else {
                state.error = Some("Every seat needs a player and a commander.".into());
                (Task::none(), None)
            }
        }
    }
}

pub fn view<'a>(
    state: &'a SetupState,
    players_cache: &'a [Player],
    commanders_cache: &'a [Commander],
) -> Element<'a, Message> {
    let pod_size_row = row(
        (MIN_POD..=MAX_POD)
            .map(|n| {
                let selected = n == state.pod_size;
                button(text(n.to_string()).size(20))
                    .padding(12)
                    .style(if selected {
                        button::primary
                    } else {
                        button::secondary
                    })
                    .on_press(Message::Setup(SetupMessage::ChangePodSize(n)))
                    .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(8);

    let layout_row = row([Layout::Grid, Layout::List]
        .into_iter()
        .map(|l| {
            let selected = l == state.layout;
            button(text(l.label()).size(18))
                .padding(12)
                .style(if selected {
                    button::primary
                } else {
                    button::secondary
                })
                .on_press(Message::Setup(SetupMessage::ChangeLayout(l)))
                .into()
        })
        .collect::<Vec<Element<Message>>>())
    .spacing(8);

    let seat_tabs = row(
        (0..state.pod_size)
            .map(|i| {
                let seat = &state.seats[i];
                let label = match (&seat.player, &seat.commander) {
                    (Some(p), Some(c)) => format!("{} \u{2713}\n{}", p.name, c.name),
                    (Some(p), None) => format!("{}\n(pick commander)", p.name),
                    _ => format!("Seat {}", i + 1),
                };
                let selected = i == state.active_seat;
                button(text(label).size(16))
                    .padding(10)
                    .width(Length::Fixed(160.0))
                    .style(if selected {
                        button::primary
                    } else if seat.player.is_some() && seat.commander.is_some() {
                        button::success
                    } else {
                        button::secondary
                    })
                    .on_press(Message::Setup(SetupMessage::SelectSeat(i)))
                    .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(8)
    .wrap();

    let seat = &state.seats[state.active_seat];
    let editor: Element<Message> = if seat.player.is_none() {
        player_picker(state, players_cache)
    } else if seat.commander.is_none() {
        commander_picker(state, commanders_cache)
    } else {
        seat_summary(seat)
    };

    let mut start_button = button(text("Start Game").size(22)).padding(16);
    if state.all_seats_ready() {
        start_button = start_button
            .style(button::success)
            .on_press(Message::Setup(SetupMessage::StartGame));
    }

    let error_text: Element<Message> = match &state.error {
        Some(e) => text(e.clone()).size(16).into(),
        None => text("").into(),
    };

    container(
        column![
            text("Commander Pod").size(32),
            row![text("Players:").size(18), pod_size_row].spacing(12).align_y(iced::Alignment::Center),
            row![text("Layout:").size(18), layout_row].spacing(12).align_y(iced::Alignment::Center),
            scrollable(seat_tabs).width(Length::Fill),
            container(editor).padding(16).width(Length::Fill),
            error_text,
            row![
                start_button,
                button(text("Game History").size(18)).padding(12).on_press(Message::GoToHistory),
                button(text("Stats").size(18)).padding(12).on_press(Message::GoToStats),
            ]
            .spacing(12),
        ]
        .spacing(16)
        .padding(20),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn player_picker<'a>(state: &'a SetupState, players_cache: &'a [Player]) -> Element<'a, Message> {
    let existing = row(
        players_cache
            .iter()
            .map(|p| {
                button(text(p.name.clone()).size(18))
                    .padding(12)
                    .on_press(Message::Setup(SetupMessage::PickExistingPlayer(p.clone())))
                    .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(8)
    .wrap();

    column![
        text("Who's sitting here?").size(22),
        scrollable(existing).height(Length::Fixed(160.0)),
        row![
            text_input("New player name", &state.new_player_name)
                .size(18)
                .padding(10)
                .on_input(|s| Message::Setup(SetupMessage::NewPlayerNameChanged(s)))
                .on_submit(Message::Setup(SetupMessage::CreatePlayer)),
            button(text("Add Player").size(18))
                .padding(12)
                .on_press(Message::Setup(SetupMessage::CreatePlayer)),
        ]
        .spacing(8),
    ]
    .spacing(12)
    .into()
}

fn commander_picker<'a>(
    state: &'a SetupState,
    commanders_cache: &'a [Commander],
) -> Element<'a, Message> {
    let cached = row(
        commanders_cache
            .iter()
            .map(|c| {
                button(text(c.name.clone()).size(16))
                    .padding(10)
                    .on_press(Message::Setup(SetupMessage::PickCachedCommander(
                        c.clone(),
                    )))
                    .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(8)
    .wrap();

    let results = column(
        state
            .commander_results
            .iter()
            .map(|c| {
                button(text(format!("{}  [{}]", c.name, c.color_identity)).size(16))
                    .padding(10)
                    .width(Length::Fill)
                    .on_press(Message::Setup(SetupMessage::PickCommander(c.clone())))
                    .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(6);

    let search_label = if state.searching {
        "Searching Scryfall..."
    } else {
        "Search Scryfall"
    };

    column![
        text("Pick a commander").size(22),
        text("Previously used:").size(16),
        scrollable(cached).height(Length::Fixed(100.0)),
        row![
            text_input("Commander name", &state.commander_query)
                .size(18)
                .padding(10)
                .on_input(|s| Message::Setup(SetupMessage::CommanderQueryChanged(s)))
                .on_submit(Message::Setup(SetupMessage::SearchCommanders)),
            button(text(search_label).size(18))
                .padding(12)
                .on_press(Message::Setup(SetupMessage::SearchCommanders)),
        ]
        .spacing(8),
        scrollable(results).height(Length::Fixed(220.0)),
    ]
    .spacing(12)
    .into()
}

fn seat_summary(seat: &SeatSetup) -> Element<Message> {
    let player = seat.player.as_ref().unwrap();
    let commander = seat.commander.as_ref().unwrap();
    column![
        text(format!("{} is playing {}", player.name, commander.name)).size(22),
        text(format!("Color identity: {}", commander.color_identity)).size(16),
        row![
            button(text("Change Player").size(16))
                .padding(10)
                .on_press(Message::Setup(SetupMessage::ClearSeatPlayer)),
            button(text("Change Commander").size(16))
                .padding(10)
                .on_press(Message::Setup(SetupMessage::ClearSeatCommander)),
        ]
        .spacing(8),
    ]
    .spacing(12)
    .into()
}
