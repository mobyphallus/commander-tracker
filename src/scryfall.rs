//! Minimal Scryfall client: search for legal commanders by name and fetch
//! their art so it can be cached locally and shown as a player's portrait.

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct ScryfallCard {
    pub scryfall_id: String,
    pub name: String,
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
    name: String,
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
        let (image_url, art_crop_url) = match images {
            Some(i) => (i.normal, i.art_crop),
            None => (None, None),
        };
        ScryfallCard {
            scryfall_id: c.id,
            name: c.name,
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

/// Searches Scryfall for legal commanders matching `query`. Empty query
/// returns no results rather than hitting the network.
pub async fn search_commanders(query: String) -> Result<Vec<ScryfallCard>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let q = format!("{trimmed} is:commander");
    let resp = client()?
        .get("https://api.scryfall.com/cards/search")
        .query(&[("q", q.as_str()), ("unique", "cards"), ("order", "name")])
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
