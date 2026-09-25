//! Minimal Moxfield client: turn a deck link into the card list behind it.
//!
//! Moxfield publishes no official API. What's here is the same JSON its own
//! front end calls, which means two things worth remembering: it can change
//! without notice, and we are guests. Moxfield operate a User-Agent
//! allowlist - if this app ever gets used at any volume, mail them and get
//! `USER_AGENT` blessed rather than raising the request rate.
//!
//! No rate limit is published either, so `through_gate` spaces requests a
//! second apart, the same shape as the Scryfall gate in `crate::scryfall`
//! but more conservative because we have nothing documented to aim at.
//!
//! The deck JSON is rich enough that one request funds the whole salt
//! analysis: every card arrives with its oracle text, type line, USD price
//! and reserved-list flag, so `crate::salt` categorises cards locally
//! instead of asking anyone else about them.

use std::fmt;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::Deserialize;

/// Deliberately generous. Moxfield documents no limit, and a pod only ever
/// links a handful of decks, so there's nothing to gain by pushing.
const MIN_API_GAP: Duration = Duration::from_millis(1000);

/// Identifies the app so Moxfield can tell us apart from a browser, and
/// tells them where to look if they want it to stop.
const USER_AGENT: &str =
    "commander_pod/0.1 (local Commander pod tracker; https://github.com/commander_pod)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The text in the box wasn't a Moxfield deck link.
    NotALink,
    /// Moxfield has no deck with that id, or it isn't public.
    NotFound,
    /// The deck exists but has no commander, so it isn't a Commander deck
    /// we can say anything useful about.
    NoCommander,
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotALink => write!(
                f,
                "That doesn't look like a Moxfield deck link. Paste the whole URL."
            ),
            Error::NotFound => write!(f, "Moxfield has no public deck at that link."),
            Error::NoCommander => write!(f, "That deck has no commander set on Moxfield."),
            Error::Other(msg) => write!(f, "Moxfield request failed: {msg}"),
        }
    }
}

impl Error {
    fn other(e: impl fmt::Display) -> Self {
        Error::Other(e.to_string())
    }
}

/// One card in a linked deck, carrying everything the salt pass needs so it
/// never has to look the card up again.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub name: String,
    /// Basics and other duplicates arrive as one entry with a count, so
    /// every per-card score has to be multiplied by this.
    pub quantity: u32,
    pub scryfall_id: String,
    pub oracle_text: String,
    pub type_line: String,
    /// Concatenated WUBRG letters, matching `Commander::color_identity`.
    pub color_identity: String,
    pub usd: Option<f64>,
    pub reserved: bool,
}

/// A linked Moxfield deck, reduced to what this app cares about.
#[derive(Debug, Clone, PartialEq)]
pub struct Deck {
    pub public_id: String,
    pub name: String,
    pub url: String,
    /// The bracket the deck's owner assigned to it, if they did. Self
    /// reported, so it's shown next to our own number rather than trusted.
    pub owner_bracket: Option<u8>,
    /// Moxfield's own calculation from the list. Caps at 4 - bracket 5 is a
    /// declaration of intent no algorithm can read off a decklist.
    pub auto_bracket: Option<u8>,
    pub commanders: Vec<Card>,
    pub mainboard: Vec<Card>,
}

impl Deck {
    /// Commanders and mainboard together, which is what both the bracket
    /// estimate and the salt sum run over.
    pub fn all_cards(&self) -> impl Iterator<Item = &Card> {
        self.commanders.iter().chain(self.mainboard.iter())
    }

    /// Total cards counting duplicates, for "100 cards" style captions.
    pub fn card_count(&self) -> u32 {
        self.all_cards().map(|c| c.quantity).sum()
    }
}

/// Pulls the public id out of whatever someone pasted.
///
/// Accepts a full URL with or without scheme and `www`, with anything
/// trailing (`/primer`, a query string), or the bare id on its own.
pub fn parse_ref(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // A link: take the segment after /decks/.
    if let Some(rest) = trimmed.split("/decks/").nth(1) {
        let id = rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .trim();
        return is_public_id(id).then(|| id.to_string());
    }

    // A bare id. Anything with a slash or a space is a malformed link
    // rather than an id, and is better rejected than half-understood.
    is_public_id(trimmed).then(|| trimmed.to_string())
}

/// Moxfield public ids are base64url-ish: letters, digits, `-` and `_`.
/// Length isn't fixed, so this only rules out the obviously-not.
fn is_public_id(s: &str) -> bool {
    s.len() >= 8
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[derive(Debug, Deserialize)]
struct DeckResponse {
    #[serde(rename = "publicId")]
    public_id: String,
    name: String,
    #[serde(rename = "publicUrl")]
    public_url: Option<String>,
    #[serde(default)]
    bracket: Option<u8>,
    #[serde(rename = "autoBracket", default)]
    auto_bracket: Option<u8>,
    boards: Boards,
}

#[derive(Debug, Deserialize)]
struct Boards {
    #[serde(default)]
    commanders: Board,
    #[serde(default)]
    mainboard: Board,
}

#[derive(Debug, Default, Deserialize)]
struct Board {
    /// Keyed by Moxfield's own card id, which we don't need - only the
    /// entries matter.
    #[serde(default)]
    cards: std::collections::HashMap<String, Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    #[serde(default = "one")]
    quantity: u32,
    card: CardData,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Deserialize)]
struct CardData {
    name: String,
    #[serde(rename = "scryfall_id", default)]
    scryfall_id: String,
    #[serde(rename = "oracle_text", default)]
    oracle_text: String,
    #[serde(rename = "type_line", default)]
    type_line: String,
    #[serde(rename = "color_identity", default)]
    color_identity: Vec<String>,
    #[serde(default)]
    prices: Option<Prices>,
    #[serde(default)]
    reserved: bool,
}

#[derive(Debug, Deserialize)]
struct Prices {
    #[serde(default)]
    usd: Option<f64>,
}

impl From<CardData> for Card {
    fn from(c: CardData) -> Self {
        Card {
            name: c.name,
            // Replaced by the entry's real count in `board_cards`.
            quantity: 1,
            scryfall_id: c.scryfall_id,
            oracle_text: c.oracle_text,
            type_line: c.type_line,
            color_identity: c.color_identity.join(""),
            usd: c.prices.and_then(|p| p.usd),
            reserved: c.reserved,
        }
    }
}

fn board_cards(board: Board) -> Vec<Card> {
    let mut cards: Vec<Card> = board
        .cards
        .into_values()
        .map(|entry| Card {
            quantity: entry.quantity.max(1),
            ..Card::from(entry.card)
        })
        .collect();
    // The JSON is a map, so iteration order is arbitrary; sort so the same
    // deck always analyses and renders in the same order.
    cards.sort_by(|a, b| a.name.cmp(&b.name));
    cards
}

fn client() -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(Error::other)
}

fn api_gate() -> &'static tokio::sync::Mutex<Option<Instant>> {
    static GATE: OnceLock<tokio::sync::Mutex<Option<Instant>>> = OnceLock::new();
    GATE.get_or_init(|| tokio::sync::Mutex::new(None))
}

/// One request at a time, never closer together than `MIN_API_GAP`. The
/// lock is deliberately held across the await - that's what serialises
/// callers instead of letting them all check the clock and go at once.
async fn through_gate<T>(send: impl std::future::Future<Output = T>) -> T {
    let mut last_sent = api_gate().lock().await;
    if let Some(prev) = *last_sent {
        let elapsed = prev.elapsed();
        if elapsed < MIN_API_GAP {
            tokio::time::sleep(MIN_API_GAP - elapsed).await;
        }
    }
    let out = send.await;
    *last_sent = Some(Instant::now());
    out
}

/// Fetches one public deck. `input` is whatever someone pasted in the box.
pub async fn fetch(input: String) -> Result<Deck, Error> {
    let public_id = parse_ref(&input).ok_or(Error::NotALink)?;
    let client = client()?;

    let resp = through_gate(
        client
            .get(format!(
                "https://api2.moxfield.com/v3/decks/all/{public_id}"
            ))
            .send(),
    )
    .await
    .map_err(Error::other)?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(Error::NotFound);
    }
    if !resp.status().is_success() {
        return Err(Error::Other(format!(
            "deck request failed: {}",
            resp.status()
        )));
    }

    let body: DeckResponse = resp.json().await.map_err(Error::other)?;

    let commanders = board_cards(body.boards.commanders);
    if commanders.is_empty() {
        return Err(Error::NoCommander);
    }

    Ok(Deck {
        url: body
            .public_url
            .unwrap_or_else(|| format!("https://moxfield.com/decks/{}", body.public_id)),
        public_id: body.public_id,
        name: body.name,
        owner_bracket: body.bracket,
        auto_bracket: body.auto_bracket,
        commanders,
        mainboard: board_cards(body.boards.mainboard),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_shape_of_link() {
        let id = "kd5-ZBCUzE68xFOI1x-NzA";
        for input in [
            "https://www.moxfield.com/decks/kd5-ZBCUzE68xFOI1x-NzA",
            "https://moxfield.com/decks/kd5-ZBCUzE68xFOI1x-NzA",
            "moxfield.com/decks/kd5-ZBCUzE68xFOI1x-NzA",
            // Trailing path and query, as copied off a primer tab.
            "https://www.moxfield.com/decks/kd5-ZBCUzE68xFOI1x-NzA/primer",
            "https://www.moxfield.com/decks/kd5-ZBCUzE68xFOI1x-NzA?utm=share",
            // Someone pasting just the id, and with stray whitespace.
            "kd5-ZBCUzE68xFOI1x-NzA",
            "  https://www.moxfield.com/decks/kd5-ZBCUzE68xFOI1x-NzA  ",
        ] {
            assert_eq!(parse_ref(input).as_deref(), Some(id), "failed on {input:?}");
        }
    }

    #[test]
    fn rejects_what_isnt_a_deck_link() {
        for input in [
            "",
            "   ",
            // A profile, not a deck.
            "https://www.moxfield.com/users/maxpotter1997",
            // Right host, no id after /decks/.
            "https://www.moxfield.com/decks/",
            // Too short to be an id.
            "abc",
            // A sentence, not an id.
            "my deck",
        ] {
            assert_eq!(parse_ref(input), None, "should have rejected {input:?}");
        }
    }

    #[test]
    fn quantities_survive_and_order_is_stable() {
        let json = r#"{
            "publicId": "abc12345",
            "name": "Test",
            "publicUrl": "https://moxfield.com/decks/abc12345",
            "bracket": 3,
            "autoBracket": 2,
            "boards": {
                "commanders": { "cards": {
                    "k1": { "quantity": 1, "card": { "name": "Kibo, Uktabi Prince",
                        "scryfall_id": "x", "oracle_text": "", "type_line": "Legendary Creature",
                        "color_identity": ["G","R"], "prices": { "usd": 1.5 }, "reserved": false } }
                } },
                "mainboard": { "cards": {
                    "k2": { "quantity": 9, "card": { "name": "Forest", "scryfall_id": "y",
                        "oracle_text": "", "type_line": "Basic Land", "color_identity": ["G"],
                        "prices": { "usd": 0.1 }, "reserved": false } },
                    "k3": { "quantity": 1, "card": { "name": "Ancient Tomb", "scryfall_id": "z",
                        "oracle_text": "", "type_line": "Land", "color_identity": [],
                        "prices": null, "reserved": false } }
                } }
            }
        }"#;
        let body: DeckResponse = serde_json::from_str(json).unwrap();
        let main = board_cards(body.boards.mainboard);

        // Sorted by name, so Ancient Tomb comes before Forest.
        assert_eq!(main[0].name, "Ancient Tomb");
        assert_eq!(main[1].name, "Forest");
        assert_eq!(main[1].quantity, 9, "a basic's count must survive");
        assert_eq!(main[0].usd, None, "a missing price is not a zero price");
        assert_eq!(main[1].usd, Some(0.1));

        let commanders = board_cards(body.boards.commanders);
        assert_eq!(commanders[0].color_identity, "GR");
    }
}
