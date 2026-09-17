use std::collections::HashMap;

use iced::widget::{button, column, container, image, row, scrollable, text, text_input};
use iced::{Element, Length, Task};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::model::{Commander, Player, Seat, STARTING_LIFE};
use crate::scryfall::{self, ScryfallCard};
use crate::style;

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

/// The commander a player picked by name; art is chosen next, but this
/// identity (oracle id) is what stats will key on regardless of which
/// printing's art ends up chosen.
#[derive(Debug, Clone)]
pub struct ArtTarget {
    pub oracle_id: String,
    pub name: String,
    pub color_identity: String,
}

#[derive(Debug, Clone, Default)]
pub struct SeatSetup {
    pub player: Option<Player>,
    pub commander: Option<Commander>,
    /// Commanders this specific player has piloted before - personal to
    /// them, never shared with the rest of the pod.
    pub commander_history: Vec<Commander>,
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
    pub art_target: Option<ArtTarget>,
    pub art_options: Vec<ScryfallCard>,
    pub loading_art_options: bool,
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
            art_target: None,
            art_options: Vec::new(),
            loading_art_options: false,
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

    fn players_taken_by_other_seats(&self) -> std::collections::HashSet<i64> {
        self.seats
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self.active_seat)
            .filter_map(|(_, s)| s.player.as_ref().map(|p| p.id))
            .collect()
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
    PickCommanderName(ScryfallCard),
    PickHistoryCommander(Commander),
    ArtOptionsLoaded(Result<Vec<ScryfallCard>, String>),
    PickArt(ScryfallCard),
    CancelArtPick,
    ChangeArt,
    ClearSeatCommander,
    StartGame,
}

pub enum Action {
    StartGame(Vec<Seat>, Layout),
}

fn load_portrait_task(commander: &Commander) -> Task<Message> {
    match commander.portrait_url() {
        Some(url) => {
            let url = url.to_string();
            let key = url.clone();
            Task::perform(scryfall::fetch_image(url), move |res| {
                Message::ArtLoaded(key.clone(), res)
            })
        }
        None => Task::none(),
    }
}

fn load_prints_task(oracle_id: String) -> Task<Message> {
    Task::perform(scryfall::fetch_prints(oracle_id), |res| {
        Message::Setup(SetupMessage::ArtOptionsLoaded(res))
    })
}

pub fn update(
    state: &mut SetupState,
    conn: &Connection,
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
            state.art_target = None;
            state.art_options.clear();
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
                    state.seats[state.active_seat].player = Some(player);
                    state.seats[state.active_seat].commander_history = Vec::new();
                    state.new_player_name.clear();
                }
                Err(e) => state.error = Some(format!("Couldn't create player: {e}")),
            }
            (Task::none(), None)
        }
        SetupMessage::PickExistingPlayer(player) => {
            if state.players_taken_by_other_seats().contains(&player.id) {
                state.error = Some(format!("{} is already seated at this table.", player.name));
                return (Task::none(), None);
            }
            let history = db::player_commander_history(conn, player.id).unwrap_or_default();
            state.seats[state.active_seat].player = Some(player);
            state.seats[state.active_seat].commander_history = history;
            (Task::none(), None)
        }
        SetupMessage::ClearSeatPlayer => {
            state.seats[state.active_seat].player = None;
            state.seats[state.active_seat].commander_history.clear();
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
        SetupMessage::PickCommanderName(card) => {
            state.commander_results.clear();
            state.commander_query.clear();
            state.loading_art_options = true;
            let default_thumb_task = match card.small_url.clone() {
                Some(url) => {
                    let key = url.clone();
                    Task::perform(scryfall::fetch_image(url), move |res| {
                        Message::ArtLoaded(key.clone(), res)
                    })
                }
                None => Task::none(),
            };
            let prints_task = load_prints_task(card.oracle_id.clone());
            state.art_target = Some(ArtTarget {
                oracle_id: card.oracle_id.clone(),
                name: card.name.clone(),
                color_identity: card.color_identity.clone(),
            });
            state.art_options = vec![card];
            (Task::batch([prints_task, default_thumb_task]), None)
        }
        SetupMessage::PickHistoryCommander(commander) => {
            let task = load_portrait_task(&commander);
            state.seats[state.active_seat].commander = Some(commander);
            (task, None)
        }
        SetupMessage::ArtOptionsLoaded(res) => {
            state.loading_art_options = false;
            match res {
                Ok(list) => {
                    let thumb_tasks: Vec<Task<Message>> = list
                        .iter()
                        .filter_map(|c| c.small_url.clone())
                        .take(20)
                        .map(|url| {
                            let key = url.clone();
                            Task::perform(scryfall::fetch_image(url), move |res| {
                                Message::ArtLoaded(key.clone(), res)
                            })
                        })
                        .collect();
                    state.art_options = list;
                    (Task::batch(thumb_tasks), None)
                }
                Err(e) => {
                    state.error = Some(format!("Couldn't load printings: {e}"));
                    (Task::none(), None)
                }
            }
        }
        SetupMessage::PickArt(card) => {
            let Some(target) = state.art_target.take() else {
                return (Task::none(), None);
            };
            state.art_options.clear();
            match db::upsert_commander(
                conn,
                &target.oracle_id,
                &target.name,
                card.image_url.as_deref(),
                card.art_crop_url.as_deref(),
                &target.color_identity,
            ) {
                Ok(commander) => {
                    let task = load_portrait_task(&commander);
                    state.seats[state.active_seat].commander = Some(commander);
                    (task, None)
                }
                Err(e) => {
                    state.error = Some(format!("Couldn't save commander: {e}"));
                    (Task::none(), None)
                }
            }
        }
        SetupMessage::CancelArtPick => {
            state.art_target = None;
            state.art_options.clear();
            (Task::none(), None)
        }
        SetupMessage::ChangeArt => {
            let Some(commander) = state.seats[state.active_seat].commander.clone() else {
                return (Task::none(), None);
            };
            state.art_target = Some(ArtTarget {
                oracle_id: commander.oracle_id.clone(),
                name: commander.name.clone(),
                color_identity: commander.color_identity.clone(),
            });
            state.art_options.clear();
            state.loading_art_options = true;
            (load_prints_task(commander.oracle_id), None)
        }
        SetupMessage::ClearSeatCommander => {
            state.seats[state.active_seat].commander = None;
            state.art_target = None;
            state.art_options.clear();
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
                            STARTING_LIFE,
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
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let header = container(
        row![
            text("Commander Pod").size(30),
            iced::widget::horizontal_space(),
            button(text("Game History").size(16))
                .padding(10)
                .on_press(Message::GoToHistory),
            button(text("Stats").size(16))
                .padding(10)
                .on_press(Message::GoToStats),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
    )
    .padding(16)
    .width(Length::Fill)
    .style(style::header);

    let pod_size_row = row((MIN_POD..=MAX_POD)
        .map(|n| {
            let selected = n == state.pod_size;
            button(text(n.to_string()).size(18))
                .padding(10)
                .style(if selected {
                    button::primary
                } else {
                    button::secondary
                })
                .on_press(Message::Setup(SetupMessage::ChangePodSize(n)))
                .into()
        })
        .collect::<Vec<Element<Message>>>())
    .spacing(6);

    let layout_row = row([Layout::Grid, Layout::List]
        .into_iter()
        .map(|l| {
            let selected = l == state.layout;
            button(text(l.label()).size(16))
                .padding(10)
                .style(if selected {
                    button::primary
                } else {
                    button::secondary
                })
                .on_press(Message::Setup(SetupMessage::ChangeLayout(l)))
                .into()
        })
        .collect::<Vec<Element<Message>>>())
    .spacing(6);

    let config_card = container(
        row![
            column![text("Players").size(14), pod_size_row].spacing(6),
            column![text("Layout").size(14), layout_row].spacing(6),
        ]
        .spacing(32),
    )
    .padding(16)
    .width(Length::Fill)
    .style(style::panel);

    let seat_tabs = container(
        scrollable(
            row((0..state.pod_size)
                .map(|i| {
                    let seat = &state.seats[i];
                    let ready = seat.player.is_some() && seat.commander.is_some();
                    let label = match &seat.player {
                        Some(p) if ready => format!("{} \u{2713}", p.name),
                        Some(p) => p.name.clone(),
                        None => format!("Seat {}", i + 1),
                    };
                    let selected = i == state.active_seat;
                    button(text(label).size(16))
                        .padding(12)
                        .width(Length::Fixed(140.0))
                        .style(if selected {
                            button::primary
                        } else if ready {
                            button::success
                        } else {
                            button::secondary
                        })
                        .on_press(Message::Setup(SetupMessage::SelectSeat(i)))
                        .into()
                })
                .collect::<Vec<Element<Message>>>())
            .spacing(8)
            .wrap(),
        )
        .width(Length::Fill),
    )
    .padding(12)
    .width(Length::Fill)
    .style(style::panel);

    let seat = &state.seats[state.active_seat];
    let editor: Element<Message> = if state.art_target.is_some() {
        art_gallery(state, image_cache)
    } else if seat.player.is_none() {
        player_picker(state, players_cache)
    } else if seat.commander.is_none() {
        commander_picker(state, &seat.commander_history)
    } else {
        seat_summary(seat, image_cache)
    };

    let editor_card = container(editor)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::panel);

    let mut start_button = button(text("Start Game").size(22)).padding(18);
    if state.all_seats_ready() {
        start_button = start_button
            .style(button::success)
            .on_press(Message::Setup(SetupMessage::StartGame));
    }

    let mut content = column![header, config_card, seat_tabs, editor_card].spacing(14);

    if let Some(e) = &state.error {
        content = content.push(
            container(text(e.clone()).size(16))
                .padding(10)
                .width(Length::Fill)
                .style(style::panel_danger),
        );
    }

    content = content.push(start_button);

    container(content.padding(20))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn player_picker<'a>(state: &'a SetupState, players_cache: &'a [Player]) -> Element<'a, Message> {
    let taken = state.players_taken_by_other_seats();
    let available: Vec<&Player> = players_cache.iter().filter(|p| !taken.contains(&p.id)).collect();

    let existing = row(available
        .into_iter()
        .map(|p| {
            button(text(p.name.clone()).size(18))
                .padding(12)
                .on_press(Message::Setup(SetupMessage::PickExistingPlayer(p.clone())))
                .into()
        })
        .collect::<Vec<Element<Message>>>())
    .spacing(8)
    .wrap();

    column![
        text("Who's sitting here?").size(24),
        scrollable(existing).height(Length::Fixed(160.0)),
        row![
            text_input("New player name", &state.new_player_name)
                .size(18)
                .padding(10)
                .on_input(|s| Message::Setup(SetupMessage::NewPlayerNameChanged(s)))
                .on_submit(Message::Setup(SetupMessage::CreatePlayer)),
            button(text("Add Player").size(18))
                .padding(12)
                .style(button::primary)
                .on_press(Message::Setup(SetupMessage::CreatePlayer)),
        ]
        .spacing(8),
    ]
    .spacing(14)
    .into()
}

fn commander_picker<'a>(
    state: &'a SetupState,
    history: &'a [Commander],
) -> Element<'a, Message> {
    let history_row: Element<Message> = if history.is_empty() {
        text("No commanders played yet - search below to add one.")
            .size(14)
            .into()
    } else {
        row(history
            .iter()
            .map(|c| {
                button(text(c.name.clone()).size(16))
                    .padding(10)
                    .on_press(Message::Setup(SetupMessage::PickHistoryCommander(
                        c.clone(),
                    )))
                    .into()
            })
            .collect::<Vec<Element<Message>>>())
        .spacing(8)
        .wrap()
        .into()
    };

    let results = column(
        state
            .commander_results
            .iter()
            .map(|c| {
                button(text(format!("{}  [{}]", c.name, c.color_identity)).size(16))
                    .padding(10)
                    .width(Length::Fill)
                    .on_press(Message::Setup(SetupMessage::PickCommanderName(c.clone())))
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
        text("Pick a commander").size(24),
        text("This player's commanders:").size(15),
        scrollable(history_row).height(Length::Fixed(90.0)),
        row![
            text_input("Commander name", &state.commander_query)
                .size(18)
                .padding(10)
                .on_input(|s| Message::Setup(SetupMessage::CommanderQueryChanged(s)))
                .on_submit(Message::Setup(SetupMessage::SearchCommanders)),
            button(text(search_label).size(18))
                .padding(12)
                .style(button::primary)
                .on_press(Message::Setup(SetupMessage::SearchCommanders)),
        ]
        .spacing(8),
        scrollable(results).height(Length::Fixed(200.0)),
    ]
    .spacing(12)
    .into()
}

fn art_gallery<'a>(
    state: &'a SetupState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let target = state.art_target.as_ref().unwrap();

    let tiles: Vec<Element<Message>> = state
        .art_options
        .iter()
        .map(|card| {
            let thumb: Element<Message> = match card.small_url.as_deref().and_then(|u| image_cache.get(u)) {
                Some(handle) => image(handle.clone())
                    .width(Length::Fixed(150.0))
                    .height(Length::Fixed(110.0))
                    .into(),
                None => container(text("...").size(14))
                    .width(Length::Fixed(150.0))
                    .height(Length::Fixed(110.0))
                    .center_x(Length::Fixed(150.0))
                    .center_y(Length::Fixed(110.0))
                    .into(),
            };
            button(
                column![thumb, text(card.set_name.clone()).size(12)]
                    .spacing(4)
                    .align_x(iced::Alignment::Center),
            )
            .padding(6)
            .on_press(Message::Setup(SetupMessage::PickArt(card.clone())))
            .into()
        })
        .collect();

    let status = if state.loading_art_options {
        text("Loading every printing from Scryfall...").size(14)
    } else {
        text(format!("{} printings found", state.art_options.len())).size(14)
    };

    column![
        text(format!("Choose art for {}", target.name)).size(24),
        status,
        scrollable(row(tiles).spacing(10).wrap()).height(Length::Fixed(360.0)),
        button(text("Cancel").size(16))
            .padding(10)
            .on_press(Message::Setup(SetupMessage::CancelArtPick)),
    ]
    .spacing(12)
    .into()
}

fn seat_summary<'a>(
    seat: &'a SeatSetup,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let player = seat.player.as_ref().unwrap();
    let commander = seat.commander.as_ref().unwrap();

    let portrait: Element<Message> = match commander.portrait_url().and_then(|u| image_cache.get(u)) {
        Some(handle) => image(handle.clone())
            .width(Length::Fill)
            .height(Length::Fixed(200.0))
            .into(),
        None => container(text("Loading art...").size(16))
            .width(Length::Fill)
            .height(Length::Fixed(200.0))
            .center_x(Length::Fill)
            .center_y(Length::Fixed(200.0))
            .into(),
    };

    column![
        portrait,
        text(format!("{} is playing {}", player.name, commander.name)).size(24),
        text(format!("Color identity: {}", commander.color_identity)).size(16),
        row![
            button(text("Change Player").size(16))
                .padding(12)
                .on_press(Message::Setup(SetupMessage::ClearSeatPlayer)),
            button(text("Change Commander").size(16))
                .padding(12)
                .on_press(Message::Setup(SetupMessage::ClearSeatCommander)),
            button(text("Change Art").size(16))
                .padding(12)
                .on_press(Message::Setup(SetupMessage::ChangeArt)),
        ]
        .spacing(8),
    ]
    .spacing(14)
    .into()
}
