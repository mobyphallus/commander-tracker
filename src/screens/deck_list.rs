//! Linked deck contents, shared by player management and setup.
use crate::{
    db,
    icon::{self, Glyph},
    moxfield, style,
};
use iced::widget::{column, container, horizontal_rule, responsive, row, scrollable, text, Space};
use iced::{Element, Length, Task};
use rusqlite::{Connection, OptionalExtension};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Cards,
    ManaCurve,
}

pub struct State {
    pub page: Page,
    pub curve_bucket: Option<usize>,
    pub label: String,
    pub public_id: Option<String>,
    pub deck: Option<moxfield::Deck>,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Close,
    Page(Page),
    CurveBucket(usize),
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
            page: Page::Cards,
            curve_bucket: None,
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
            Message::Page(page) => {
                self.page = page;
                return scrollable::snap_to(
                    scrollable::Id::new("deck-list-content"),
                    scrollable::RelativeOffset::START,
                );
            }
            Message::CurveBucket(bucket) if bucket < 8 => {
                self.curve_bucket = if self.curve_bucket == Some(bucket) {
                    None
                } else {
                    Some(bucket)
                };
            }
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
    let content = responsive(move |size| {
        let mut body = column![].spacing(24).width(Length::Fill);
        if let Some(error) = &state.error {
            body = body.push(
                container(text(error).size(style::T_BODY).color(style::DANGER))
                    .padding(style::GAP)
                    .width(Length::Fill)
                    .style(style::panel_danger),
            );
        }
        if let Some(deck) = &state.deck {
            let summary = row![
                container(icon::view(Glyph::Decks, 32., style::ACCENT_BRIGHT))
                    .padding(style::GAP)
                    .style(style::badge),
                column![
                    text("MOXFIELD · DECK LIST")
                        .size(style::T_CAPTION)
                        .color(style::TEXT_MUTED),
                    text(&deck.name).size(style::T_HEADING),
                    text(if state.loading {
                        "Refreshing · showing the saved list"
                    } else {
                        if state.page == Page::Cards {
                            "Linked deck · grouped by card type"
                        } else {
                            "Linked deck · mana distribution"
                        }
                    })
                    .size(style::T_CAPTION)
                    .color(style::TEXT_MUTED),
                ]
                .spacing(style::GAP_SM)
                .width(Length::Fill),
                column![
                    text(deck.card_count().to_string())
                        .size(style::T_DISPLAY)
                        .color(style::ACCENT_BRIGHT),
                    text("cards")
                        .size(style::T_CAPTION)
                        .color(style::TEXT_MUTED),
                ]
                .align_x(iced::Alignment::Center),
            ]
            .spacing(24)
            .align_y(iced::Alignment::Center);
            body = body.push(
                container(summary)
                    .padding(24)
                    .width(Length::Fill)
                    .style(style::panel),
            );
            if state.page == Page::ManaCurve {
                body = body.push(mana_curve(deck, state.curve_bucket, size.width - 16.));
            } else {
                if !deck.commanders.is_empty() {
                    body = body.push(section("Commanders", deck.commanders.iter().collect()));
                }
                // Keep each type together, using the shortest column for the next group.
                let column_count = if size.width >= 1500. {
                    3
                } else if size.width >= 1000. {
                    2
                } else {
                    1
                };
                let mut columns: Vec<_> = (0..column_count)
                    .map(|_| column![].spacing(style::GAP).width(Length::Fill))
                    .collect();
                let mut heights = vec![0; column_count];
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
                    let cards: Vec<_> = deck
                        .mainboard
                        .iter()
                        .filter(|card| category(card) == group)
                        .collect();
                    if !cards.is_empty() {
                        let index = (0..column_count).min_by_key(|&i| heights[i]).unwrap();
                        heights[index] += cards.len() + 2;
                        let column = std::mem::replace(&mut columns[index], column![]);
                        columns[index] = column.push(section(group, cards));
                    }
                }
                body = body.push(row(columns.into_iter().map(Element::from)).spacing(style::GAP));
            }
        } else {
            let (title, detail) = if state.loading {
                (
                    "Loading your deck",
                    "Fetching the linked list from Moxfield…",
                )
            } else if state.public_id.is_none() {
                (
                    "Your deck list belongs here",
                    "Add this deck's Moxfield link in Salt & bracket to see its cards here.",
                )
            } else {
                (
                    "The deck list isn't available yet",
                    "Use Refresh list to try downloading it again.",
                )
            };
            body = body.push(
                container(
                    column![
                        icon::view(Glyph::Decks, 48., style::ACCENT_BRIGHT),
                        text(title).size(style::T_HEADING),
                        text(detail).size(style::T_BODY).color(style::TEXT_MUTED),
                    ]
                    .spacing(style::GAP)
                    .align_x(iced::Alignment::Center),
                )
                .padding(48)
                .center_x(Length::Fill)
                .style(style::panel),
            );
        }
        scrollable(container(body).padding(iced::Padding {
            right: 16.,
            ..iced::Padding::ZERO
        }))
        .id(scrollable::Id::new("deck-list-content"))
        .height(Length::Fill)
        .into()
    });
    let tabs = row![
        style::touch_button("Cards", style::T_LABEL)
            .width(150)
            .style(if state.page == Page::Cards {
                style::primary
            } else {
                style::secondary
            })
            .on_press(Message::Page(Page::Cards)),
        style::touch_button("Mana curve", style::T_LABEL)
            .width(180)
            .style(if state.page == Page::ManaCurve {
                style::primary
            } else {
                style::secondary
            })
            .on_press(Message::Page(Page::ManaCurve)),
    ]
    .spacing(style::GAP_SM);
    container(column![header, tabs, content].spacing(24))
        .padding(24)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[derive(Debug, Default)]
struct Curve {
    buckets: [u32; 8],
    lands: u32,
    unknown: u32,
    known: u32,
    mana_total: f64,
    fractional: bool,
}

fn mana_bucket(card: &moxfield::Card) -> Option<usize> {
    if category(card) == "Lands" {
        return None;
    }
    card.mana_value
        .filter(|v| v.is_finite() && *v >= 0.)
        .map(|v| v.ceil().min(7.) as usize)
}

impl Curve {
    fn from_deck(deck: &moxfield::Deck) -> Self {
        let mut curve = Self::default();
        for card in deck.all_cards() {
            if category(card) == "Lands" {
                curve.lands += card.quantity;
            } else if let Some(bucket) = mana_bucket(card) {
                curve.buckets[bucket] += card.quantity;
                curve.known += card.quantity;
                let value = card.mana_value.unwrap();
                curve.mana_total += value * f64::from(card.quantity);
                curve.fractional |= value.fract() != 0.;
            } else {
                curve.unknown += card.quantity;
            }
        }
        curve
    }
}

fn curve_stat<'a>(label: &str, value: String, note: &str, width: f32) -> Element<'a, Message> {
    container(
        column![
            text(label.to_owned())
                .size(style::T_CAPTION)
                .color(style::TEXT_MUTED),
            text(value)
                .size(style::T_HEADING)
                .color(style::ACCENT_BRIGHT),
            text(note.to_owned())
                .size(style::T_CAPTION)
                .color(style::TEXT_MUTED),
        ]
        .spacing(style::GAP_SM),
    )
    .padding(style::GAP)
    .width(width)
    .style(style::panel)
    .into()
}

fn mana_curve<'a>(
    deck: &'a moxfield::Deck,
    selected: Option<usize>,
    width: f32,
) -> Element<'a, Message> {
    let curve = Curve::from_deck(deck);
    let peak = curve.buckets.iter().copied().max().unwrap_or(0).max(1);
    let average = if curve.known == 0 {
        "—".into()
    } else {
        format!("{:.2}", curve.mana_total / f64::from(curve.known))
    };
    let columns = if width >= 1050. { 4. } else { 2. };
    let tile_width = ((width - (columns - 1.) * f32::from(style::GAP_SM)) / columns).max(100.);
    let stats = row![
        curve_stat(
            "Average mana value",
            average,
            "Known nonland cards",
            tile_width
        ),
        curve_stat(
            "Nonland cards",
            (curve.known + curve.unknown).to_string(),
            "Including commanders",
            tile_width
        ),
        curve_stat(
            "Lands",
            curve.lands.to_string(),
            "Outside the spell curve",
            tile_width
        ),
        curve_stat(
            "Mana value 0–2",
            curve.buckets[..3].iter().sum::<u32>().to_string(),
            "Low-cost cards",
            tile_width
        ),
    ]
    .spacing(style::GAP_SM)
    .wrap();
    let mut bars = row![].spacing(style::GAP_SM);
    for (bucket, count) in curve.buckets.iter().enumerate() {
        let label = if bucket == 7 {
            "7+".into()
        } else {
            bucket.to_string()
        };
        let height = 200. * (*count as f32 / peak as f32);
        let tone = if selected == Some(bucket) {
            style::ACCENT_BRIGHT
        } else {
            style::ACCENT
        };
        let stem = container(Space::new(Length::Fill, height))
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(tone.into()),
                border: iced::Border {
                    radius: 6.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
        bars = bars.push(
            iced::widget::button(
                column![
                    text(count.to_string())
                        .size(style::T_LABEL)
                        .color(if *count > 0 {
                            style::TEXT
                        } else {
                            style::TEXT_MUTED
                        }),
                    container(column![Space::new(Length::Shrink, 200. - height), stem])
                        .height(200)
                        .width(Length::Fill),
                    text(label).size(style::T_LABEL).color(style::TEXT_MUTED),
                ]
                .spacing(style::GAP_SM)
                .align_x(iced::Alignment::Center),
            )
            .padding([12, 8])
            .width(Length::FillPortion(1))
            .style(if selected == Some(bucket) {
                style::tile_selected
            } else {
                style::ghost
            })
            .on_press(Message::CurveBucket(bucket)),
        );
    }
    let mut body = column![stats,
        container(column![
            text("Mana curve").size(style::T_HEADING),
            text("Nonland cards by mana value · tap a bar to see its cards").size(style::T_BODY).color(style::TEXT_MUTED),
            bars,
            text("MANA VALUE").size(style::T_CAPTION).color(style::TEXT_MUTED),
        ].spacing(style::GAP)).padding(style::GAP).width(Length::Fill).style(style::panel),
        text("Counts include every copy and commanders. Double-faced cards use their front face; lands stay outside the curve. Mana value is the printed value, so X counts as 0 and cost reductions do not change it.").size(style::T_CAPTION).color(style::TEXT_MUTED),
    ].spacing(style::GAP);
    if curve.unknown > 0 {
        body = body.push(container(text(format!("{} nonland cards have no known mana value and are excluded from the chart and average. Refresh the list to retrieve current card data.", curve.unknown)).size(style::T_BODY)).padding(style::GAP).width(Length::Fill).style(style::panel));
    }
    if curve.fractional {
        body = body.push(text("Fractional mana values round up in the chart; the average uses their exact values.").size(style::T_CAPTION).color(style::TEXT_MUTED));
    }
    if let Some(bucket) = selected {
        let cards: Vec<_> = deck
            .all_cards()
            .filter(|card| mana_bucket(card) == Some(bucket))
            .collect();
        if cards.is_empty() {
            body = body.push(
                text("No cards in this mana-value range.")
                    .size(style::T_BODY)
                    .color(style::TEXT_MUTED),
            );
        } else {
            body = body.push(section(
                &format!(
                    "Mana value {}",
                    if bucket == 7 {
                        "7+".into()
                    } else {
                        bucket.to_string()
                    }
                ),
                cards,
            ));
        }
    }
    body.into()
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
    let mut rows = column![
        row![
            text(label.to_owned())
                .size(style::T_SUBHEAD)
                .width(Length::Fill),
            container(
                text(count.to_string())
                    .size(style::T_CAPTION)
                    .color(style::ACCENT_BRIGHT)
            )
            .padding([4, 12])
            .style(style::badge),
        ]
        .align_y(iced::Alignment::Center),
        horizontal_rule(1)
    ]
    .spacing(style::GAP_SM);
    for card in cards {
        rows = rows.push(
            container(
                row![
                    container(
                        text(format!("{}×", card.quantity))
                            .size(style::T_BODY)
                            .color(style::TEXT_MUTED)
                    )
                    .width(44)
                    .center_y(48),
                    column![
                        text(&card.name).size(style::T_LABEL),
                        text(&card.type_line)
                            .size(style::T_CAPTION)
                            .color(style::TEXT_MUTED),
                    ]
                    .spacing(style::GAP_XS)
                    .width(Length::Fill),
                ]
                .spacing(style::GAP_SM)
                .align_y(iced::Alignment::Center),
            )
            .padding([style::GAP_SM, 0])
            .width(Length::Fill),
        );
    }
    container(rows)
        .padding(20)
        .width(Length::Fill)
        .style(style::panel)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(id: &str) -> moxfield::Deck {
        let card = |name: &str, quantity, kind: &str| moxfield::Card {
            name: name.into(),
            quantity,
            type_line: kind.into(),
            layout: String::new(),
            scryfall_id: String::new(),
            oracle_text: String::new(),
            color_identity: String::new(),
            usd: None,
            reserved: false,
            mana_value: None,
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
    fn mana_curve_counts_copies_commanders_and_front_face_nonlands() {
        let mut deck = sample("curve");
        deck.commanders[0].mana_value = Some(4.);
        deck.mainboard[0].mana_value = Some(0.); // Lands must not inflate the zero bucket.
        deck.mainboard[1].quantity = 3;
        deck.mainboard[1].mana_value = Some(2.);
        let mut high = deck.mainboard[1].clone();
        high.name = "Big spell".into();
        high.quantity = 2;
        high.mana_value = Some(12.);
        deck.mainboard.push(high);
        let mut unknown = deck.mainboard[1].clone();
        unknown.quantity = 2;
        unknown.mana_value = None;
        deck.mainboard.push(unknown);
        let curve = Curve::from_deck(&deck);
        assert_eq!(curve.buckets, [0, 0, 3, 0, 1, 0, 0, 2]);
        assert_eq!((curve.lands, curve.known, curve.unknown), (35, 6, 2));
        assert_eq!(curve.mana_total, 34.);
        assert!(!curve.fractional);
    }

    #[test]
    fn mana_curve_preserves_zero_and_fractional_values_and_omits_invalid_data() {
        let mut deck = sample("curve");
        deck.commanders[0].mana_value = Some(0.);
        deck.mainboard[1].mana_value = Some(0.5);
        let curve = Curve::from_deck(&deck);
        assert_eq!(curve.buckets[..2], [1, 1]);
        assert_eq!(curve.mana_total, 0.5);
        assert!(curve.fractional);
        for value in [f64::NAN, f64::INFINITY, -1.] {
            deck.mainboard[1].mana_value = Some(value);
            assert_eq!(Curve::from_deck(&deck).unknown, 1);
        }
        deck.mainboard[1].mana_value = None;
        deck.commanders[0].mana_value = None;
        let curve = Curve::from_deck(&deck);
        assert_eq!(curve.buckets, [0; 8]);
        assert_eq!((curve.known, curve.unknown), (0, 2));
    }

    #[test]
    fn mana_curve_page_and_bucket_navigation_preserve_the_loaded_deck() {
        let (conn, game) = crate::session::tests::fixture();
        let (mut state, _) = State::open(
            &conn,
            game.seats[0].player.id,
            game.seats[0].commander.id,
            "Deck".into(),
        );
        state.deck = Some(sample("curve"));
        assert_eq!(state.page, Page::Cards);
        let _ = state.update(&conn, Message::Page(Page::ManaCurve));
        let _ = state.update(&conn, Message::CurveBucket(2));
        assert_eq!(state.curve_bucket, Some(2));
        let _ = state.update(&conn, Message::Page(Page::Cards));
        assert_eq!(state.deck.as_ref().unwrap().public_id, "curve");
        let _ = state.update(&conn, Message::CurveBucket(2));
        assert_eq!(state.curve_bucket, None);
        let _ = state.update(&conn, Message::CurveBucket(8));
        assert_eq!(state.curve_bucket, None);
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
            page: Page::Cards,
            curve_bucket: None,
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
