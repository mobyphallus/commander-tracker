use std::collections::HashMap;

use iced::widget::{
    button, column, container, image, mouse_area, row, scrollable, text, text_input,
};
use iced::{Element, Length, Task};
use rusqlite::Connection;

use crate::app::Message;
use crate::cards;
use crate::db;
use crate::keyboard::{self, Keyboard};
use crate::model::{Commander, Player, SavedDeck};
use crate::screens::breakdown;
use crate::scryfall::{self, Cooldown, ScryfallCard, ScryfallError};
use crate::style;

/// Every text field on this screen. The on-screen keyboard types into one
/// at a time and needs to know which, since it edits the `String` behind
/// the field rather than the widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    NewPlayer,
    Rename,
    Search,
    /// The Moxfield deck link on a deck's own page.
    MoxfieldLink,
}

impl Field {
    /// The iced widget id, so tapping a field can focus the real
    /// `text_input` as well as raising the keyboard.
    fn id(self) -> text_input::Id {
        text_input::Id::new(match self {
            Field::NewPlayer => "players.new",
            Field::Rename => "players.rename",
            Field::Search => "players.search",
            Field::MoxfieldLink => "players.link",
        })
    }
}

/// What the commander search is currently shopping for. A search that knows
/// this can add a partner straight from Scryfall, instead of making someone
/// save both halves separately and pair them afterwards.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchFor {
    /// A new deck for this player.
    Deck,
    /// Replace the primary commander of the selected saved deck.
    Replace(Commander),
    /// The second half of a partner pair, for a deck they already have.
    Partner(Commander),
}

pub struct PlayersState {
    pub players: Vec<Player>,
    pub profile: Option<i64>,
    pub pictures: HashMap<i64, image::Handle>,
    pub choosing_picture: bool,
    pub new_player_name: String,
    pub editing: Option<(i64, String)>,
    /// Seat of a pending delete, so it takes two taps to remove someone.
    pub confirming_delete: Option<i64>,
    /// When set, we're managing this player's commander list instead of the
    /// roster.
    pub managing: Option<ManagedPlayer>,
    /// Non-zero while Scryfall has us locked out for exceeding the rate
    /// limit. Lives on the screen rather than on `managing` so it survives
    /// closing and reopening a player's commander list.
    pub cooldown: Cooldown,
    /// The app's own keyboard, and the field it's typing into.
    pub kb: Keyboard<Field>,
    pub error: Option<String>,
}

pub fn load_pictures(conn: &Connection) -> HashMap<i64, image::Handle> {
    db::player_pictures(conn)
        .unwrap_or_default()
        .into_iter()
        .map(|(id, bytes)| (id, image::Handle::from_bytes(bytes)))
        .collect()
}

fn normalize_picture(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let photo = ::image::load_from_memory(bytes)
        .map_err(|_| "Choose a valid PNG or JPEG image.".to_string())?;
    let side = photo.width().min(photo.height());
    let square = photo
        .crop_imm(
            (photo.width() - side) / 2,
            (photo.height() - side) / 2,
            side,
            side,
        )
        .resize_exact(512, 512, ::image::imageops::FilterType::Lanczos3);
    let mut png = std::io::Cursor::new(Vec::new());
    square
        .write_to(&mut png, ::image::ImageOutputFormat::Png)
        .map_err(|e| format!("Couldn't prepare the picture: {e}"))?;
    Ok(png.into_inner())
}

async fn choose_picture() -> Result<Option<Vec<u8>>, String> {
    tokio::task::spawn_blocking(|| {
        let result = std::process::Command::new("zenity")
            .args([
                "--file-selection",
                "--title=Choose profile picture",
                "--file-filter=Images | *.png *.jpg *.jpeg *.PNG *.JPG *.JPEG",
            ])
            .output()
            .map_err(|e| format!("Couldn't open the image picker: {e}"))?;
        if result.status.code() == Some(1) {
            return Ok(None);
        }
        if !result.status.success() {
            return Err("Couldn't open the image picker.".to_string());
        }
        let path = String::from_utf8(result.stdout)
            .map_err(|_| "Couldn't read that filename.".to_string())?;
        let path = path.trim_end_matches(['\n', '\r']);
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 20 * 1024 * 1024 {
            return Err("Choose an image smaller than 20 MB.".to_string());
        }
        let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read the picture: {e}"))?;
        normalize_picture(&bytes).map(Some)
    })
    .await
    .map_err(|e| format!("Couldn't load the picture: {e}"))?
}

pub struct ManagedPlayer {
    pub player: Player,
    pub commanders: Vec<SavedDeck>,
    pub query: String,
    pub results: Vec<ScryfallCard>,
    pub searching: bool,
    /// Set while picking a different printing's art for one of this
    /// player's commanders.
    pub art_for: Option<Commander>,
    pub art_options: Vec<ScryfallCard>,
    pub loading_art: bool,
    /// Set while the search is open, saying what a picked card becomes.
    pub search_for: Option<SearchFor>,
    /// The deck whose actions are showing, by its primary commander's id.
    /// A deck you're about to change art on or remove is picked first, so
    /// the tiles themselves stay nothing but the cards.
    pub selected: Option<i64>,
    /// What's known about each deck's Moxfield link, keyed by the deck's
    /// primary commander. Loaded with the player so the tiles can show a
    /// bracket without a query per deck.
    pub links: HashMap<i64, db::DeckLink>,
    /// Set while one deck's own page is open, on top of the deck list.
    pub deck_page: Option<DeckPage>,
}

/// One deck's page: its Moxfield link, and the breakdown if it has one.
///
/// The analysis is held here rather than read from the database on every
/// frame because it's a big structure and `view` runs constantly. It's
/// loaded once when the page opens.
pub struct DeckPage {
    pub commander_id: i64,
    pub deck_label: String,
    /// What's in the link box. Starts as whatever was saved, so a link can
    /// be corrected rather than retyped.
    pub link_input: String,
    pub analysis: Option<crate::salt::Analysis>,
    /// True while Moxfield and the scorers are being talked to.
    pub busy: bool,
}

impl ManagedPlayer {
    fn deck(&self, commander_id: i64) -> Option<&SavedDeck> {
        self.commanders
            .iter()
            .find(|d| d.commander.id == commander_id)
    }
}

impl PlayersState {
    pub fn load(conn: &Connection) -> Self {
        let (players, error) = match db::list_players(conn) {
            Ok(p) => (p, None),
            Err(e) => (Vec::new(), Some(format!("Couldn’t load players: {e}"))),
        };
        Self {
            players,
            profile: None,
            pictures: load_pictures(conn),
            choosing_picture: false,
            new_player_name: String::new(),
            editing: None,
            confirming_delete: None,
            managing: None,
            cooldown: Cooldown::default(),
            kb: Keyboard::default(),
            error,
        }
    }

    fn refresh(&mut self, conn: &Connection) {
        match db::list_players(conn) {
            Ok(p) => self.players = p,
            Err(e) => self.error = Some(format!("Couldn’t reload players: {e}")),
        }
    }
}

#[derive(Debug, Clone)]
pub enum PlayersMessage {
    RetryLoad,
    OpenProfile(i64),
    ChoosePicture(i64),
    PictureChosen(i64, Result<Option<Vec<u8>>, String>),
    CloseProfile,
    NewNameChanged(String),
    CreatePlayer,
    StartEdit(i64, String),
    NameChanged(String),
    Save,
    Cancel,
    AskDelete(i64),
    ConfirmDelete(i64),
    CancelDelete,
    ManageCommanders(Player),
    CloseManage,
    /// A deck tile was tapped: show its actions, or put them away again.
    SelectDeck(i64),
    OpenSearch(SearchFor),
    CloseSearch,
    QueryChanged(String),
    Search,
    SearchResults(Result<Vec<ScryfallCard>, ScryfallError>),
    AddCommander(ScryfallCard),
    RemoveCommander(i64),
    ChangeArt(Commander),
    ArtOptionsLoaded(Result<Vec<ScryfallCard>, ScryfallError>),
    PickArt(ScryfallCard),
    CancelArt,
    PickPartner(Commander),
    Unpair(Commander),
    /// A text field was tapped, so the keyboard comes up on it.
    Focus(Field),
    Key(keyboard::Key),
    CooldownTick,

    /// Open one deck's own page, where its Moxfield link and breakdown live.
    OpenDeckPage(i64),
    CloseDeckPage,
    LinkChanged(String),
    /// Save what's in the link box and analyse it. No argument: it always
    /// acts on the deck page that's open, which is also what lets the
    /// keyboard's submit key stand in for it.
    SaveLink,
    /// Forget a deck's link and everything worked out from it.
    RemoveLink,
    /// An analysis finished, for the deck with this commander id. Carries the
    /// id rather than assuming the same page is still open - someone can walk
    /// away from a slow analysis and it must not land on another deck.
    Analysed(i64, Result<crate::salt::Analysis, String>),
}

pub fn update(
    state: &mut PlayersState,
    conn: &Connection,
    message: PlayersMessage,
) -> Task<Message> {
    state.error = None;
    match message {
        PlayersMessage::RetryLoad => {
            state.error = None;
            state.refresh(conn);
        }
        PlayersMessage::ChoosePicture(id) => {
            if state.choosing_picture {
                return Task::none();
            }
            state.choosing_picture = true;
            return Task::perform(choose_picture(), move |result| {
                Message::Players(PlayersMessage::PictureChosen(id, result))
            });
        }
        PlayersMessage::PictureChosen(id, result) => {
            state.choosing_picture = false;
            match result {
                Ok(Some(bytes)) => match db::set_player_picture(conn, id, &bytes) {
                    Ok(()) => {
                        state.pictures.insert(id, image::Handle::from_bytes(bytes));
                    }
                    Err(e) => state.error = Some(format!("Couldn't save the picture: {e}")),
                },
                Ok(None) => {}
                Err(e) => state.error = Some(e),
            }
        }
        PlayersMessage::OpenProfile(id) => {
            state.profile = Some(id);
            state.kb.close();
        }
        PlayersMessage::CloseProfile => {
            state.profile = None;
            state.editing = None;
            state.confirming_delete = None;
            state.kb.close();
        }

        PlayersMessage::NewNameChanged(s) => state.new_player_name = s,
        PlayersMessage::CreatePlayer => {
            let name = state.new_player_name.trim().to_string();
            if name.is_empty() {
                return Task::none();
            }
            match db::create_player(conn, &name) {
                Ok(_) => {
                    state.new_player_name.clear();
                    state.kb.close();
                    state.refresh(conn);
                }
                Err(e) => state.error = Some(format!("Couldn't add player: {e}")),
            }
        }
        PlayersMessage::StartEdit(id, name) => {
            state.kb.open(Field::Rename, &name);
            state.editing = Some((id, name));
            state.confirming_delete = None;
            return text_input::focus(Field::Rename.id());
        }
        PlayersMessage::NameChanged(s) => {
            if let Some((_, name)) = &mut state.editing {
                *name = s;
            }
        }
        PlayersMessage::Save => {
            if let Some((id, name)) = state.editing.take() {
                let trimmed = name.trim().to_string();
                if trimmed.is_empty() {
                    state.error = Some("Name can't be empty.".into());
                } else {
                    match db::rename_player(conn, id, &trimmed) {
                        Ok(()) => {
                            state.kb.close();
                            state.refresh(conn);
                        }
                        Err(e) => state.error = Some(format!("Couldn't rename: {e}")),
                    }
                }
            }
        }
        PlayersMessage::Cancel => {
            state.editing = None;
            state.kb.close();
        }
        PlayersMessage::AskDelete(id) => {
            state.confirming_delete = Some(id);
            state.editing = None;
            state.kb.close();
        }
        PlayersMessage::CancelDelete => state.confirming_delete = None,
        PlayersMessage::ConfirmDelete(id) => {
            state.confirming_delete = None;
            match db::player_game_count(conn, id) {
                Ok(0) => match db::delete_player(conn, id) {
                    Ok(()) => state.refresh(conn),
                    Err(e) => state.error = Some(format!("Couldn't delete: {e}")),
                },
                Ok(n) => {
                    state.error = Some(format!(
                        "That player is in {n} recorded game(s), so their history would break. Rename them instead."
                    ))
                }
                Err(e) => state.error = Some(format!("Couldn't check history: {e}")),
            }
        }
        PlayersMessage::ManageCommanders(player) => {
            let commanders = db::player_commander_history(conn, player.id).unwrap_or_default();
            let art = deck_art_tasks(&commanders);
            state.editing = None;
            state.kb.close();
            let links = db::deck_links(conn, player.id).unwrap_or_default();
            state.managing = Some(ManagedPlayer {
                player,
                commanders,
                query: String::new(),
                results: Vec::new(),
                searching: false,
                art_for: None,
                art_options: Vec::new(),
                loading_art: false,
                search_for: None,
                selected: None,
                links,
                deck_page: None,
            });
            return art;
        }
        PlayersMessage::CloseManage => {
            state.managing = None;
            state.kb.close();
        }
        PlayersMessage::SelectDeck(commander_id) => {
            if let Some(m) = &mut state.managing {
                // Tapping the open deck again closes it, so there's always a
                // way back to just looking at the cards.
                m.selected = if m.selected == Some(commander_id) {
                    None
                } else {
                    Some(commander_id)
                };
            }
        }
        PlayersMessage::OpenSearch(what) => {
            if let Some(m) = &mut state.managing {
                m.search_for = Some(what);
                m.query.clear();
                m.results.clear();
                m.selected = None;
            }
            state.kb.open(Field::Search, "");
            return text_input::focus(Field::Search.id());
        }
        PlayersMessage::CloseSearch => {
            if let Some(m) = &mut state.managing {
                if let Some(SearchFor::Replace(previous)) = m.search_for.take() {
                    m.selected = Some(previous.id);
                }
                m.query.clear();
                m.results.clear();
            }
            state.kb.close();
        }
        PlayersMessage::QueryChanged(s) => {
            if let Some(m) = &mut state.managing {
                m.query = s;
            }
        }
        PlayersMessage::Search => {
            if state.cooldown.active() {
                return Task::none();
            }
            if let Some(m) = &mut state.managing {
                let query = m.query.clone();
                if query.trim().is_empty() {
                    return Task::none();
                }
                m.searching = true;
                return Task::perform(scryfall::search_commanders(query), |res| {
                    Message::Players(PlayersMessage::SearchResults(res))
                });
            }
        }
        PlayersMessage::SearchResults(res) => {
            if let Some(m) = &mut state.managing {
                m.searching = false;
            }
            match res {
                Ok(list) => {
                    // The card is the thing being picked, so the pictures
                    // are fetched with the names rather than on demand.
                    let art = card_art_tasks(&list);
                    if let Some(m) = &mut state.managing {
                        m.results = list;
                    }
                    state.error = None;
                    return art;
                }
                Err(e) => {
                    state.cooldown.absorb(&e);
                    state.error = Some(e.to_string());
                }
            }
        }
        PlayersMessage::CooldownTick => {
            state.cooldown.tick();
            if !state.cooldown.active() {
                state.error = None;
            }
        }

        PlayersMessage::OpenDeckPage(commander_id) => {
            let Some(m) = &mut state.managing else {
                return Task::none();
            };
            let Some(deck) = m.deck(commander_id) else {
                return Task::none();
            };
            let deck_label = deck.label();
            let link_input = m
                .links
                .get(&commander_id)
                .map(|l| l.url.clone())
                .unwrap_or_default();
            // Read once here rather than on every frame: the breakdown is a
            // big structure and `view` runs constantly.
            let analysis = db::deck_breakdown(conn, m.player.id, commander_id);
            state.kb.close();
            m.deck_page = Some(DeckPage {
                commander_id,
                deck_label,
                link_input,
                analysis,
                busy: false,
            });
        }
        PlayersMessage::CloseDeckPage => {
            if let Some(m) = &mut state.managing {
                m.deck_page = None;
            }
            state.kb.close();
        }
        PlayersMessage::LinkChanged(value) => {
            if let Some(page) = state.managing.as_mut().and_then(|m| m.deck_page.as_mut()) {
                page.link_input = value;
            }
        }
        PlayersMessage::SaveLink => {
            let Some(m) = &mut state.managing else {
                return Task::none();
            };
            let player_id = m.player.id;
            let Some(page) = &mut m.deck_page else {
                return Task::none();
            };

            let input = page.link_input.trim().to_string();
            // Checked here rather than after a round trip, so a typo comes
            // back instantly instead of as a failed request.
            let Some(public_id) = crate::moxfield::parse_ref(&input) else {
                state.error = Some(crate::moxfield::Error::NotALink.to_string());
                return Task::none();
            };

            let commander_id = page.commander_id;
            let url = format!("https://moxfield.com/decks/{public_id}");
            if let Err(e) = db::set_deck_link(conn, player_id, commander_id, &public_id, &url) {
                state.error = Some(format!("Couldn't save that link: {e}"));
                return Task::none();
            }

            // A re-check of the same list keeps the old breakdown on screen
            // while it runs, so a failed check doesn't blank out numbers we
            // still have. A different list invalidates them.
            if m.links
                .get(&commander_id)
                .is_some_and(|l| l.public_id != public_id)
            {
                page.analysis = None;
            }
            page.link_input = url.clone();
            page.busy = true;
            m.links = db::deck_links(conn, player_id).unwrap_or_default();
            state.kb.close();
            return Task::perform(crate::salt::from_link(url), move |result| {
                Message::Players(PlayersMessage::Analysed(commander_id, result))
            });
        }
        PlayersMessage::RemoveLink => {
            let Some(m) = &mut state.managing else {
                return Task::none();
            };
            let player_id = m.player.id;
            let Some(page) = &mut m.deck_page else {
                return Task::none();
            };
            if let Err(e) = db::remove_deck_link(conn, player_id, page.commander_id) {
                state.error = Some(format!("Couldn't remove that link: {e}"));
                return Task::none();
            }
            page.link_input.clear();
            page.analysis = None;
            page.busy = false;
            m.links = db::deck_links(conn, player_id).unwrap_or_default();
        }
        PlayersMessage::Analysed(commander_id, result) => {
            let Some(m) = &mut state.managing else {
                return Task::none();
            };
            let player_id = m.player.id;

            match result {
                Ok(analysis) => {
                    if let Err(e) = db::save_deck_analysis(
                        conn,
                        player_id,
                        commander_id,
                        &analysis.public_id,
                        &analysis,
                    ) {
                        state.error = Some(format!("Couldn't save the analysis: {e}"));
                    }
                    m.links = db::deck_links(conn, player_id).unwrap_or_default();
                    // Only fill in the page if it's still this deck's: a slow
                    // analysis must not land on a deck someone has since
                    // opened instead.
                    if let Some(page) = m
                        .deck_page
                        .as_mut()
                        .filter(|p| p.commander_id == commander_id)
                    {
                        page.analysis = Some(analysis);
                        page.busy = false;
                    }
                }
                Err(message) => {
                    state.error = Some(message);
                    if let Some(page) = m
                        .deck_page
                        .as_mut()
                        .filter(|p| p.commander_id == commander_id)
                    {
                        page.busy = false;
                    }
                }
            }
        }
        PlayersMessage::AddCommander(card) => {
            let Some(m) = &mut state.managing else {
                return Task::none();
            };
            match db::upsert_commander(
                conn,
                &card.oracle_id,
                &card.name,
                card.image_url.as_deref(),
                card.art_crop_url.as_deref(),
                &card.color_identity,
            ) {
                Ok(commander) => {
                    let saved = if let Some(SearchFor::Replace(previous)) = &m.search_for {
                        if commander.id != previous.id
                            && m.commanders.iter().any(|deck| {
                                deck.commander.id == commander.id
                                    || deck.partner.as_ref().is_some_and(|p| p.id == commander.id)
                            })
                        {
                            state.error = Some("That commander is already in this player's collection. Choose another commander.".into());
                            return Task::none();
                        }
                        db::replace_player_commander(conn, m.player.id, previous.id, commander.id)
                    } else {
                        db::record_player_commander_use(conn, m.player.id, commander.id).and_then(
                            |()| {
                                if let Some(SearchFor::Partner(primary)) = &m.search_for {
                                    if primary.id != commander.id {
                                        return db::set_player_partner(
                                            conn,
                                            m.player.id,
                                            primary.id,
                                            Some(commander.id),
                                        );
                                    }
                                }
                                Ok(())
                            },
                        )
                    };
                    if let Err(error) = saved {
                        state.error = Some(format!("Couldn't save commander: {error}"));
                        return Task::none();
                    }
                    m.selected = match &m.search_for {
                        Some(SearchFor::Partner(primary)) => Some(primary.id),
                        _ => Some(commander.id),
                    };
                    match db::player_commander_history(conn, m.player.id) {
                        Ok(decks) => m.commanders = decks,
                        Err(e) => state.error = Some(format!("Couldn’t load decks: {e}")),
                    }
                    m.search_for = None;
                    m.results.clear();
                    m.query.clear();
                    let art = deck_art_tasks(&m.commanders);
                    state.kb.close();
                    return art;
                }
                Err(e) => state.error = Some(format!("Couldn't add commander: {e}")),
            }
        }
        PlayersMessage::RemoveCommander(commander_id) => {
            if let Some(m) = &mut state.managing {
                // A partner pair is one deck, so removing it takes both
                // halves with it. Leaving the partner behind would leave it
                // pointing at a commander this player no longer has, and
                // the pair would come back as a deck under the other name.
                let partner_id = m
                    .deck(commander_id)
                    .and_then(|d| d.partner.as_ref())
                    .map(|p| p.id);
                if let Err(e) = db::set_player_partner(conn, m.player.id, commander_id, None) {
                    state.error = Some(format!("Couldn’t save the partner change: {e}"));
                    return Task::none();
                }
                let removed = db::remove_player_commander(conn, m.player.id, commander_id)
                    .and_then(|()| match partner_id {
                        Some(id) => db::remove_player_commander(conn, m.player.id, id),
                        None => Ok(()),
                    });
                match removed {
                    Ok(()) => {
                        m.selected = None;
                        match db::player_commander_history(conn, m.player.id) {
                            Ok(decks) => m.commanders = decks,
                            Err(e) => state.error = Some(format!("Couldn’t load decks: {e}")),
                        }
                    }
                    Err(e) => state.error = Some(format!("Couldn't remove: {e}")),
                }
            }
        }
        PlayersMessage::ChangeArt(commander) => {
            if let Some(m) = &mut state.managing {
                let oracle_id = commander.oracle_id.clone();
                m.art_for = Some(commander);
                m.art_options.clear();
                m.loading_art = true;
                return Task::perform(scryfall::fetch_prints(oracle_id), |res| {
                    Message::Players(PlayersMessage::ArtOptionsLoaded(res))
                });
            }
        }
        PlayersMessage::ArtOptionsLoaded(res) => {
            if let Some(m) = &mut state.managing {
                m.loading_art = false;
            }
            match res {
                Ok(list) => {
                    let thumbs: Vec<Task<Message>> = list
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
                    if let Some(m) = &mut state.managing {
                        m.art_options = list;
                    }
                    return Task::batch(thumbs);
                }
                Err(e) => {
                    state.cooldown.absorb(&e);
                    state.error = Some(e.to_string());
                }
            }
        }
        PlayersMessage::PickArt(card) => {
            let Some(m) = &mut state.managing else {
                return Task::none();
            };
            let Some(target) = m.art_for.take() else {
                return Task::none();
            };
            m.art_options.clear();
            match db::upsert_commander(
                conn,
                &target.oracle_id,
                &target.name,
                card.image_url.as_deref(),
                card.art_crop_url.as_deref(),
                &target.color_identity,
            ) {
                Ok(commander) => {
                    match db::player_commander_history(conn, m.player.id) {
                        Ok(decks) => m.commanders = decks,
                        Err(e) => state.error = Some(format!("Couldn’t load decks: {e}")),
                    }
                    if let Some(url) = commander.portrait_url() {
                        let url = url.to_string();
                        let key = url.clone();
                        return Task::perform(scryfall::fetch_image(url), move |res| {
                            Message::ArtLoaded(key.clone(), res)
                        });
                    }
                }
                Err(e) => state.error = Some(format!("Couldn't save art: {e}")),
            }
        }
        PlayersMessage::CancelArt => {
            if let Some(m) = &mut state.managing {
                m.art_for = None;
                m.art_options.clear();
            }
        }
        PlayersMessage::PickPartner(partner) => {
            let Some(m) = &mut state.managing else {
                return Task::none();
            };
            let Some(SearchFor::Partner(primary)) = m.search_for.take() else {
                return Task::none();
            };
            if primary.id != partner.id {
                if let Err(e) =
                    db::set_player_partner(conn, m.player.id, primary.id, Some(partner.id))
                {
                    state.error = Some(format!("Couldn’t save the partner change: {e}"));
                    return Task::none();
                }
            }
            m.selected = Some(primary.id);
            match db::player_commander_history(conn, m.player.id) {
                Ok(decks) => m.commanders = decks,
                Err(e) => state.error = Some(format!("Couldn’t load decks: {e}")),
            }
            m.query.clear();
            m.results.clear();
            state.kb.close();
        }
        PlayersMessage::Unpair(commander) => {
            if let Some(m) = &mut state.managing {
                if let Err(e) = db::set_player_partner(conn, m.player.id, commander.id, None) {
                    state.error = Some(format!("Couldn’t save the partner change: {e}"));
                    return Task::none();
                }
                match db::player_commander_history(conn, m.player.id) {
                    Ok(decks) => m.commanders = decks,
                    Err(e) => state.error = Some(format!("Couldn’t load decks: {e}")),
                }
            }
        }
        PlayersMessage::Focus(field) => {
            let value = current_text(state, field).to_string();
            state.kb.open(field, &value);
            return text_input::focus(field.id());
        }
        PlayersMessage::Key(key) => {
            let Some(field) = state.kb.field() else {
                return Task::none();
            };
            // The keyboard edits the string behind the field, so the value
            // has to be lifted out, typed into, and put back.
            let mut value = current_text(state, field).to_string();
            let outcome = state.kb.press(key, &mut value);
            set_text(state, field, value);
            return match outcome {
                keyboard::Outcome::Submit => update(state, conn, field.action()),
                _ => Task::none(),
            };
        }
    }
    Task::none()
}

/// The text currently in `field`, wherever on the screen it lives.
fn current_text(state: &PlayersState, field: Field) -> &str {
    match field {
        Field::NewPlayer => &state.new_player_name,
        Field::Rename => state
            .editing
            .as_ref()
            .map(|(_, n)| n.as_str())
            .unwrap_or(""),
        Field::Search => state
            .managing
            .as_ref()
            .map(|m| m.query.as_str())
            .unwrap_or(""),
        Field::MoxfieldLink => state
            .managing
            .as_ref()
            .and_then(|m| m.deck_page.as_ref())
            .map(|p| p.link_input.as_str())
            .unwrap_or(""),
    }
}

fn set_text(state: &mut PlayersState, field: Field, value: String) {
    match field {
        Field::NewPlayer => state.new_player_name = value,
        Field::Rename => {
            if let Some((_, name)) = &mut state.editing {
                *name = value;
            }
        }
        Field::Search => {
            if let Some(m) = &mut state.managing {
                m.query = value;
            }
        }
        Field::MoxfieldLink => {
            if let Some(page) = state.managing.as_mut().and_then(|m| m.deck_page.as_mut()) {
                page.link_input = value;
            }
        }
    }
}

impl Field {
    /// What this field's Enter key does - the one action the keyboard's
    /// own submit key stands in for.
    fn action(self) -> PlayersMessage {
        match self {
            Field::NewPlayer => PlayersMessage::CreatePlayer,
            Field::Rename => PlayersMessage::Save,
            Field::Search => PlayersMessage::Search,
            Field::MoxfieldLink => PlayersMessage::SaveLink,
        }
    }

    /// The label on that submit key.
    fn action_label(self) -> &'static str {
        match self {
            Field::NewPlayer => "Add",
            Field::Rename => "Save",
            Field::Search => "Search",
            Field::MoxfieldLink => "Check",
        }
    }
}

/// Fetches the portraits for a player's saved decks, so their list of decks
/// is a list of pictures the moment it opens.
fn deck_art_tasks(decks: &[SavedDeck]) -> Task<Message> {
    let urls: Vec<String> = decks
        .iter()
        .flat_map(|d| [Some(&d.commander), d.partner.as_ref()])
        .flatten()
        .filter_map(|c| c.portrait_url().map(str::to_string))
        .collect();
    fetch_all(urls)
}

/// Thumbnails for search results. Capped: Scryfall answers a loose name
/// with up to 175 cards, and nobody scrolls past the first screenful of a
/// search they're about to refine anyway.
fn card_art_tasks(cards: &[ScryfallCard]) -> Task<Message> {
    let urls: Vec<String> = cards
        .iter()
        .take(cards::PREFETCH)
        .filter_map(|c| c.small_url.clone().or_else(|| c.image_url.clone()))
        .collect();
    fetch_all(urls)
}

pub(crate) fn fetch_all(urls: Vec<String>) -> Task<Message> {
    Task::batch(urls.into_iter().map(|url| {
        let key = url.clone();
        Task::perform(scryfall::fetch_image(url), move |res| {
            Message::ArtLoaded(key.clone(), res)
        })
    }))
}

// ---------------------------------------------------------------------------
// Layout rhythm
//
// One padding scale for the whole screen, all of it derived from
// [`style::GAP`], and one set of column widths so every row in every list
// breaks at the same places.
// ---------------------------------------------------------------------------

/// Row padding: snug vertically, a full gap in from the edge. With a
/// [`style::TOUCH_H`] control inside, every row lands on the same height.
const ROW_PAD: [u16; 2] = [style::GAP_SM, style::GAP];

/// Vertical padding that puts a [`style::T_SUBHEAD`] field on exactly the
/// standard touch height, so a row being renamed is the same height as the
/// row it replaced. iced's default line height is 1.3x the font size.
const FIELD_PAD: [f32; 2] = [style::FIELD_PAD, style::GAP as f32];

/// Action column widths. Shared by the roster and the commander list so the
/// right-hand edge of every list is one straight line.
const W_WIDE: f32 = 200.0;
const W_ACTION: f32 = 160.0;
const W_NARROW: f32 = 128.0;
/// One printing in the art gallery. Card-shaped, because a printing is a
/// whole card and a box that isn't its shape would make the picture
/// overflow - see the note in [`crate::cards`].
const ART_W: f32 = 200.0;
const ART_H: f32 = ART_W * 204.0 / 146.0;

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

/// The title block every view on this screen starts with, with the way out
/// in the top right where it is on every other screen.
fn screen_header<'a>(title: String, _size: u16, back: Message) -> Element<'a, Message> {
    style::page_header(title, "Players & commanders", back)
}

/// Something deliberate to look at when a list is empty - centred, so it
/// reads as a state of the screen rather than as rows that failed to draw.
fn empty_state<'a>(headline: &'a str, note: &'a str) -> Element<'a, Message> {
    container(
        column![
            text(headline).size(style::T_LEAD).color(style::TEXT),
            text(note).size(style::T_BODY).color(style::TEXT_MUTED),
        ]
        .spacing(style::GAP_XS)
        .align_x(iced::Alignment::Center),
    )
    .padding(style::GAP * 2)
    .center_x(Length::Fill)
    .style(style::panel)
    .into()
}

/// [`empty_state`] floated in the middle of whatever space is left, for the
/// cases where the empty list *is* the whole screen.
fn empty_fill<'a>(headline: &'a str, note: &'a str) -> Element<'a, Message> {
    container(empty_state(headline, note))
        .width(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

/// A quiet heading over a list, so the two halves of the commander screen
/// say what they are. Takes an owned `String` as happily as a literal, for
/// the lines that have to be built at render time.
fn section_label<'a>(label: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    text(label)
        .size(style::T_CAPTION)
        .color(style::TEXT_MUTED)
        .into()
}

/// A text field and the button that commits it, held in one panel so they
/// read as a single control rather than as a field that happens to have a
/// button next to it.
fn field_pod<'a>(
    field: impl Into<Element<'a, Message>>,
    action: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(
        row![field.into(), action.into()]
            .spacing(style::GAP_XS)
            .align_y(iced::Alignment::Center),
    )
    .padding(style::GAP_XS)
    .width(Length::Fill)
    .style(style::panel)
    .into()
}

/// One row of a list: a name that takes the slack, then the actions in a
/// fixed-width cluster on the right.
fn list_row<'a>(
    body: impl Into<Element<'a, Message>>,
    style_fn: fn(&iced::Theme) -> container::Style,
) -> Element<'a, Message> {
    container(body.into())
        .padding(ROW_PAD)
        .width(Length::Fill)
        .style(style_fn)
        .into()
}

/// The error banner. Errors here are sentences ("that player is in 3 games"),
/// so they get a full-width panel rather than a toast.
fn error_banner<'a>(message: &'a str) -> Element<'a, Message> {
    container(text(message).size(style::T_LABEL))
        .padding(style::GAP)
        .width(Length::Fill)
        .style(style::panel_danger)
        .into()
}

// ---------------------------------------------------------------------------
// Roster
// ---------------------------------------------------------------------------

pub fn view<'a>(
    state: &'a PlayersState,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    if let Some(managed) = &state.managing {
        if managed.art_for.is_some() {
            return art_view(managed, image_cache);
        }
        if managed.search_for.is_some() {
            return search_view(state, managed, image_cache);
        }
        // A deck's own page sits on top of the deck list, the same way the
        // art picker does.
        if let Some(page) = &managed.deck_page {
            return deck_page_view(state, managed, page);
        }
        return manage_view(state, managed, image_cache);
    }

    if let Some(player) = state
        .profile
        .and_then(|id| state.players.iter().find(|p| p.id == id))
    {
        let mut content = column![
            screen_header(
                "Player profile".to_string(),
                style::T_TITLE,
                Message::Players(PlayersMessage::CloseProfile)
            ),
            container(
                row![
                    avatar(player, state.pictures.get(&player.id), 112.0),
                    column![
                        text(&player.name).size(style::T_DISPLAY),
                        text("Player profile")
                            .size(style::T_LABEL)
                            .color(style::TEXT_MUTED)
                    ]
                    .spacing(8)
                ]
                .spacing(24)
                .align_y(iced::Alignment::Center)
            )
            .padding(24)
            .width(Length::Fill)
            .style(style::panel),
            if state.editing.is_some() || state.confirming_delete.is_some() {
                player_row(state, player)
            } else {
                iced::widget::responsive(move |size| {
                    let height = ((size.height - 16.0) / 2.0).clamp(180.0, 320.0);
                    scrollable(
                        column![
                            row![
                                profile_action(
                                    crate::icon::Glyph::Decks,
                                    "Commanders",
                                    "Browse and manage decks",
                                    PlayersMessage::ManageCommanders(player.clone()),
                                    height,
                                    false
                                ),
                                profile_action(
                                    crate::icon::Glyph::Edit,
                                    "Rename",
                                    "Change this player's name",
                                    PlayersMessage::StartEdit(player.id, player.name.clone()),
                                    height,
                                    false
                                ),
                            ]
                            .spacing(16),
                            row![
                                profile_action(
                                    crate::icon::Glyph::Delete,
                                    "Delete",
                                    "Remove an unused profile",
                                    PlayersMessage::AskDelete(player.id),
                                    height,
                                    true
                                ),
                                profile_action(
                                    crate::icon::Glyph::Image,
                                    if state.pictures.contains_key(&player.id) {
                                        "Change profile picture"
                                    } else {
                                        "Add profile picture"
                                    },
                                    if state.choosing_picture {
                                        "Choose an image in the file picker"
                                    } else {
                                        "Choose a photo from this laptop"
                                    },
                                    PlayersMessage::ChoosePicture(player.id),
                                    height,
                                    false
                                ),
                            ]
                            .spacing(16),
                        ]
                        .spacing(16),
                    )
                    .into()
                })
                .into()
            },
        ]
        .spacing(style::GAP);
        if let Some(error) = &state.error {
            content = content.push(error_banner(error));
        }
        return with_keyboard(
            state,
            container(content.padding(style::GAP))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        );
    }

    let roster: Element<Message> = if state.players.is_empty() {
        empty_fill(
            "No players yet",
            "Add everyone who sits at this table - they'll keep their commanders and their record.",
        )
    } else {
        player_grid(
            state.players.iter().collect(),
            &state.pictures,
            "View profile",
            |p| Message::Players(PlayersMessage::OpenProfile(p.id)),
        )
    };

    let mut add_button = style::icon_button(crate::icon::Glyph::Add, "Add Player", style::T_ACTION)
        .width(Length::Fixed(W_WIDE))
        .style(style::primary);
    if !state.new_player_name.trim().is_empty() {
        add_button = add_button.on_press(Message::Players(PlayersMessage::CreatePlayer));
    }

    let add_row = field_pod(
        keyed_field(
            Field::NewPlayer,
            "New player name",
            &state.new_player_name,
            |s| Message::Players(PlayersMessage::NewNameChanged(s)),
        ),
        add_button,
    );

    let mut content = column![
        screen_header("Players".to_string(), style::T_TITLE, Message::GoHome),
        add_row,
        roster,
    ]
    .spacing(style::GAP);

    if let Some(e) = &state.error {
        content = content.push(error_banner(e)).push(
            style::touch_button("Reload players", style::T_LABEL)
                .style(style::ghost)
                .on_press(Message::Players(PlayersMessage::RetryLoad)),
        );
    }

    with_keyboard(
        state,
        container(content.padding(style::GAP))
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    )
}

fn profile_action(
    glyph: crate::icon::Glyph,
    title: &str,
    caption: &str,
    action: PlayersMessage,
    height: f32,
    danger: bool,
) -> Element<'static, Message> {
    let compact = height < 220.0;
    button(
        container(
            column![
                row![
                    crate::icon::view(
                        glyph,
                        if compact { 28.0 } else { 36.0 },
                        if danger {
                            style::DANGER
                        } else {
                            style::ACCENT_BRIGHT
                        }
                    ),
                    iced::widget::horizontal_space(),
                    crate::icon::view(crate::icon::Glyph::Next, 24.0, style::TEXT_MUTED)
                ],
                iced::widget::vertical_space(),
                text(title.to_string()).size(if compact {
                    style::T_SUBHEAD
                } else {
                    style::T_HEADING
                }),
                text(caption.to_string())
                    .size(if compact {
                        style::T_CAPTION
                    } else {
                        style::T_BODY
                    })
                    .color(style::TEXT_MUTED),
            ]
            .spacing(if compact { 8 } else { 12 }),
        )
        .padding(if compact { 16 } else { 24 }),
    )
    .padding(0)
    .width(Length::Fill)
    .height(height)
    .style(style::row_button)
    .on_press(Message::Players(action))
    .into()
}

pub fn avatar<'a>(
    player: &'a Player,
    picture: Option<&'a image::Handle>,
    size: f32,
) -> Element<'a, Message> {
    let content: Element<Message> = if let Some(handle) = picture {
        image(handle.clone())
            .width(size)
            .height(size)
            .content_fit(iced::ContentFit::Cover)
            .into()
    } else {
        let initials: String = player
            .name
            .split_whitespace()
            .take(2)
            .filter_map(|w| w.chars().next())
            .flat_map(char::to_uppercase)
            .collect();
        container(text(initials).size(size * 0.4).color(style::ACCENT_BRIGHT))
            .center_x(size)
            .center_y(size)
            .into()
    };
    container(content)
        .width(size)
        .height(size)
        .style(style::badge)
        .clip(true)
        .into()
}

pub fn player_grid<'a>(
    players: Vec<&'a Player>,
    pictures: &'a HashMap<i64, image::Handle>,
    caption: &'static str,
    action: impl Fn(Player) -> Message + 'a,
) -> Element<'a, Message> {
    iced::widget::responsive(move |size| {
        let columns = if size.width >= 1100.0 { 3 } else { 2 };
        let width = (size.width - 16.0 * columns as f32) / columns as f32;
        let height = ((size.height - 16.0) / players.len().div_ceil(columns).max(1) as f32)
            .clamp(240.0, 300.0);
        let rows = players
            .chunks(columns)
            .map(|group| {
                row(group
                    .iter()
                    .map(|player| {
                        button(
                            container(
                                column![
                                    row![
                                        avatar(player, pictures.get(&player.id), 96.0),
                                        iced::widget::horizontal_space(),
                                        crate::icon::view(
                                            crate::icon::Glyph::Next,
                                            24.0,
                                            style::ACCENT_BRIGHT
                                        )
                                    ],
                                    iced::widget::vertical_space(),
                                    text(&player.name).size(style::T_HEADING),
                                    text(caption)
                                        .size(style::T_CAPTION)
                                        .color(style::TEXT_MUTED),
                                ]
                                .spacing(8),
                            )
                            .padding(24),
                        )
                        .padding(0)
                        .width(width)
                        .height(height)
                        .style(style::row_button)
                        .on_press(action((*player).clone()))
                        .into()
                    })
                    .collect::<Vec<Element<Message>>>())
                .spacing(16)
                .into()
            })
            .collect::<Vec<Element<Message>>>();
        scrollable(column(rows).spacing(16)).into()
    })
    .into()
}

fn player_row<'a>(state: &'a PlayersState, p: &'a Player) -> Element<'a, Message> {
    // Renaming: the field stands in for the name, and the row is lit so it's
    // obvious which person is being edited.
    if let Some((id, name)) = &state.editing {
        if *id == p.id {
            return list_row(
                row![
                    keyed_field(Field::Rename, "Player name", name, |s| Message::Players(
                        PlayersMessage::NameChanged(s)
                    )),
                    row![
                        style::touch_button("Save", style::T_LABEL)
                            .width(Length::Fixed(W_ACTION))
                            .style(style::success)
                            .on_press(Message::Players(PlayersMessage::Save)),
                        style::touch_button("Cancel", style::T_LABEL)
                            .width(Length::Fixed(W_NARROW))
                            .style(style::ghost)
                            .on_press(Message::Players(PlayersMessage::Cancel)),
                    ]
                    .spacing(style::GAP_SM),
                ]
                .spacing(style::GAP)
                .align_y(iced::Alignment::Center),
                style::panel_active,
            );
        }
    }

    // Confirming a delete: the whole row turns into the question, and this
    // is the one moment the destructive action is allowed to shout.
    if state.confirming_delete == Some(p.id) {
        return list_row(
            row![
                column![
                    text(format!("Delete {}?", p.name))
                        .size(style::T_SUBHEAD)
                        .color(style::TEXT),
                    text("This can't be undone.")
                        .size(style::T_CAPTION)
                        .color(style::TEXT_MUTED),
                ]
                .spacing(style::GAP_XS)
                .width(Length::Fill),
                row![
                    style::touch_button("Yes, delete", style::T_LABEL)
                        .width(Length::Fixed(W_WIDE))
                        .style(style::danger)
                        .on_press(Message::Players(PlayersMessage::ConfirmDelete(p.id))),
                    style::touch_button("Keep", style::T_LABEL)
                        .width(Length::Fixed(W_NARROW))
                        .style(style::secondary)
                        .on_press(Message::Players(PlayersMessage::CancelDelete)),
                ]
                .spacing(style::GAP_SM),
            ]
            .spacing(style::GAP)
            .align_y(iced::Alignment::Center),
            style::panel_danger,
        );
    }

    // Resting state. Delete is a ghost: findable, but it doesn't sit there
    // in red next to the two harmless actions - the confirmation step is
    // where the warning belongs.
    list_row(
        row![
            text(p.name.clone())
                .size(style::T_SUBHEAD)
                .color(style::TEXT)
                .width(Length::Fill),
            row![
                style::touch_button("Commanders", style::T_LABEL)
                    .width(Length::Fixed(W_WIDE))
                    .style(style::secondary)
                    .on_press(Message::Players(PlayersMessage::ManageCommanders(
                        p.clone()
                    ))),
                style::icon_button(crate::icon::Glyph::Edit, "Rename", style::T_LABEL)
                    .width(Length::Fixed(W_ACTION))
                    .style(style::secondary)
                    .on_press(Message::Players(PlayersMessage::StartEdit(
                        p.id,
                        p.name.clone()
                    ))),
                style::icon_button(crate::icon::Glyph::Delete, "Delete", style::T_LABEL)
                    .width(Length::Fixed(W_NARROW))
                    .style(style::danger_ghost)
                    .on_press(Message::Players(PlayersMessage::AskDelete(p.id))),
            ]
            .spacing(style::GAP_SM),
        ]
        .spacing(style::GAP)
        .align_y(iced::Alignment::Center),
        style::panel,
    )
}

// ---------------------------------------------------------------------------
// One player's decks
//
// A deck is a picture of its commander, at a size you can recognise from
// across a table. Everything that can be done to one lives on a bar under
// the grid rather than on the tile itself, so ten decks read as ten cards
// instead of thirty buttons - and the grid gets the whole screen, which is
// what someone opening their deck list came to see.
// ---------------------------------------------------------------------------

fn manage_view<'a>(
    state: &'a PlayersState,
    managed: &'a ManagedPlayer,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    if let Some(deck) = managed.selected.and_then(|id| managed.deck(id)) {
        return selected_deck_view(state, managed, deck, image_cache);
    }
    let grid: Element<Message> = if managed.commanders.is_empty() {
        empty_fill(
            "No decks yet",
            "Add one and it's here every time this player sits down.",
        )
    } else {
        cards::adaptive_grid(managed.commanders.len(), move |i, width| {
            let deck = &managed.commanders[i];
            cards::deck_tile(
                deck,
                managed.selected == Some(deck.commander.id),
                image_cache,
                deck_meta(managed, deck),
                Message::Players(PlayersMessage::SelectDeck(deck.commander.id)),
                Message::Players(PlayersMessage::OpenDeckPage(deck.commander.id)),
                width,
            )
        })
    };

    let caption = match managed.commanders.len() {
        0 => "Nothing saved yet".to_string(),
        1 => "1 deck".to_string(),
        n => format!("{n} decks"),
    };

    let mut content = column![
        screen_header(
            format!("{}'s Decks", managed.player.name),
            style::T_HEADING,
            Message::Players(PlayersMessage::CloseManage),
        ),
        section_label(caption),
        grid,
        deck_bar(),
    ]
    .spacing(style::GAP);

    if let Some(e) = &state.error {
        content = content.push(error_banner(e));
    }

    container(content.padding(style::GAP))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn deck_bar() -> Element<'static, Message> {
    container(
        row![
            text("Choose a deck to view its scores and options")
                .size(style::T_BODY)
                .color(style::TEXT_MUTED)
                .width(Length::Fill),
            style::icon_button(crate::icon::Glyph::Add, "Add a deck", style::T_ACTION)
                .width(W_WIDE)
                .style(style::primary)
                .on_press(Message::Players(PlayersMessage::OpenSearch(
                    SearchFor::Deck
                ))),
        ]
        .spacing(16)
        .align_y(iced::Alignment::Center),
    )
    .padding(16)
    .style(style::panel)
    .into()
}

fn selected_deck_view<'a>(
    state: &'a PlayersState,
    managed: &'a ManagedPlayer,
    deck: &'a SavedDeck,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let body = iced::widget::responsive(move |size| {
        use crate::icon::Glyph;
        let height = (size.height / 3.0 - 12.0).clamp(180.0, 260.0);
        let meta = deck_meta(managed, deck);
        let scores = match (meta.bracket, meta.salt) {
            (Some(b), Some(s)) => format!("Bracket {b} · {s:.0} salt"),
            _ => "Link a deck or view its analysis".to_string(),
        };
        let mut options = vec![
            profile_action(
                Glyph::Stats,
                "Salt & bracket",
                &scores,
                PlayersMessage::OpenDeckPage(deck.commander.id),
                height,
                false,
            ),
            profile_action(
                Glyph::Image,
                "Commander art",
                &deck.commander.name,
                PlayersMessage::ChangeArt(deck.commander.clone()),
                height,
                false,
            ),
            profile_action(
                Glyph::Players,
                if deck.partner.is_some() {
                    "Unpair commanders"
                } else {
                    "Set partner"
                },
                "Manage this deck's partner",
                if deck.partner.is_some() {
                    PlayersMessage::Unpair(deck.commander.clone())
                } else {
                    PlayersMessage::OpenSearch(SearchFor::Partner(deck.commander.clone()))
                },
                height,
                false,
            ),
        ];
        if let Some(partner) = &deck.partner {
            options.push(profile_action(
                Glyph::Image,
                "Partner art",
                &partner.name,
                PlayersMessage::ChangeArt(partner.clone()),
                height,
                false,
            ));
        }
        options.push(profile_action(
            Glyph::Delete,
            "Remove deck",
            "Remove from this player's collection",
            PlayersMessage::RemoveCommander(deck.commander.id),
            height,
            true,
        ));
        options.push(profile_action(
            Glyph::Edit,
            "Change commander",
            "Choose a replacement for this deck",
            PlayersMessage::OpenSearch(SearchFor::Replace(deck.commander.clone())),
            height,
            false,
        ));
        let mut iter = options.into_iter();
        let mut rows = Vec::new();
        while let Some(first) = iter.next() {
            rows.push(
                row![
                    first,
                    iter.next()
                        .unwrap_or_else(|| iced::widget::horizontal_space().into())
                ]
                .spacing(16)
                .into(),
            );
        }
        let preview = container(crate::art::framed_pair(
            &deck.commander,
            deck.partner.as_ref(),
            image_cache,
            style::T_LABEL,
            0.0,
        ))
        .padding(3)
        .style(style::panel)
        .clip(true);
        row![
            preview.width(Length::FillPortion(2)).height(Length::Fill),
            scrollable(column(rows).spacing(16))
                .width(Length::FillPortion(3))
                .height(Length::Fill)
        ]
        .spacing(24)
        .into()
    });
    let mut content = column![
        screen_header(
            deck.label(),
            style::T_HEADING,
            Message::Players(PlayersMessage::SelectDeck(deck.commander.id))
        ),
        row![
            text(format!("{}'s deck", managed.player.name))
                .size(style::T_LABEL)
                .color(style::TEXT_MUTED),
            cards::mana_row(&cards::identity(deck))
        ]
        .spacing(16)
        .align_y(iced::Alignment::Center),
        body,
    ]
    .spacing(16);
    if let Some(error) = &state.error {
        content = content.push(error_banner(error));
    }
    container(content)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// One deck's page: the Moxfield link at the top, and whatever has been
/// worked out from it underneath.
fn deck_page_view<'a>(
    state: &'a PlayersState,
    managed: &'a ManagedPlayer,
    page: &'a DeckPage,
) -> Element<'a, Message> {
    let linked = managed.links.contains_key(&page.commander_id);

    let action_label = if page.busy {
        "Checking..."
    } else if linked {
        "Re-check"
    } else {
        "Check"
    };
    let mut action = style::touch_button(action_label, style::T_ACTION)
        .width(Length::Fixed(W_ACTION))
        .style(style::primary);
    // Unpressable while a check is running, so a double tap can't start two
    // analyses of the same deck.
    if !page.busy {
        // Re-checking an unchanged link shouldn't need the link re-parsed,
        // but going through `SaveLink` means an edited link is picked up too.
        action = action.on_press(Message::Players(PlayersMessage::SaveLink));
    }

    let mut controls = row![field_pod(
        keyed_field(
            Field::MoxfieldLink,
            "https://moxfield.com/decks/...",
            &page.link_input,
            |s| Message::Players(PlayersMessage::LinkChanged(s)),
        ),
        action,
    )]
    .spacing(style::GAP)
    .align_y(iced::Alignment::Center);

    if linked {
        controls = controls.push(
            style::touch_button("Unlink", style::T_LABEL)
                .width(Length::Fixed(W_NARROW))
                .style(style::danger_ghost)
                .on_press(Message::Players(PlayersMessage::RemoveLink)),
        );
    }

    let body: Element<Message> = match &page.analysis {
        Some(analysis) => breakdown::body(analysis),
        None if page.busy => empty_fill(
            "Reading the list...",
            "Fetching the deck, then scoring every card. The first deck takes longest - after that most cards are already known.",
        ),
        None => empty_fill(
            "Paste this deck's Moxfield link",
            "You'll get its bracket, the cards that set it, and how salty the list is - all of it saved, so it's here without wifi next time.",
        ),
    };

    let mut content = column![
        screen_header(
            page.deck_label.clone(),
            style::T_HEADING,
            Message::Players(PlayersMessage::CloseDeckPage),
        ),
        controls,
    ]
    .spacing(style::GAP);

    if let Some(e) = &state.error {
        content = content.push(error_banner(e));
    }

    with_keyboard(
        state,
        container(content.push(body).padding(style::GAP))
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    )
}

/// Pick a different printing's art for one of this player's commanders.
fn art_view<'a>(
    managed: &'a ManagedPlayer,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let target = managed.art_for.as_ref().unwrap();

    let tiles: Vec<Element<Message>> = managed
        .art_options
        .iter()
        .map(|card| {
            let thumb: Element<Message> =
                match card.small_url.as_deref().and_then(|u| image_cache.get(u)) {
                    Some(handle) => cards::card_picture(handle, ART_W),
                    None => container(
                        text("Loading")
                            .size(style::T_CAPTION)
                            .color(style::TEXT_MUTED),
                    )
                    .center_x(Length::Fixed(ART_W))
                    .center_y(Length::Fixed(ART_H))
                    .into(),
                };
            button(
                column![
                    thumb,
                    text(card.set_name.clone())
                        .size(style::T_CAPTION)
                        .color(style::TEXT_MUTED),
                ]
                .spacing(style::GAP_XS)
                .align_x(iced::Alignment::Center),
            )
            .padding(style::GAP_XS)
            .style(style::secondary)
            .on_press(Message::Players(PlayersMessage::PickArt(card.clone())))
            .into()
        })
        .collect();

    let body: Element<Message> = if tiles.is_empty() {
        let (headline, note) = if managed.loading_art {
            (
                "Looking up printings",
                "Fetching every version from Scryfall.",
            )
        } else {
            (
                "No printings found",
                "Scryfall had nothing else for this card - the current art stays.",
            )
        };
        empty_fill(headline, note)
    } else {
        cards::grid(tiles)
    };

    let status = if managed.loading_art {
        "Loading every printing from Scryfall...".to_string()
    } else {
        format!("{} printings found", managed.art_options.len())
    };

    container(
        column![
            screen_header(
                format!("Art for {}", target.name),
                style::T_HEADING,
                Message::Players(PlayersMessage::CancelArt),
            ),
            section_label(status),
            body,
        ]
        .spacing(style::GAP)
        .padding(style::GAP),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Searching Scryfall for a card, whether it's about to become a deck of
/// its own or the second half of one.
///
/// The cards are the list. A commander is a picture first and a name
/// second, and half the reason to search at all is to check you've landed
/// on the right one of the four cards sharing a name.
fn search_view<'a>(
    state: &'a PlayersState,
    managed: &'a ManagedPlayer,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    let looking_for = managed.search_for.as_ref();
    let title = match looking_for {
        Some(SearchFor::Partner(primary)) => format!("A partner for {}", primary.name),
        Some(SearchFor::Replace(previous)) => format!("Replace {}", previous.name),
        _ => format!("A deck for {}", managed.player.name),
    };

    let results: Element<Message> = if managed.searching {
        empty_fill(
            "Searching Scryfall",
            "Looking for commanders whose name matches what you typed.",
        )
    } else if state.cooldown.active() {
        empty_fill(
            "Scryfall asked us to slow down",
            "Search comes back as soon as the countdown on the button runs out.",
        )
    } else if managed.results.is_empty() {
        empty_fill(
            "No cards yet",
            "Type part of a commander's name and tap Search.",
        )
    } else {
        cards::grid(
            managed
                .results
                .iter()
                .map(|card| {
                    cards::card_tile(
                        card,
                        image_cache,
                        Message::Players(PlayersMessage::AddCommander(card.clone())),
                    )
                })
                .collect(),
        )
    };

    let search_label: String = if state.cooldown.active() {
        state.cooldown.label()
    } else if managed.searching {
        "Searching...".into()
    } else {
        "Search".into()
    };
    let mut search_button =
        style::icon_button(crate::icon::Glyph::Search, search_label, style::T_ACTION)
            .width(Length::Fixed(W_WIDE))
            .style(style::primary);
    if !state.cooldown.active() && !managed.searching && !managed.query.trim().is_empty() {
        search_button = search_button.on_press(Message::Players(PlayersMessage::Search));
    }

    let mut content = column![
        screen_header(
            title,
            style::T_HEADING,
            Message::Players(PlayersMessage::CloseSearch),
        ),
        field_pod(
            keyed_field(
                Field::Search,
                "Search Scryfall by name",
                &managed.query,
                |s| Message::Players(PlayersMessage::QueryChanged(s)),
            ),
            search_button,
        ),
    ]
    .spacing(style::GAP);

    // A partner they already own is one tap. This is the old pairing list,
    // kept beside the search rather than instead of it - before, it was the
    // only way in, which meant both halves had to be saved separately
    // before they could ever be a deck.
    if let Some(SearchFor::Partner(primary)) = looking_for {
        let owned: Vec<Element<Message>> = managed
            .commanders
            .iter()
            .filter(|d| d.commander.id != primary.id && d.partner.is_none())
            .map(|d| quick_partner(&d.commander, image_cache))
            .collect();
        if !owned.is_empty() {
            content = content.push(section_label("Or one they already play"));
            content = content.push(row(owned).spacing(style::GAP_SM).wrap());
        }
    }

    content = content.push(results);

    if let Some(e) = &state.error {
        content = content.push(error_banner(e));
    }

    with_keyboard(
        state,
        container(content.padding(style::GAP))
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    )
}

/// What a deck's Moxfield link says about it, for the tile to show. A deck
/// with no link, or one linked but not yet analysed, has nothing to say.
fn deck_meta(managed: &ManagedPlayer, deck: &SavedDeck) -> cards::DeckMeta {
    managed
        .links
        .get(&deck.commander.id)
        .map(|link| cards::DeckMeta {
            bracket: link.bracket,
            salt: link.salt_total,
        })
        .unwrap_or_default()
}

/// A commander this player already has, offered as a partner without a
/// trip through Scryfall.
fn quick_partner<'a>(
    commander: &'a Commander,
    image_cache: &'a HashMap<String, image::Handle>,
) -> Element<'a, Message> {
    button(
        row![
            cards::thumbnail(commander, image_cache),
            text(&commander.name)
                .size(style::T_LABEL)
                .color(style::TEXT),
        ]
        .spacing(style::GAP_SM)
        .align_y(iced::Alignment::Center),
    )
    .padding(style::GAP_XS)
    .style(style::row_button)
    .on_press(Message::Players(PlayersMessage::PickPartner(
        commander.clone(),
    )))
    .into()
}

// ---------------------------------------------------------------------------
// Text entry
//
// There is no hardware keyboard on the table and iced 0.13 can't ask the
// compositor for one, so every field here raises the app's own.
// ---------------------------------------------------------------------------

/// A text field that brings the keyboard up when it's tapped.
///
/// `text_input` captures the press that focuses it but lets the release go
/// past, so the release is what the keyboard listens for. The field keeps
/// its own caret and a real keyboard still works alongside.
fn keyed_field<'a>(
    field: Field,
    placeholder: &'a str,
    value: &'a str,
    on_input: fn(String) -> Message,
) -> Element<'a, Message> {
    mouse_area(
        text_input(placeholder, value)
            .id(field.id())
            .size(style::T_SUBHEAD)
            .padding(FIELD_PAD)
            .style(style::input)
            .on_input(on_input)
            .on_submit(Message::Players(field.action())),
    )
    .on_release(Message::Players(PlayersMessage::Focus(field)))
    .into()
}

/// Puts the keyboard under a screen while one of its fields is being typed
/// into. It takes its space from the content rather than floating over it:
/// what you're typing into is the thing you most need to keep seeing.
fn with_keyboard<'a>(state: &PlayersState, body: Element<'a, Message>) -> Element<'a, Message> {
    let Some(field) = state.kb.field() else {
        return body;
    };
    column![
        container(body).height(Length::Fill),
        keyboard::view(
            &state.kb,
            |key| Message::Players(PlayersMessage::Key(key)),
            Some(field.action_label()),
        ),
    ]
    .into()
}

#[cfg(test)]
mod picture_tests {
    use super::*;

    #[test]
    fn pictures_are_cropped_and_stored_as_portable_png() {
        let photo = ::image::DynamicImage::ImageRgba8(::image::RgbaImage::from_pixel(
            100,
            60,
            ::image::Rgba([40, 80, 120, 255]),
        ));
        let mut source = std::io::Cursor::new(Vec::new());
        photo
            .write_to(&mut source, ::image::ImageOutputFormat::Png)
            .unwrap();
        let saved = normalize_picture(&source.into_inner()).unwrap();
        let decoded = ::image::load_from_memory(&saved).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (512, 512));
        assert_eq!(decoded.to_rgba8().get_pixel(256, 256).0, [40, 80, 120, 255]);
        assert!(normalize_picture(b"not an image").is_err());
    }

    #[test]
    fn cancelling_a_picture_preserves_the_previous_picture() {
        let conn = Connection::open_in_memory().unwrap();
        let mut state = PlayersState::load(&conn);
        state
            .pictures
            .insert(1, image::Handle::from_rgba(1, 1, vec![1, 2, 3, 255]));
        state.choosing_picture = true;
        let _ = update(
            &mut state,
            &conn,
            PlayersMessage::PictureChosen(1, Ok(None)),
        );
        assert!(state.pictures.contains_key(&1));
        assert!(!state.choosing_picture);
        assert!(state.error.is_none());
    }
}
