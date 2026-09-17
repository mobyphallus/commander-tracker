//! Minimal Scryfall client: search for legal commanders by name, list every
//! printing/art of a chosen card, and fetch art so it can be cached locally
//! and shown as a player's portrait.

use serde::Deserialize;

#[derive(Debug, Clone)]
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

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("commander_pod/0.1 (local desktop app; no network beyond scryfall.com)")
        .build()
        .map_err(|e| e.to_string())
}

async fn run_search(query: &str, extra: &[(&str, &str)]) -> Result<Vec<ScryfallCard>, String> {
    let mut pairs: Vec<(&str, &str)> = vec![("q", query)];
    pairs.extend_from_slice(extra);
    let resp = client()?
        .get("https://api.scryfall.com/cards/search")
        .query(&pairs)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        // Scryfall's way of saying "zero results".
        return Ok(Vec::new());
    }
    if !resp.status().is_success() {
        return Err(format!("Scryfall search failed: {}", resp.status()));
    }

    let body: SearchResponse = resp.json().await.map_err(|e| e.to_string())?;
    Ok(body.data.into_iter().map(ScryfallCard::from).collect())
}

/// Searches Scryfall for legal commanders matching `query`, one result per
/// distinct card. Empty query returns no results rather than hitting the network.
pub async fn search_commanders(query: String) -> Result<Vec<ScryfallCard>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let q = format!("{trimmed} is:commander");
    run_search(&q, &[("unique", "cards"), ("order", "name")]).await
}

/// Every printing/art of the given oracle card, newest first, so the player
/// can pick which art represents their commander at the table.
pub async fn fetch_prints(oracle_id: String) -> Result<Vec<ScryfallCard>, String> {
    let q = format!("oracleid:{oracle_id}");
    run_search(&q, &[("unique", "prints"), ("order", "released"), ("dir", "desc")]).await
}

/// Downloads raw image bytes for use with `iced::widget::image::Handle`.
pub async fn fetch_image(url: String) -> Result<Vec<u8>, String> {
    let resp = client()?
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("image fetch failed: {}", resp.status()));
    }
    Ok(resp.bytes().await.map_err(|e| e.to_string())?.to_vec())
}
