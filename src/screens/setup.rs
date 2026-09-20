use std::collections::HashMap;

use iced::widget::{button, column, container, image, row, scrollable, stack, text, text_input};
use iced::{Color, ContentFit, Element, Length, Task};
use rusqlite::Connection;

use crate::app::Message;
use crate::db;
use crate::layout::{self, TableLayout};
use crate::model::{Commander, Player, Seat, STARTING_LIFE};
use crate::scryfall::{self, ScryfallCard};
use crate::style;

pub const MIN_POD: usize = 2;
pub const MAX_POD: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStage {
    ChoosePodSize,
    ChooseLayout,
    Grid,
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
    pub stage: SetupStage,
    pub pod_size: usize,
    pub table_layout: Option<TableLayout>,
    pub seats: Vec<SeatSetup>,
    pub editing_seat: Option<usize>,
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
        Self {
            stage: SetupStage::ChoosePodSize,
            pod_size: 0,
            table_layout: None,
            seats: Vec::new(),
            editing_seat: None,
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
        let editing = self.editing_seat;
        self.seats
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != editing)
            .filter_map(|(_, s)| s.player.as_ref().map(|p| p.id))
            .collect()
    }

    fn clear_editor_fields(&mut self) {
        self.new_player_name.clear();
        self.commander_query.clear();
        self.commander_results.clear();
        self.art_target = None;
        self.art_options.clear();
    }
}

#[derive(Debug, Clone)]
pub enum SetupMessage {
    ChoosePodSize(usize),
    BackToPodSizeChoice,
    ChooseLayout(TableLayout),
    BackToLayoutChoice,
    EditSeat(usize),
    BackToGrid,
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
    StartGame(Vec<Seat>, TableLayout),
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
        SetupMessage::ChoosePodSize(n) => {
            state.pod_size = n;
            state.seats = vec![SeatSetup::default(); n];
            state.table_layout = None;
            state.stage = SetupStage::ChooseLayout;
            (Task::none(), None)
        }
        SetupMessage::BackToPodSizeChoice => {
            state.stage = SetupStage::ChoosePodSize;
            state.editing_seat = None;
            state.clear_editor_fields();
            (Task::none(), None)
        }
        SetupMessage::ChooseLayout(layout) => {
            state.table_layout = Some(layout);
            state.stage = SetupStage::Grid;
            (Task::none(), None)
        }
        SetupMessage::BackToLayoutChoice => {
            state.stage = SetupStage::ChooseLayout;
            state.editing_seat = None;
            state.clear_editor_fields();
            (Task::none(), None)
        }
        SetupMessage::EditSeat(i) => {
            state.editing_seat = Some(i);
            state.clear_editor_fields();
            (Task::none(), None)
        }
        SetupMessage::BackToGrid => {
            state.editing_seat = None;
            state.clear_editor_fields();
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
            let Some(seat) = state.editing_seat else {
                return (Task::none(), None);
            };
            match db::create_player(conn, &name) {
                Ok(player) => {
                    state.seats[seat].player = Some(player);
                    state.seats[seat].commander_history = Vec::new();
                    state.new_player_name.clear();
                }
                Err(e) => state.error = Some(format!("Couldn't create player: {e}")),
            }
            (Task::none(), None)
        }
        SetupMessage::PickExistingPlayer(player) => {
            let Some(seat) = state.editing_seat else {
                return (Task::none(), None);
            };
            if state.players_taken_by_other_seats().contains(&player.id) {
                state.error = Some(format!("{} is already seated at this table.", player.name));
                return (Task::none(), None);
            }
            let history = db::player_commander_history(conn, player.id).unwrap_or_default();
            state.seats[seat].player = Some(player);
            state.seats[seat].commander_history = history;
            (Task::none(), None)
        }
        SetupMessage::ClearSeatPlayer => {
            if let Some(seat) = state.editing_seat {
                state.seats[seat].player = None;
                state.seats[seat].commander_history.clear();
            }
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
            let Some(seat) = state.editing_seat else {
                return (Task::none(), None);
            };
            if let Some(player) = &state.seats[seat].player {
                let _ = db::record_player_commander_use(conn, player.id, commander.id);
            }
            let task = load_portrait_task(&commander);
            state.seats[seat].commander = Some(commander);
            state.editing_seat = None;
            state.clear_editor_fields();
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
            let Some(seat) = state.editing_seat else {
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
                    if let Some(player) = &state.seats[seat].player {
                        let _ = db::record_player_commander_use(conn, player.id, commander.id);
                    }
                    let task = load_portrait_task(&commander);
                    state.seats[seat].commander = Some(commander);
                    state.editing_seat = None;
                    state.clear_editor_fields();
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
            let Some(seat) = state.editing_seat else {
                return (Task::none(), None);
            };
            let Some(commander) = state.seats[seat].commander.clone() else {
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
            if let Some(seat) = state.editing_seat {
                state.seats[seat].commander = None;
            }
            state.art_target = None;
            state.art_options.clear();
            (Task::none(), None)
        }
        SetupMessage::StartGame => {
            let Some(layout) = state.table_layout.clone() else {
                state.error = Some("Pick a table layout first.".into());
                return (Task::none(), None);
            };
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
                (Task::none(), Some(Action::StartGame(seats, layout)))
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
    match state.stage {
        SetupStage::ChoosePodSize => pod_size_view(),
        SetupStage::ChooseLayout => layout_choice_view(state),
        SetupStage::Grid => {
            if state.editing_seat.is_some() {
                editor_overlay(state, players_cache, image_cache)
            } else {
                grid_view(state, image_cache)
            }
        }
    }
}

/// A small numbered-box diagram of a layout, using the same rendering logic
/// as the real board so it's an accurate preview, just shrunk down.
fn layout_preview(table: &TableLayout) -> Element<'static, Message> {
    container(layout::render_table(table, |idx| {
        container(text((idx + 1).to_string()).size(22))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(style::panel)
            .into()
    }))
    .width(Length::Fixed(300.0))
    .height(Length::Fixed(190.0))
    .into()
}

fn layout_choice_view(state: &SetupState) -> Element<'_, Message> {
    let options = layout::options_for(state.pod_size);

    let cards = row(options
        .into_iter()
        .map(|opt| {
            let label = opt.name.clone();
            button(
                column![layout_preview(&opt), text(label).size(22)]
                    .spacing(14)
                    .align_x(iced::Alignment::Center),
            )
            .padding(20)
            .style(button::secondary)
            .on_press(Message::Setup(SetupMessage::ChooseLayout(opt)))
            .into()
        })
        .collect::<Vec<Element<Message>>>())
    .spacing(24)
    .wrap();

    container(
        column![
            text("How are you sitting?").size(44),
            text(format!(
                "{} players - pick the arrangement that matches your table",
                state.pod_size
            ))
            .size(18),
            scrollable(cards).width(Length::Fill),
            style::touch_button("Back", 20)
                .width(Length::Fixed(240.0))
                .style(button::secondary)
                .on_press(Message::Setup(SetupMessage::BackToPodSizeChoice)),
        ]
        .spacing(28)
        .align_x(iced::Alignment::Center)
        .padding(style::GAP),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

fn pod_size_view<'a>() -> Element<'a, Message> {
    let buttons = row((MIN_POD..=MAX_POD)
        .map(|n| {
            button(
                container(text(n.to_string()).size(46))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .padding(0)
            .width(Length::Fixed(140.0))
            .height(Length::Fixed(140.0))
            .style(button::primary)
            .on_press(Message::Setup(SetupMessage::ChoosePodSize(n)))
            .into()
        })
        .collect::<Vec<Element<Message>>>())
    .spacing(20)
    .wrap();

    container(
        column![
            text("How many players?").size(48),
            buttons,
            style::touch_button("Back", 20)
                .width(Length::Fixed(240.0))
                .style(button::secondary)
                .on_press(Message::GoHome),
        ]
        .spacing(44)
        .align_x(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

fn grid_view<'a>(
    state: &'a SetupState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let header = container(
        row![
            text("Set up your pod").size(32),
            iced::widget::horizontal_space(),
            style::touch_button("Layout", 18)
                .width(Length::Fixed(160.0))
                .style(button::secondary)
                .on_press(Message::Setup(SetupMessage::BackToLayoutChoice)),
            style::touch_button("Player Count", 18)
                .width(Length::Fixed(220.0))
                .style(button::secondary)
                .on_press(Message::Setup(SetupMessage::BackToPodSizeChoice)),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .padding(16)
    .width(Length::Fill)
    .style(style::header);

    let board = match &state.table_layout {
        Some(table) => {
            layout::render_table(table, |idx| seat_tile(idx, &state.seats[idx], image_cache))
        }
        None => text("Pick a layout first.").size(20).into(),
    };

    let mut start_button = style::cta_button("Start Game", 30).width(Length::Fixed(480.0));
    if state.all_seats_ready() {
        start_button = start_button
            .style(button::success)
            .on_press(Message::Setup(SetupMessage::StartGame));
    }

    let mut content =
        column![header, container(board).height(Length::Fill)].spacing(style::GAP);

    if let Some(e) = &state.error {
        content = content.push(
            container(text(e.clone()).size(20))
                .padding(16)
                .width(Length::Fill)
                .style(style::panel_danger),
        );
    }

    content = content.push(container(start_button).center_x(Length::Fill));

    container(content.padding(style::GAP))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn seat_tile<'a>(
    index: usize,
    seat: &'a SeatSetup,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    match (&seat.player, &seat.commander) {
        (Some(player), Some(commander)) => {
            let art: Element<Message> = match commander.portrait_url().and_then(|u| image_cache.get(u)) {
                Some(handle) => image(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(ContentFit::Cover)
                    .into(),
                None => container(text("Loading art...").size(16))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .into(),
            };

            let caption = container(
                column![
                    text(player.name.clone()).size(22),
                    text(commander.name.clone()).size(14),
                ]
                .spacing(2),
            )
            .padding(14)
            .width(Length::Fill)
            .style(|_theme: &iced::Theme| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.6).into()),
                text_color: Some(Color::WHITE),
                ..container::Style::default()
            });

            let overlay = container(caption)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(iced::alignment::Vertical::Bottom);

            button(stack![art, overlay])
                .padding(0)
                .width(Length::Fill)
                .height(Length::Fill)
                .on_press(Message::Setup(SetupMessage::EditSeat(index)))
                .into()
        }
        (Some(player), None) => button(
            container(
                column![
                    text(player.name.clone()).size(30),
                    text("Tap to pick a commander").size(18),
                ]
                .spacing(10)
                .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(button::secondary)
        .on_press(Message::Setup(SetupMessage::EditSeat(index)))
        .into(),
        _ => button(
            container(text(format!("+ Add Player {}", index + 1)).size(32))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(button::secondary)
        .on_press(Message::Setup(SetupMessage::EditSeat(index)))
        .into(),
    }
}

fn editor_overlay<'a>(
    state: &'a SetupState,
    players_cache: &'a [Player],
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let seat_index = state.editing_seat.unwrap();
    let seat = &state.seats[seat_index];

    let body: Element<Message> = if state.art_target.is_some() {
        art_gallery(state, image_cache)
    } else if seat.player.is_none() {
        player_picker(state, players_cache)
    } else if seat.commander.is_none() {
        commander_picker(state, &seat.commander_history)
    } else {
        seat_summary(seat, image_cache)
    };

    let top = row![
        style::touch_button("\u{2190} Back to Grid", 20)
            .width(Length::Fixed(280.0))
            .style(button::secondary)
            .on_press(Message::Setup(SetupMessage::BackToGrid)),
    ];

    let mut content = column![top, body].spacing(style::GAP).height(Length::Fill);

    if let Some(e) = &state.error {
        content = content.push(
            container(text(e.clone()).size(20))
                .padding(16)
                .width(Length::Fill)
                .style(style::panel_danger),
        );
    }

    container(content.padding(style::GAP))
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
            style::touch_button(&p.name, 24)
                .width(Length::Fixed(300.0))
                .style(button::secondary)
                .on_press(Message::Setup(SetupMessage::PickExistingPlayer(p.clone())))
                .into()
        })
        .collect::<Vec<Element<Message>>>())
    .spacing(14)
    .wrap();

    column![
        text("Who's sitting here?").size(40),
        scrollable(existing).height(Length::Fill),
        row![
            text_input("New player name", &state.new_player_name)
                .size(26)
                .padding(22)
                .on_input(|s| Message::Setup(SetupMessage::NewPlayerNameChanged(s)))
                .on_submit(Message::Setup(SetupMessage::CreatePlayer)),
            style::touch_button("Add Player", 22)
                .width(Length::Fixed(240.0))
                .style(button::primary)
                .on_press(Message::Setup(SetupMessage::CreatePlayer)),
        ]
        .spacing(14)
        .align_y(iced::Alignment::Center),
    ]
    .spacing(22)
    .height(Length::Fill)
    .into()
}

fn commander_picker<'a>(
    state: &'a SetupState,
    history: &'a [Commander],
) -> Element<'a, Message> {
    let history_row: Element<Message> = if history.is_empty() {
        text("No commanders played yet - search below to add one.")
            .size(18)
            .into()
    } else {
        row(history
            .iter()
            .map(|c| {
                style::touch_button(&c.name, 20)
                    .width(Length::Fixed(320.0))
                    .style(button::secondary)
                    .on_press(Message::Setup(SetupMessage::PickHistoryCommander(
                        c.clone(),
                    )))
                    .into()
            })
            .collect::<Vec<Element<Message>>>())
        .spacing(12)
        .wrap()
        .into()
    };

    let results = column(
        state
            .commander_results
            .iter()
            .map(|c| {
                button(
                    container(text(format!("{}   [{}]", c.name, c.color_identity)).size(22))
                        .padding([0, 20])
                        .center_y(Length::Fill),
                )
                .padding(0)
                .height(Length::Fixed(style::TOUCH_H))
                .width(Length::Fill)
                .style(button::secondary)
                .on_press(Message::Setup(SetupMessage::PickCommanderName(c.clone())))
                .into()
            })
            .collect::<Vec<Element<Message>>>(),
    )
    .spacing(10);

    let search_label = if state.searching {
        "Searching..."
    } else {
        "Search"
    };

    column![
        text("Pick a commander").size(40),
        text("This player's commanders").size(18),
        scrollable(history_row).height(Length::Fixed(190.0)),
        row![
            text_input("Commander name", &state.commander_query)
                .size(26)
                .padding(22)
                .on_input(|s| Message::Setup(SetupMessage::CommanderQueryChanged(s)))
                .on_submit(Message::Setup(SetupMessage::SearchCommanders)),
            style::touch_button(search_label, 22)
                .width(Length::Fixed(220.0))
                .style(button::primary)
                .on_press(Message::Setup(SetupMessage::SearchCommanders)),
        ]
        .spacing(14)
        .align_y(iced::Alignment::Center),
        scrollable(results).height(Length::Fill),
    ]
    .spacing(18)
    .height(Length::Fill)
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
                    .width(Length::Fixed(300.0))
                    .height(Length::Fixed(220.0))
                    .content_fit(ContentFit::Cover)
                    .into(),
                None => container(text("...").size(20))
                    .width(Length::Fixed(300.0))
                    .height(Length::Fixed(220.0))
                    .center_x(Length::Fixed(300.0))
                    .center_y(Length::Fixed(220.0))
                    .into(),
            };
            button(
                column![thumb, text(card.set_name.clone()).size(16)]
                    .spacing(8)
                    .align_x(iced::Alignment::Center),
            )
            .padding(10)
            .style(button::secondary)
            .on_press(Message::Setup(SetupMessage::PickArt(card.clone())))
            .into()
        })
        .collect();

    let status = if state.loading_art_options {
        text("Loading every printing from Scryfall...").size(18)
    } else {
        text(format!("{} printings found", state.art_options.len())).size(18)
    };

    column![
        text(format!("Choose art for {}", target.name)).size(40),
        status,
        scrollable(row(tiles).spacing(16).wrap()).height(Length::Fill),
        style::touch_button("Back to search", 20)
            .width(Length::Fixed(280.0))
            .style(button::secondary)
            .on_press(Message::Setup(SetupMessage::CancelArtPick)),
    ]
    .spacing(18)
    .height(Length::Fill)
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
            .height(Length::Fill)
            .content_fit(ContentFit::Contain)
            .into(),
        None => container(text("Loading art...").size(16))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into(),
    };

    column![
        container(portrait).width(Length::Fill).height(Length::FillPortion(4)),
        text(format!("{} is playing {}", player.name, commander.name)).size(32),
        text(format!("Color identity: {}", commander.color_identity)).size(18),
        row![
            style::touch_button("Change Player", 20)
                .width(Length::Fill)
                .style(button::secondary)
                .on_press(Message::Setup(SetupMessage::ClearSeatPlayer)),
            style::touch_button("Change Commander", 20)
                .width(Length::Fill)
                .style(button::secondary)
                .on_press(Message::Setup(SetupMessage::ClearSeatCommander)),
            style::touch_button("Change Art", 20)
                .width(Length::Fill)
                .style(button::secondary)
                .on_press(Message::Setup(SetupMessage::ChangeArt)),
        ]
        .spacing(14),
    ]
    .spacing(18)
    .height(Length::Fill)
    .into()
}
