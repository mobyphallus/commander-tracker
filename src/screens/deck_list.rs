//! Linked deck contents, shared by player management and setup.
use crate::{
    db,
    icon::{self, Glyph},
    moxfield, style,
};
use iced::widget::{column, container, row, scrollable, text};
use iced::{Element, Length, Task};
use rusqlite::{Connection, OptionalExtension};

pub struct State {
    pub label: String,
    pub public_id: Option<String>,
    pub deck: Option<moxfield::Deck>,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Close,
    Refresh,
    Loaded(String, Result<moxfield::Deck, String>),
}

impl State {
    pub fn open(
        conn: &Connection,
        player: i64,
        commander: i64,
        label: String,
    ) -> (Self, Task<Message>) {
        let mut state = Self {
            label,
            public_id: None,
            deck: None,
            loading: false,
            error: None,
        };
        match db::deck_links(conn, player) {
            Ok(links) => state.public_id = links.get(&commander).map(|link| link.public_id.clone()),
            Err(e) => {
                state.error = Some(format!("Couldn't load this deck's link: {e}"));
                return (state, Task::none());
            }
        }
        if let Some(id) = &state.public_id {
            let cached = conn
                .query_row(
                    "SELECT contents FROM deck_lists WHERE public_id=?1",
                    [id],
                    |row| row.get::<_, String>(0),
                )
                .optional();
            match cached {
                Ok(Some(raw)) => {
                    match serde_json::from_str::<moxfield::Deck>(&raw) {
                        Ok(deck) if deck.public_id == *id => state.deck = Some(deck),
                        _ => state.error = Some(
                            "The saved deck list couldn't be read. Refresh to download it again."
                                .into(),
                        ),
                    }
                }
                Ok(None) => {
                    let task = state.fetch();
                    return (state, task);
                }
                Err(e) => state.error = Some(format!("Couldn't read the saved deck list: {e}")),
            }
        }
        (state, Task::none())
    }

    fn fetch(&mut self) -> Task<Message> {
        if self.loading {
            return Task::none();
        }
        let Some(id) = self.public_id.clone() else {
            return Task::none();
        };
        self.loading = true;
        self.error = None;
        let request = id.clone();
        Task::perform(
            async move { moxfield::fetch(request).await.map_err(|e| e.to_string()) },
            move |result| Message::Loaded(id.clone(), result),
        )
    }

    pub fn update(&mut self, conn: &Connection, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => return self.fetch(),
            Message::Loaded(id, result) if self.public_id.as_deref() == Some(&id) => {
                self.loading = false;
                match result {
                    Ok(deck) if deck.public_id == id => {
                        let saved = serde_json::to_string(&deck).map_err(|e| e.to_string()).and_then(|raw|
                            conn.execute("INSERT INTO deck_lists(public_id, contents) VALUES (?1,?2) ON CONFLICT(public_id) DO UPDATE SET contents=excluded.contents", (&id, raw))
                                .map(|_| ()).map_err(|e| e.to_string()));
                        self.error = saved.err().map(|e| {
                            format!("List loaded, but couldn't save it for offline use: {e}")
                        });
                        self.deck = Some(deck);
                    }
                    Ok(_) => {
                        self.error = Some(
                            "The returned list belongs to a different deck. Try refreshing.".into(),
                        )
                    }
                    Err(e) => self.error = Some(format!("Couldn't load the deck list: {e}")),
                }
            }
            _ => {}
        }
        Task::none()
    }
}

pub fn view(state: &State) -> Element<'_, Message> {
    let mut header = row![
        style::icon_button(Glyph::Back, "Back", style::T_LABEL)
            .width(120)
            .style(style::ghost)
            .on_press(Message::Close),
        column![
            text("Deck list").size(style::T_TITLE),
            text(&state.label)
                .size(style::T_BODY)
                .color(style::TEXT_MUTED)
        ]
        .width(Length::Fill),
    ]
    .spacing(style::GAP)
    .align_y(iced::Alignment::Center);
    if state.public_id.is_some() {
        let mut refresh = style::touch_button(
            if state.loading {
                "Loading…"
            } else {
                "Refresh list"
            },
            style::T_LABEL,
        )
        .width(180)
        .style(style::secondary);
        if !state.loading {
            refresh = refresh.on_press(Message::Refresh);
        }
        header = header.push(refresh);
    }
    let mut body = column![].spacing(style::GAP);
    if let Some(error) = &state.error {
        body = body.push(text(error).color(style::DANGER));
    }
    if let Some(deck) = &state.deck {
        body = body.push(text(&deck.name).size(style::T_HEADING)).push(
            text(format!("{} cards · Linked deck list", deck.card_count()))
                .size(style::T_BODY)
                .color(style::TEXT_MUTED),
        );
        if state.loading {
            body = body.push(text("Refreshing; showing the saved list.").color(style::TEXT_MUTED));
        }
        body = body.push(section("Commanders", deck.commanders.iter().collect()));
        for group in [
            "Creatures",
            "Planeswalkers",
            "Artifacts",
            "Enchantments",
            "Instants",
            "Sorceries",
            "Lands",
            "Other",
        ] {
            let cards = deck
                .mainboard
                .iter()
                .filter(|card| category(card) == group)
                .collect::<Vec<_>>();
            if !cards.is_empty() {
                body = body.push(section(group, cards));
            }
        }
    } else if state.loading {
        body = body.push(text("Loading the linked deck list…").size(style::T_SUBHEAD));
    } else if state.public_id.is_none() && state.error.is_none() {
        body = body
            .push(text("No linked deck list yet").size(style::T_SUBHEAD))
            .push(
                text("Add this deck's Moxfield link in Salt & bracket, then open Deck list.")
                    .size(style::T_BODY),
            );
    }
    container(column![header, scrollable(body).height(Length::Fill)].spacing(24))
        .padding(24)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn category(card: &moxfield::Card) -> &'static str {
    // Classify modal double-faced cards by their front face, once only.
    let kind = card.type_line.split(" // ").next().unwrap_or_default();
    for (needle, label) in [
        ("Land", "Lands"),
        ("Creature", "Creatures"),
        ("Planeswalker", "Planeswalkers"),
        ("Artifact", "Artifacts"),
        ("Enchantment", "Enchantments"),
        ("Instant", "Instants"),
        ("Sorcery", "Sorceries"),
    ] {
        if kind.contains(needle) {
            return label;
        }
    }
    "Other"
}

fn section<'a>(label: &str, mut cards: Vec<&'a moxfield::Card>) -> Element<'a, Message> {
    cards.sort_by(|a, b| a.name.cmp(&b.name));
    let count: u32 = cards.iter().map(|c| c.quantity).sum();
    let mut rows = column![row![
        icon::view(Glyph::Decks, 24., style::ACCENT_BRIGHT),
        text(format!("{label} · {count}")).size(style::T_SUBHEAD)
    ]
    .spacing(style::GAP_SM)]
    .spacing(style::GAP_SM);
    for card in cards {
        rows = rows.push(
            container(
                row![
                    text(format!("{}×", card.quantity))
                        .size(style::T_LABEL)
                        .color(style::TEXT_MUTED)
                        .width(60),
                    column![
                        text(&card.name).size(style::T_LABEL),
                        text(&card.type_line)
                            .size(style::T_CAPTION)
                            .color(style::TEXT_MUTED)
                    ]
                    .width(Length::Fill)
                ]
                .spacing(style::GAP),
            )
            .padding(12)
            .width(Length::Fill)
            .style(style::panel),
        );
    }
    rows.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(id: &str) -> moxfield::Deck {
        let card = |name: &str, quantity, kind: &str| moxfield::Card {
            name: name.into(),
            quantity,
            type_line: kind.into(),
            scryfall_id: String::new(),
            oracle_text: String::new(),
            color_identity: String::new(),
            usd: None,
            reserved: false,
        };
        moxfield::Deck {
            public_id: id.into(),
            name: "Test list".into(),
            url: format!("https://moxfield.com/decks/{id}"),
            owner_bracket: None,
            auto_bracket: None,
            commanders: vec![card("Leader", 1, "Legendary Creature")],
            mainboard: vec![
                card("Island", 35, "Basic Land"),
                card("Test modal", 1, "Sorcery // Land"),
            ],
        }
    }
    #[test]
    fn cache_preserves_quantities_and_uses_exact_owners_link() {
        let (conn, game) = crate::session::tests::fixture();
        let commander = game.seats[0].commander.id;
        let owner = game.seats[0].player.id;
        let pilot = game.seats[1].player.id;
        for (player, id) in [(owner, "owner-deck"), (pilot, "pilot-deck")] {
            db::set_deck_link(&conn, player, commander, id, &sample(id).url).unwrap();
            let (mut state, _) = State::open(&conn, player, commander, "Deck".into());
            let _ = state.update(&conn, Message::Loaded(id.into(), Ok(sample(id))));
            assert!(state.error.is_none());
        }
        let (owner_list, _) = State::open(&conn, owner, commander, "Borrowed".into());
        let (pilot_list, _) = State::open(&conn, pilot, commander, "Own".into());
        assert_eq!(owner_list.deck.as_ref().unwrap().public_id, "owner-deck");
        assert_eq!(pilot_list.deck.as_ref().unwrap().public_id, "pilot-deck");
        assert_eq!(owner_list.deck.as_ref().unwrap().card_count(), 37);
        assert!(!owner_list.loading);
        assert_eq!(category(&sample("x").mainboard[1]), "Sorceries");
        db::remove_deck_link(&conn, owner, commander).unwrap();
        let (unlinked, _) = State::open(&conn, owner, commander, "Deck".into());
        assert!(unlinked.deck.is_none() && unlinked.public_id.is_none());
    }
    #[test]
    fn refresh_errors_keep_saved_list_and_late_results_do_not_replace_it() {
        let (conn, _) = crate::session::tests::fixture();
        let mut state = State {
            label: "Deck".into(),
            public_id: Some("current-id".into()),
            deck: Some(sample("current-id")),
            loading: true,
            error: None,
        };
        let _ = state.update(
            &conn,
            Message::Loaded("old-deck".into(), Ok(sample("old-deck"))),
        );
        assert!(state.loading);
        let _ = state.update(
            &conn,
            Message::Loaded("current-id".into(), Err("offline".into())),
        );
        assert!(!state.loading);
        assert!(state.error.as_ref().unwrap().contains("offline"));
        assert_eq!(state.deck.unwrap().public_id, "current-id");
    }
    #[test]
    fn players_and_setup_open_lists_separately_from_breakdowns() {
        use crate::screens::{players, setup};
        let (conn, game) = crate::session::tests::fixture();
        let player = game.seats[0].player.clone();
        let commander = game.seats[0].commander.id;
        // Fixture game commanders need to be in the player's collection too.
        db::record_player_commander_use(&conn, player.id, commander).unwrap();
        let mut players = players::PlayersState::load(&conn);
        let _ = players::update(
            &mut players,
            &conn,
            players::PlayersMessage::ManageCommanders(player.clone()),
        );
        let _ = players::update(
            &mut players,
            &conn,
            players::PlayersMessage::OpenDeckList(commander),
        );
        let managed = players.managing.as_ref().unwrap();
        assert!(managed.deck_list.is_some() && managed.deck_page.is_none());
        let _ = players::update(
            &mut players,
            &conn,
            players::PlayersMessage::DeckList(Message::Close),
        );
        assert!(players.managing.as_ref().unwrap().deck_list.is_none());
        let mut setup = setup::SetupState::rematch(&game, &conn);
        let _ = setup::update(
            &mut setup,
            &conn,
            setup::SetupMessage::OpenDeckList(player.id, commander, "Deck".into()),
        );
        assert!(setup.deck_list.is_some() && setup.score_summary.is_none());
        let _ = setup::update(
            &mut setup,
            &conn,
            setup::SetupMessage::DeckList(Message::Close),
        );
        assert!(setup.deck_list.is_none());
    }
}
