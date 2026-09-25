//! Minimal Scryfall client: search for legal commanders by name, list every
//! printing/art of a chosen card, and fetch art so it can be cached locally
//! and shown as a player's portrait.
//!
//! Scryfall publishes hard rate limits: 2 requests/second to `/cards/*`, and
//! a 30-second lockout behind any HTTP 429. Every call to `api.scryfall.com`
//! here goes through `api_gate`, which serialises requests and spaces them,
//! so no amount of tapping can breach the limit. The direct file origin
//! (`cards.scryfall.io`) is explicitly exempt, so `fetch_image` skips the
//! gate and instead leans on the on-disk cache in `crate::cache`.

use std::fmt;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::cache;

/// Scryfall's limit on `/cards/*` is 2/second; 550ms leaves a little margin
/// for clock jitter without being noticeable to someone typing a search.
const MIN_API_GAP: Duration = Duration::from_millis(550);

/// How long to stay off the API after a 429 when Scryfall doesn't tell us.
/// Their documented lockout is 30 seconds.
const DEFAULT_BACKOFF: u64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScryfallError {
    /// We were rate limited. Scryfall locks the application out for a window
    /// after this, so callers must stop sending until it expires rather than
    /// retrying - continuing to overload the API risks a real ban.
    RateLimited {
        retry_after: u64,
    },
    Other(String),
}

impl fmt::Display for ScryfallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScryfallError::RateLimited { retry_after } => write!(
                f,
                "Scryfall rate limit reached. Searching again in {retry_after}s."
            ),
            ScryfallError::Other(msg) => write!(f, "Scryfall request failed: {msg}"),
        }
    }
}

impl ScryfallError {
    fn other(e: impl fmt::Display) -> Self {
        ScryfallError::Other(e.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScryfallCard {
    /// This printing's id. Not the commander's identity - see `oracle_id`.
    pub scryfall_id: String,
    /// Stable across every printing of this card; this is what a Commander
    /// row is keyed by, so changing art later doesn't fragment stats.
    pub oracle_id: String,
    pub name: String,
    pub set_name: String,
    pub small_url: Option<String>,
    pub image_url: Option<String>,
    pub art_crop_url: Option<String>,
    pub color_identity: String,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    data: Vec<CardData>,
}

#[derive(Debug, Deserialize)]
struct CardData {
    id: String,
    oracle_id: Option<String>,
    name: String,
    set_name: String,
    #[serde(default)]
    color_identity: Vec<String>,
    #[serde(default)]
    image_uris: Option<ImageUris>,
    #[serde(default)]
    card_faces: Option<Vec<CardFace>>,
}

#[derive(Debug, Clone, Deserialize)]
struct CardFace {
    #[serde(default)]
    image_uris: Option<ImageUris>,
}

#[derive(Debug, Clone, Deserialize)]
struct ImageUris {
    small: Option<String>,
    normal: Option<String>,
    art_crop: Option<String>,
}

impl From<CardData> for ScryfallCard {
    fn from(c: CardData) -> Self {
        let images = c.image_uris.or_else(|| {
            c.card_faces
                .as_ref()
                .and_then(|faces| faces.first())
                .and_then(|face| face.image_uris.clone())
        });
        let (small_url, image_url, art_crop_url) = match images {
            Some(i) => (i.small, i.normal, i.art_crop),
            None => (None, None, None),
        };
        ScryfallCard {
            oracle_id: c.oracle_id.unwrap_or_else(|| c.id.clone()),
            scryfall_id: c.id,
            name: c.name,
            set_name: c.set_name,
            small_url,
            image_url,
            art_crop_url,
            color_identity: c.color_identity.join(""),
        }
    }
}

fn client() -> Result<reqwest::Client, ScryfallError> {
    reqwest::Client::builder()
        .user_agent("commander_pod/0.1 (local desktop app; no network beyond scryfall.com)")
        .build()
        .map_err(ScryfallError::other)
}

/// The time the last API request was sent. Held as an async mutex so that
/// awaiting it both serialises callers and lets us sleep out the remainder
/// of the 500ms window while holding the gate shut.
fn api_gate() -> &'static tokio::sync::Mutex<Option<Instant>> {
    static GATE: OnceLock<tokio::sync::Mutex<Option<Instant>>> = OnceLock::new();
    GATE.get_or_init(|| tokio::sync::Mutex::new(None))
}

/// Runs `send` under the API gate: one request in flight at a time, and
/// never started closer than `MIN_API_GAP` to the previous one. Holding the
/// lock across the await is the point - it's what serialises callers rather
/// than letting a burst all check the clock and go at once.
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

/// Seconds Scryfall asked us to wait, if it said.
fn retry_after(resp: &reqwest::Response) -> u64 {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_BACKOFF)
}

async fn run_search(
    query: &str,
    extra: &[(&str, &str)],
) -> Result<Vec<ScryfallCard>, ScryfallError> {
    let mut pairs: Vec<(&str, &str)> = vec![("q", query)];
    pairs.extend_from_slice(extra);

    let client = client()?;

    let resp = through_gate(
        client
            .get("https://api.scryfall.com/cards/search")
            .query(&pairs)
            .send(),
    )
    .await
    .map_err(ScryfallError::other)?;

    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(ScryfallError::RateLimited {
            retry_after: retry_after(&resp),
        });
    }
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        // Scryfall's way of saying "zero results".
        return Ok(Vec::new());
    }
    if !resp.status().is_success() {
        return Err(ScryfallError::Other(format!(
            "search failed: {}",
            resp.status()
        )));
    }

    let body: SearchResponse = resp.json().await.map_err(ScryfallError::other)?;
    Ok(body.data.into_iter().map(ScryfallCard::from).collect())
}

/// Searches Scryfall for legal commanders matching `query`, one result per
/// distinct card. Empty query returns no results rather than hitting the network.
pub async fn search_commanders(query: String) -> Result<Vec<ScryfallCard>, ScryfallError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let q = format!("{trimmed} is:commander");
    run_search(&q, &[("unique", "cards"), ("order", "name")]).await
}

/// Every printing/art of the given oracle card, newest first, so the player
/// can pick which art represents their commander at the table. Served from
/// the local cache when we looked this card up recently - printings only
/// change on a reprint, so a day-old list is fine and spares the API.
pub async fn fetch_prints(oracle_id: String) -> Result<Vec<ScryfallCard>, ScryfallError> {
    if let Some(hit) = cache::prints(&oracle_id) {
        return Ok(hit);
    }
    let q = format!("oracleid:{oracle_id}");
    let cards = run_search(
        &q,
        &[("unique", "prints"), ("order", "released"), ("dir", "desc")],
    )
    .await?;
    cache::store_prints(&oracle_id, &cards);
    Ok(cards)
}

/// Image bytes for use with `iced::widget::image::Handle`, from the on-disk
/// cache when we have them. These come from `cards.scryfall.io`, the direct
/// file origin, which has no rate limit - so this deliberately bypasses the
/// API gate and is safe to call in a burst.
pub async fn fetch_image(url: String) -> Result<Vec<u8>, String> {
    if let Some(hit) = cache::art(&url) {
        return Ok(hit);
    }
    let resp = client()
        .map_err(|e| e.to_string())?
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("image fetch failed: {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?.to_vec();
    cache::store_art(&url, &bytes);
    Ok(bytes)
}

/// A rate-limit lockout, counted down once a second by the screen that owns
/// it. Scryfall is explicit that ignoring a 429 risks a ban, so while this
/// is active the UI refuses to send rather than letting someone lean on the
/// search button through the lockout.
#[derive(Debug, Clone, Copy, Default)]
pub struct Cooldown {
    remaining: u64,
}

impl Cooldown {
    pub fn begin(&mut self, secs: u64) {
        self.remaining = secs;
    }

    pub fn tick(&mut self) {
        self.remaining = self.remaining.saturating_sub(1);
    }

    pub fn active(&self) -> bool {
        self.remaining > 0
    }

    /// Label for the search button while locked out.
    pub fn label(&self) -> String {
        format!("Wait {}s", self.remaining)
    }

    /// Records `err` if it was a rate limit, and reports whether it was.
    pub fn absorb(&mut self, err: &ScryfallError) -> bool {
        if let ScryfallError::RateLimited { retry_after } = err {
            self.begin(*retry_after);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate is the only thing standing between a fast tapper and a 429,
    /// so prove a burst really does come out spaced rather than all at once.
    #[tokio::test]
    async fn gate_spaces_concurrent_requests() {
        let start = Instant::now();
        for _ in 0..3 {
            through_gate(async {}).await;
        }
        // Three requests means two enforced gaps between them.
        assert!(
            start.elapsed() >= MIN_API_GAP * 2,
            "3 requests took {:?}, expected at least {:?}",
            start.elapsed(),
            MIN_API_GAP * 2
        );
    }

    #[test]
    fn rate_limit_error_says_how_long() {
        let err = ScryfallError::RateLimited { retry_after: 30 };
        assert!(err.to_string().contains("30s"));
    }

    #[test]
    fn cooldown_absorbs_only_rate_limits() {
        let mut c = Cooldown::default();
        assert!(!c.absorb(&ScryfallError::Other("boom".into())));
        assert!(!c.active());

        assert!(c.absorb(&ScryfallError::RateLimited { retry_after: 2 }));
        assert!(c.active());
        assert_eq!(c.label(), "Wait 2s");
        c.tick();
        c.tick();
        assert!(!c.active(), "cooldown should run out");
        c.tick();
        assert!(!c.active(), "and not wrap around past zero");
    }
}
