//! Local deck analysis for pregame conversations.
//!
//! Brackets are estimates: card-count heuristics cannot establish a deck's
//! consistency, intended speed, or competitive intent. Combo starting mana
//! excludes prerequisite setup; only fully specified lines from hand qualify
//! for the low-cost heuristic. Unknown setup is shown for manual review.
//!
//! Salt combines EDHREC per-card scores with local category weights modeled
//! on public commandersalt.com analyses. Each contribution is shown in the
//! breakdown; it is not an official rating or an exact service replica.
//!
//! Three sources feed it, and only the first two cost a request:
//!
//! - **Commander Spellbook** supplies actual combo lines and prerequisites.
//!   Starting mana excludes setup. Only fully specified lines from hand
//!   participate in the local low-cost heuristic; this is not an official
//!   turn-speed classification. See Wizards' October 21, 2025 bracket update:
//!   https://magic.wizards.com/en/news/announcements/commander-brackets-beta-update-october-21-2025
//! - **EDHREC** supplies per-card salt, the largest term in the sum. Cached
//!   per card by `crate::cache`, so a pod pays for its card pool once.
//! - **Moxfield's own deck JSON** supplies oracle text, type lines, prices
//!   and reserved-list flags, so the remaining categories are detected here
//!   with no third request.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::cache;
use crate::moxfield;

/// Salt that reads as 100% on the meter, and the top of the A+ band. This
/// is commandersalt's own ceiling - a deck at 300 is as salty as the scale
/// goes, and cEDH lists land around 200.
pub const SALT_CEILING: f64 = 300.0;

/// How many EDHREC lookups are in flight at once. EDHREC publishes no rate
/// limit, so this is set by politeness rather than by a documented cap: six
/// keeps a cold 100-card deck to a few seconds without hammering anyone.
const SALT_CONCURRENCY: usize = 6;

/// Only cards at or above this price carry price salt. Cheap cards are not
/// what people mean when they complain about someone's mana base, and
/// counting them would make every deck's price term enormous.
///
/// Measured rather than chosen: on a freshly-scored deck the cheapest card
/// that counted was $27.65 and the priciest that didn't was $17.10, so the
/// real cut is somewhere between. $25 is the round number in that window.
const PRICE_SALT_FLOOR: f64 = 25.0;

/// Price salt is the card's price over this. Exact - on a deck scored the
/// same day its prices were read, every single entry divided by 50 to four
/// decimal places.
const PRICE_SALT_DIVISOR: f64 = 50.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Spellbook couldn't classify the deck, so there's no honest bracket to
    /// report. Salt alone would be half an answer, so this fails the lot.
    Bracket(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Bracket(msg) => write!(f, "Couldn't work out the bracket: {msg}"),
        }
    }
}

// ---------------------------------------------------------------------------
// The bracket criteria
// ---------------------------------------------------------------------------

/// The five things Wizards' bracket guidelines actually measure. Each one
/// sets a floor; a deck's bracket is the highest floor any of them sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CriterionKind {
    GameChangers,
    MassLandDenial,
    ExtraTurns,
    TwoCardCombos,
    EarlyGameInfiniteCombos,
}

impl CriterionKind {
    pub const ALL: [CriterionKind; 5] = [
        CriterionKind::GameChangers,
        CriterionKind::MassLandDenial,
        CriterionKind::ExtraTurns,
        CriterionKind::TwoCardCombos,
        CriterionKind::EarlyGameInfiniteCombos,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CriterionKind::GameChangers => "Game Changers",
            CriterionKind::MassLandDenial => "Mass Land Denial",
            CriterionKind::ExtraTurns => "Extra Turns",
            CriterionKind::TwoCardCombos => "Two-Card Combos",
            CriterionKind::EarlyGameInfiniteCombos => "Low-Cost Combos From Hand",
        }
    }

    /// What this criterion measures, in the words of the guidelines. Shown
    /// as the row's explanation so nobody has to take the number on faith.
    pub fn help(self) -> &'static str {
        match self {
            CriterionKind::GameChangers => {
                "Cards on Wizards' Game Changers list. None is bracket 2, one to three is 3, four or more is 4."
            }
            CriterionKind::MassLandDenial => {
                "Symmetrical mass land destruction. Any inclusion means bracket 4."
            }
            CriterionKind::ExtraTurns => {
                "Extra-turn spells. None is bracket 2, up to four is 3, five or more is 4."
            }
            CriterionKind::TwoCardCombos => {
                "Spellbook’s relevant two-card lines. Commanders and setup requirements may still be involved. These set a local estimated floor of 3."
            }
            CriterionKind::EarlyGameInfiniteCombos => {
                "Local estimate: a relevant two-card line starting entirely from hand, with no extra prerequisites, needing at most seven mana. Starting mana alone cannot establish how early a deck wins."
            }
        }
    }

    /// The bracket this criterion floors the deck at, given how many it
    /// found. These are local screening heuristics, not official definitions.
    pub fn floor(self, count: u32) -> u8 {
        match self {
            CriterionKind::GameChangers => match count {
                0 => 2,
                1..=3 => 3,
                _ => 4,
            },
            CriterionKind::ExtraTurns => match count {
                0 => 2,
                1..=4 => 3,
                _ => 4,
            },
            CriterionKind::TwoCardCombos => {
                if count == 0 {
                    2
                } else {
                    3
                }
            }
            CriterionKind::MassLandDenial | CriterionKind::EarlyGameInfiniteCombos => {
                if count == 0 {
                    2
                } else {
                    4
                }
            }
        }
    }
}

/// One criterion's result for a deck.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Criterion {
    pub kind: CriterionKind,
    pub count: u32,
    pub floor: u8,
    /// The cards or combo lines responsible, so the row can be opened up.
    /// Empty when the count is zero.
    pub culprits: Vec<String>,
}

impl Criterion {
    /// Whether this is the criterion that set the deck's bracket. That's the
    /// one worth reading out loud at the table.
    pub fn is_deciding(&self, bracket: u8) -> bool {
        self.floor == bracket && self.count > 0
    }
}

// ---------------------------------------------------------------------------
// The salt categories
// ---------------------------------------------------------------------------

/// A reason a card contributes salt. Several can apply to one card - a card
/// in two or more is a "stackable offender" and hurts twice, which is the
/// point rather than a bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CategoryKind {
    /// The community's own salt vote, per card. The largest term by far.
    Edhrec,
    Locks,
    Mld,
    Boardwipes,
    ExtraTurns,
    Theft,
    Wheels,
    Annihilator,
    Sacrifice,
    GroupSlug,
    OffColorFetches,
    Poison,
    ExtraCombats,
    CantUntap,
    CantCast,
    CantActivate,
    CantAttack,
    CantDraw,
    CantSearch,
    CantTrigger,
    CantGainLife,
    Taxes,
    InfiniteCombos,
    Discard,
    PayTheOne,
    Reserved,
    CardPrice,
}

impl CategoryKind {
    pub fn label(self) -> &'static str {
        match self {
            CategoryKind::Edhrec => "Community salt",
            CategoryKind::Locks => "Locks",
            CategoryKind::Mld => "Mass land denial",
            CategoryKind::Boardwipes => "Board wipes",
            CategoryKind::ExtraTurns => "Extra turns",
            CategoryKind::Theft => "Theft",
            CategoryKind::Wheels => "Wheels",
            CategoryKind::Annihilator => "Annihilator",
            CategoryKind::Sacrifice => "Forced sacrifice",
            CategoryKind::GroupSlug => "Group slug",
            CategoryKind::OffColorFetches => "Off-colour fetches",
            CategoryKind::Poison => "Poison",
            CategoryKind::ExtraCombats => "Extra combats",
            CategoryKind::CantUntap => "Can't untap",
            CategoryKind::CantCast => "Can't cast",
            CategoryKind::CantActivate => "Can't activate",
            CategoryKind::CantAttack => "Can't attack",
            CategoryKind::CantDraw => "Can't draw",
            CategoryKind::CantSearch => "Can't search",
            CategoryKind::CantTrigger => "Can't trigger",
            CategoryKind::CantGainLife => "Can't gain life",
            CategoryKind::Taxes => "Tax effects",
            CategoryKind::InfiniteCombos => "Infinite combos",
            CategoryKind::Discard => "Discard",
            CategoryKind::PayTheOne => "Do you pay the 1?",
            CategoryKind::Reserved => "Reserved list",
            CategoryKind::CardPrice => "Card price",
        }
    }

    /// Salt per offending card.
    ///
    /// Everything here was read off real scored decks, except the four
    /// marked below - those categories never turned up in the decks
    /// sampled, so they're set to the value their neighbours use. They're
    /// the only numbers in this file that are a judgement call, and they're
    /// all in one place so they can be corrected in one edit.
    pub fn weight(self) -> f64 {
        match self {
            CategoryKind::Locks => 20.0,
            CategoryKind::Mld => 10.0,
            CategoryKind::Boardwipes | CategoryKind::ExtraTurns | CategoryKind::Theft => 7.0,
            // Unobserved: grouped with the other "this ends the game on its
            // own" effects.
            CategoryKind::Annihilator => 7.0,
            CategoryKind::Wheels => 6.0,
            CategoryKind::Sacrifice | CategoryKind::GroupSlug | CategoryKind::OffColorFetches => {
                5.0
            }
            // Unobserved: both are alternate-wincon / extra-attack effects,
            // in line with the 5-point band.
            CategoryKind::Poison | CategoryKind::ExtraCombats => 5.0,
            CategoryKind::CantUntap => 4.0,
            CategoryKind::CantCast
            | CategoryKind::CantActivate
            | CategoryKind::CantAttack
            | CategoryKind::CantDraw
            | CategoryKind::CantSearch
            | CategoryKind::CantTrigger
            | CategoryKind::CantGainLife
            | CategoryKind::Taxes
            | CategoryKind::InfiniteCombos
            | CategoryKind::Discard => 3.0,
            CategoryKind::PayTheOne => 2.0,
            CategoryKind::Reserved => 1.5,
            // Both of these are per-card values rather than flat weights,
            // and never go through `weight`.
            CategoryKind::Edhrec | CategoryKind::CardPrice => 0.0,
        }
    }
}

/// One card's contribution to one category.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardScore {
    pub name: String,
    pub score: f64,
}

/// One category's total, and what made it up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Category {
    pub kind: CategoryKind,
    pub score: f64,
    /// Biggest contributor first, which is also the order to show them in.
    pub cards: Vec<CardScore>,
}

/// A combo line Spellbook found in the deck.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComboLine {
    pub cards: Vec<String>,
    pub produces: Vec<String>,
    /// Starting mana with Spellbook prerequisites already satisfied, not
    /// the total cost of casting all pieces. Kept under its original JSON key.
    pub mana_value: Option<u32>,
    #[serde(default)]
    pub mana_needed: String,
    #[serde(default)]
    pub prerequisites: String,
    pub two_card: bool,
    pub early: bool,
    #[serde(default)]
    pub setup_cost_checked: bool,
    pub lock: bool,
}

impl ComboLine {
    pub fn mana_label(&self) -> String {
        match self.mana_value {
            Some(0) => "No extra mana after setup".into(),
            Some(mana) => format!("{mana} mana to start after setup"),
            None => "Starting mana unknown".into(),
        }
    }

    pub fn label(&self) -> String {
        self.cards.join(" + ")
    }
}

/// Everything worked out about one linked deck. Serialised whole into the
/// database, so the breakdown survives without the deck being re-fetched -
/// which also means it still opens at a table with no wifi.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    #[serde(default)]
    pub scoring_version: u8,
    pub deck_name: String,
    /// Moxfield's id for the list this came from, so a stored analysis can be
    /// matched back to the link that produced it.
    pub public_id: String,
    pub url: String,
    pub card_count: u32,

    /// Our own calculation: the highest floor any criterion sets.
    pub bracket: u8,
    pub criteria: Vec<Criterion>,
    /// What the deck's owner claims on Moxfield, for comparison.
    pub owner_bracket: Option<u8>,
    /// Moxfield's independent calculation, which caps at 4.
    pub auto_bracket: Option<u8>,

    pub salt_total: f64,
    pub categories: Vec<Category>,
    pub combos: Vec<ComboLine>,
    /// Cards EDHREC had no salt score for - usually too new. Listed rather
    /// than hidden, because it's the one thing that can quietly understate
    /// the total.
    pub unscored: Vec<String>,

    pub analysed_at: String,
}

impl Analysis {
    /// Older cached analyses treated free activation as free assembly.
    /// Re-evaluate their combo floor without inventing missing prerequisites.
    pub fn review_saved(mut self) -> Self {
        if self.scoring_version < 2 {
            for combo in &mut self.combos {
                combo.early = false;
                combo.setup_cost_checked = false;
            }
            for criterion in &mut self.criteria {
                if criterion.kind == CriterionKind::EarlyGameInfiniteCombos {
                    criterion.count = 0;
                    criterion.floor = 2;
                    criterion.culprits.clear();
                }
            }
            self.bracket = self.criteria.iter().map(|c| c.floor).max().unwrap_or(2);
        }
        self
    }

    /// Where this deck sits on the 0-300 salt scale, clamped for the meter.
    pub fn salt_percent(&self) -> f64 {
        (self.salt_total / SALT_CEILING * 100.0).clamp(0.0, 100.0)
    }

    /// The letter grade for the salt total, on the same cuts the pod is used
    /// to reading.
    pub fn salt_grade(&self) -> &'static str {
        let p = self.salt_percent();
        match p {
            p if p < 2.0 => "D-",
            p if p < 5.0 => "D",
            p if p < 15.0 => "D+",
            p if p < 25.0 => "C-",
            p if p < 35.0 => "C",
            p if p < 45.0 => "C+",
            p if p < 55.0 => "B-",
            p if p < 65.0 => "B",
            p if p < 75.0 => "B+",
            p if p < 85.0 => "A-",
            p if p < 95.0 => "A",
            _ => "A+",
        }
    }

    /// A word for the total, for people who don't want a number.
    pub fn salt_band(&self) -> &'static str {
        match self.salt_percent() {
            t if t < 5.0 => "Mild",
            t if t < 15.0 => "Moderate",
            t if t < 30.0 => "Salty",
            t if t < 60.0 => "High",
            _ => "Extreme",
        }
    }

    /// The criteria that set the bracket, for the one-line explanation.
    pub fn deciding(&self) -> Vec<&Criterion> {
        self.criteria
            .iter()
            .filter(|c| c.is_deciding(self.bracket))
            .collect()
    }

    /// True when the owner claims a softer deck than the list supports. The
    /// single most useful thing on the screen for a rule 0 conversation.
    pub fn understated(&self) -> bool {
        self.owner_bracket.is_some_and(|owner| owner < self.bracket)
    }

    /// Cards carrying salt in two or more categories. These are the ones
    /// worth swapping first, and the ones people argue about.
    pub fn stacked_offenders(&self) -> Vec<(String, usize, f64)> {
        let mut hits: HashMap<&str, (usize, f64)> = HashMap::new();
        for category in &self.categories {
            // A card's own EDHREC score isn't a second opinion about it, so
            // it doesn't count towards stacking.
            if category.kind == CategoryKind::Edhrec {
                continue;
            }
            for card in &category.cards {
                let entry = hits.entry(card.name.as_str()).or_insert((0, 0.0));
                entry.0 += 1;
                entry.1 += card.score;
            }
        }
        let mut stacked: Vec<(String, usize, f64)> = hits
            .into_iter()
            .filter(|(_, (count, _))| *count >= 2)
            .map(|(name, (count, score))| (name.to_string(), count, score))
            .collect();
        stacked.sort_by(|a, b| b.2.total_cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        stacked
    }
}

// ---------------------------------------------------------------------------
// Category detection from the card itself
// ---------------------------------------------------------------------------

/// The basic land types, and the colour each one makes, for spotting fetch
/// lands that reach outside the deck's identity.
const BASIC_TYPES: [(&str, char); 5] = [
    ("plains", 'W'),
    ("island", 'U'),
    ("swamp", 'B'),
    ("mountain", 'R'),
    ("forest", 'G'),
];

/// Which categories a card falls into, from its oracle text and type line.
///
/// Mass land denial and extra turns are deliberately absent: Spellbook
/// classifies those authoritatively, and a text heuristic would only
/// disagree with it.
fn categories_for(card: &moxfield::Card, identity: &str) -> Vec<CategoryKind> {
    let text = card.oracle_text.to_lowercase();
    let types = card.type_line.to_lowercase();
    let mut kinds = Vec::new();

    let mut add = |cond: bool, kind: CategoryKind| {
        if cond {
            kinds.push(kind);
        }
    };

    // "Unless that player pays" - the tax that spawned the meme, and the
    // reason people groan when a Study hits the table.
    let pay_the_one = text.contains("unless that player pays")
        || text.contains("unless they pay")
        || text.contains("unless its controller pays");
    add(pay_the_one, CategoryKind::PayTheOne);
    add(
        pay_the_one || text.contains("more to cast") || text.contains("costs more"),
        CategoryKind::Taxes,
    );

    add(
        text.contains("can't untap") || text.contains("don't untap"),
        CategoryKind::CantUntap,
    );
    add(text.contains("can't cast"), CategoryKind::CantCast);
    add(text.contains("can't activate"), CategoryKind::CantActivate);
    add(text.contains("can't attack"), CategoryKind::CantAttack);
    add(text.contains("can't draw"), CategoryKind::CantDraw);
    add(text.contains("can't search"), CategoryKind::CantSearch);
    add(text.contains("can't trigger"), CategoryKind::CantTrigger);
    add(text.contains("can't gain life"), CategoryKind::CantGainLife);

    add(
        text.contains("gain control of")
            || text.contains("from an opponent's")
            || (text.contains("you may cast") && text.contains("opponent")),
        CategoryKind::Theft,
    );

    add(
        text.contains("draws seven cards")
            || (text.contains("discards their hand") && text.contains("draw")),
        CategoryKind::Wheels,
    );
    add(
        text.contains("each opponent discards") || text.contains("target player discards"),
        CategoryKind::Discard,
    );
    add(
        text.contains("each opponent sacrifices"),
        CategoryKind::Sacrifice,
    );
    add(
        text.contains("deals damage to each opponent") || text.contains("each opponent loses"),
        CategoryKind::GroupSlug,
    );
    add(
        text.contains("destroy all creatures")
            || text.contains("exile all creatures")
            || text.contains("all creatures get -"),
        CategoryKind::Boardwipes,
    );
    add(text.contains("annihilator"), CategoryKind::Annihilator);
    add(
        text.contains("poison counter") || text.contains("infect") || text.contains("toxic"),
        CategoryKind::Poison,
    );
    add(
        text.contains("additional combat phase"),
        CategoryKind::ExtraCombats,
    );
    add(card.reserved, CategoryKind::Reserved);

    // A fetch land that can only find colours this deck isn't playing is a
    // Rhystic-adjacent tell: it means the mana base was bought, not built.
    if types.contains("land") && text.contains("search your library") {
        let reaches_off_colour = BASIC_TYPES
            .iter()
            .any(|(basic, colour)| text.contains(basic) && !identity.contains(*colour));
        add(reaches_off_colour, CategoryKind::OffColorFetches);
    }

    kinds
}

/// EDHREC's URL slug for a card name. Verified against their card pages:
/// lower-cased, apostrophes and other punctuation dropped rather than
/// hyphenated, everything else collapsed to single hyphens. A double-faced
/// card is filed under its front face.
pub fn edhrec_slug(name: &str) -> String {
    let front = name.split(" // ").next().unwrap_or(name);
    let mut slug = String::with_capacity(front.len());
    let mut pending_hyphen = false;
    for ch in front.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_hyphen && !slug.is_empty() {
                slug.push('-');
            }
            pending_hyphen = false;
            slug.push(ch.to_ascii_lowercase());
        } else if matches!(
            ch,
            '\'' | '"' | ',' | '.' | '!' | '?' | ':' | ';' | '’' | '“' | '”'
        ) {
            // Dropped outright, not turned into a separator: EDHREC files
            // "Thassa's Oracle" as thassas-oracle, not thassa-s-oracle.
        } else {
            pending_hyphen = true;
        }
    }
    slug
}

// ---------------------------------------------------------------------------
// Commander Spellbook
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct SpellbookDeck {
    commanders: Vec<SpellbookCard>,
    main: Vec<SpellbookCard>,
}

#[derive(Debug, Serialize)]
struct SpellbookCard {
    card: String,
    quantity: u32,
}

impl From<&moxfield::Card> for SpellbookCard {
    fn from(c: &moxfield::Card) -> Self {
        SpellbookCard {
            card: c.name.clone(),
            quantity: c.quantity,
        }
    }
}

#[derive(Debug, Deserialize)]
struct EstimateResponse {
    #[serde(default)]
    cards: Vec<ClassifiedCard>,
    #[serde(default)]
    combos: Vec<ClassifiedVariant>,
}

#[derive(Debug, Deserialize)]
struct ClassifiedCard {
    card: SpellbookCardName,
    #[serde(default = "one")]
    quantity: u32,
    #[serde(rename = "gameChanger", default)]
    game_changer: bool,
    #[serde(rename = "massLandDenial", default)]
    mass_land_denial: bool,
    #[serde(rename = "extraTurn", default)]
    extra_turn: bool,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Deserialize)]
struct SpellbookCardName {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ClassifiedVariant {
    combo: Variant,
    #[serde(default)]
    relevant: bool,
    #[serde(rename = "definitelyTwoCard", default)]
    definitely_two_card: bool,
    #[serde(default)]
    lock: bool,
}

#[derive(Debug, Deserialize)]
struct Variant {
    #[serde(default)]
    uses: Vec<Uses>,
    #[serde(default)]
    requires: Vec<serde_json::Value>,
    #[serde(rename = "isManaNeededAnAccurateMinimum", default)]
    accurate_minimum: bool,
    #[serde(default)]
    produces: Vec<Produces>,
    /// Leniently typed on purpose: this is the one field the early-infinite
    /// criterion depends on, and a shape change shouldn't sink the whole
    /// analysis.
    #[serde(rename = "manaValueNeeded", default)]
    mana_value_needed: Option<serde_json::Value>,
    #[serde(rename = "manaNeeded", default)]
    mana_needed: String,
    #[serde(rename = "easyPrerequisites", default)]
    easy_prerequisites: String,
    #[serde(rename = "notablePrerequisites", default)]
    notable_prerequisites: String,
}

#[derive(Debug, Deserialize)]
struct Uses {
    card: SpellbookCardName,
    #[serde(rename = "zoneLocations", default)]
    zone_locations: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Produces {
    feature: Feature,
}

#[derive(Debug, Deserialize)]
struct Feature {
    name: String,
}

fn mana_value(raw: &Option<serde_json::Value>) -> Option<u32> {
    match raw {
        Some(serde_json::Value::Number(n)) => n.as_f64().and_then(|v| {
            (v.is_finite() && v >= 0.0 && v <= u32::MAX as f64 && v.fract() == 0.0)
                .then_some(v as u32)
        }),
        Some(serde_json::Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

async fn estimate_bracket(deck: &moxfield::Deck) -> Result<EstimateResponse, Error> {
    let body = SpellbookDeck {
        commanders: deck.commanders.iter().map(SpellbookCard::from).collect(),
        main: deck.mainboard.iter().map(SpellbookCard::from).collect(),
    };

    let client = reqwest::Client::builder()
        .user_agent("commander_pod/0.1 (local Commander pod tracker)")
        .build()
        .map_err(|e| Error::Bracket(e.to_string()))?;

    let resp = client
        .post("https://backend.commanderspellbook.com/estimate-bracket")
        .json(&body)
        .send()
        .await
        .map_err(|e| Error::Bracket(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(Error::Bracket(format!("Spellbook said {}", resp.status())));
    }
    resp.json().await.map_err(|e| Error::Bracket(e.to_string()))
}

// ---------------------------------------------------------------------------
// EDHREC salt
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct EdhrecPage {
    container: EdhrecContainer,
}

#[derive(Debug, Deserialize)]
struct EdhrecContainer {
    json_dict: EdhrecDict,
}

#[derive(Debug, Deserialize)]
struct EdhrecDict {
    card: EdhrecCard,
}

#[derive(Debug, Deserialize)]
struct EdhrecCard {
    #[serde(default)]
    salt: Option<f64>,
}

/// One card's salt, from the cache if we've seen it, from EDHREC if not.
///
/// A failed request is reported as "no score" rather than an error: one
/// unreachable card shouldn't sink an analysis, and the cards it happens to
/// are listed in `Analysis::unscored` so the gap is visible. It isn't
/// cached, though - only a real answer from EDHREC is, so a network blip
/// doesn't poison the cache for a month.
async fn fetch_salt(client: &reqwest::Client, name: String) -> (String, Option<f64>) {
    let slug = edhrec_slug(&name);
    if let Some(hit) = cache::card_salt(&slug) {
        return (name, hit);
    }

    let url = format!("https://json.edhrec.com/pages/cards/{slug}.json");
    let salt = match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<EdhrecPage>().await {
            Ok(page) => {
                let salt = page.container.json_dict.card.salt;
                cache::store_card_salt(&slug, salt);
                salt
            }
            Err(_) => None,
        },
        // A 403 or 404 is EDHREC saying it has no page for this card, which
        // is a real answer and worth remembering.
        Ok(_) => {
            cache::store_card_salt(&slug, None);
            None
        }
        Err(_) => None,
    };
    (name, salt)
}

/// Salt for every distinct card in the deck, keyed by name.
async fn all_salt(deck: &moxfield::Deck) -> HashMap<String, Option<f64>> {
    let client = match reqwest::Client::builder()
        .user_agent("commander_pod/0.1 (local Commander pod tracker)")
        .build()
    {
        Ok(client) => client,
        Err(_) => return HashMap::new(),
    };

    let mut names: Vec<String> = deck.all_cards().map(|c| c.name.clone()).collect();
    names.sort();
    names.dedup();

    let mut out = HashMap::new();
    // Chunked rather than all at once: a hundred simultaneous connections
    // would be rude, and the cache means this is usually a handful anyway.
    for chunk in names.chunks(SALT_CONCURRENCY) {
        let fetches = chunk
            .iter()
            .map(|name| fetch_salt(&client, name.clone()))
            .collect::<Vec<_>>();
        for (name, salt) in iced::futures::future::join_all(fetches).await {
            out.insert(name, salt);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Putting it together
// ---------------------------------------------------------------------------

/// The deck's colour identity, as the union of its commanders'. Needed to
/// tell a fetch land that serves the deck from one that doesn't.
fn deck_identity(deck: &moxfield::Deck) -> String {
    let mut identity = String::new();
    for commander in &deck.commanders {
        for colour in commander.color_identity.chars() {
            if !identity.contains(colour) {
                identity.push(colour);
            }
        }
    }
    identity
}

/// Scores a fetched deck against a classification and a salt table. Split
/// out from `analyse` so it can be tested without a network.
fn score(
    deck: &moxfield::Deck,
    estimate: &EstimateResponse,
    salt: &HashMap<String, Option<f64>>,
) -> Analysis {
    let identity = deck_identity(deck);
    let mut buckets: HashMap<CategoryKind, Vec<CardScore>> = HashMap::new();

    let mut push = |kind: CategoryKind, name: &str, score: f64| {
        if score > 0.0 {
            buckets.entry(kind).or_default().push(CardScore {
                name: name.to_string(),
                score,
            });
        }
    };

    // Per-card terms. Everything scales with quantity, so nine Forests cost
    // nine times one Forest.
    let mut unscored = Vec::new();
    for card in deck.all_cards() {
        let quantity = card.quantity as f64;

        match salt.get(&card.name).copied().flatten() {
            Some(value) => push(CategoryKind::Edhrec, &card.name, value * quantity),
            None => unscored.push(card.name.clone()),
        }

        // Only the genuinely expensive cards, and gently: a $50 card is one
        // point of salt.
        if let Some(usd) = card.usd.filter(|usd| *usd >= PRICE_SALT_FLOOR) {
            push(
                CategoryKind::CardPrice,
                &card.name,
                usd / PRICE_SALT_DIVISOR * quantity,
            );
        }

        for kind in categories_for(card, &identity) {
            push(kind, &card.name, kind.weight() * quantity);
        }
    }

    // Spellbook's classifications, which override any local guess.
    let mut game_changers = Vec::new();
    let mut mld = Vec::new();
    let mut extra_turns = Vec::new();
    for classified in &estimate.cards {
        let name = &classified.card.name;
        let quantity = classified.quantity.max(1);
        if classified.game_changer {
            game_changers.push(name.clone());
        }
        if classified.mass_land_denial {
            mld.push(name.clone());
            push(
                CategoryKind::Mld,
                name,
                CategoryKind::Mld.weight() * quantity as f64,
            );
        }
        if classified.extra_turn {
            extra_turns.push(name.clone());
            push(
                CategoryKind::ExtraTurns,
                name,
                CategoryKind::ExtraTurns.weight() * quantity as f64,
            );
        }
    }

    // Combo lines. Only the ones Spellbook calls relevant count - the rest
    // are combos the deck could assemble with cards it isn't running.
    let mut combos = Vec::new();
    for variant in &estimate.combos {
        if !variant.relevant {
            continue;
        }
        let mana = mana_value(&variant.combo.mana_value_needed);
        let setup_cost_checked = !variant.combo.uses.is_empty()
            && variant.combo.uses.iter().all(|u| u.zone_locations == ["H"])
            && variant.combo.requires.is_empty()
            && variant.combo.notable_prerequisites.is_empty()
            && variant.combo.easy_prerequisites.is_empty()
            && variant.combo.accurate_minimum;
        let line = ComboLine {
            cards: variant
                .combo
                .uses
                .iter()
                .map(|u| u.card.name.clone())
                .collect(),
            produces: variant
                .combo
                .produces
                .iter()
                .map(|p| p.feature.name.clone())
                .collect(),
            mana_value: mana,
            mana_needed: variant.combo.mana_needed.clone(),
            prerequisites: [
                &variant.combo.easy_prerequisites,
                &variant.combo.notable_prerequisites,
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n"),
            two_card: variant.definitely_two_card,
            setup_cost_checked,
            early: variant.definitely_two_card
                && setup_cost_checked
                && mana.is_some_and(|m| m <= 7),
            lock: variant.lock,
        };

        let kind = if line.lock {
            CategoryKind::Locks
        } else {
            CategoryKind::InfiniteCombos
        };
        push(kind, &line.label(), kind.weight());
        combos.push(line);
    }

    let two_card: Vec<&ComboLine> = combos.iter().filter(|c| c.two_card).collect();
    let early: Vec<&ComboLine> = combos.iter().filter(|c| c.early).collect();

    let criteria: Vec<Criterion> = CriterionKind::ALL
        .iter()
        .map(|&kind| {
            let culprits: Vec<String> = match kind {
                CriterionKind::GameChangers => game_changers.clone(),
                CriterionKind::MassLandDenial => mld.clone(),
                CriterionKind::ExtraTurns => extra_turns.clone(),
                CriterionKind::TwoCardCombos => two_card.iter().map(|c| c.label()).collect(),
                CriterionKind::EarlyGameInfiniteCombos => early.iter().map(|c| c.label()).collect(),
            };
            let count = culprits.len() as u32;
            Criterion {
                kind,
                count,
                floor: kind.floor(count),
                culprits,
            }
        })
        .collect();

    let bracket = criteria.iter().map(|c| c.floor).max().unwrap_or(2);

    // Biggest category first, and biggest card within each.
    let mut categories: Vec<Category> = buckets
        .into_iter()
        .map(|(kind, mut cards)| {
            cards.sort_by(|a, b| {
                b.score
                    .total_cmp(&a.score)
                    .then_with(|| a.name.cmp(&b.name))
            });
            let score = cards.iter().map(|c| c.score).sum();
            Category { kind, score, cards }
        })
        .collect();
    categories.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.kind.cmp(&b.kind))
    });

    combos.sort_by(|a, b| {
        b.two_card
            .cmp(&a.two_card)
            .then_with(|| {
                a.mana_value
                    .unwrap_or(u32::MAX)
                    .cmp(&b.mana_value.unwrap_or(u32::MAX))
            })
            .then_with(|| a.label().cmp(&b.label()))
    });
    unscored.sort();
    unscored.dedup();

    Analysis {
        scoring_version: 2,
        deck_name: deck.name.clone(),
        public_id: deck.public_id.clone(),
        url: deck.url.clone(),
        card_count: deck.card_count(),
        bracket,
        criteria,
        owner_bracket: deck.owner_bracket,
        auto_bracket: deck.auto_bracket,
        salt_total: categories.iter().map(|c| c.score).sum(),
        categories,
        combos,
        unscored,
        analysed_at: chrono::Local::now().to_rfc3339(),
    }
}

/// Fetches what's needed and scores the deck.
pub async fn analyse(deck: moxfield::Deck) -> Result<Analysis, Error> {
    let estimate = estimate_bracket(&deck).await?;
    let salt = all_salt(&deck).await;
    Ok(score(&deck, &estimate, &salt))
}

/// Everything from a pasted link in one call: fetch the deck, then score it.
///
/// The two error types are flattened to a string here because the screen
/// treats them the same way - it shows the sentence and offers to try again -
/// and keeping them apart would only push the match one layer outwards.
pub async fn from_link(input: String) -> Result<Analysis, String> {
    let deck = moxfield::fetch(input).await.map_err(|e| e.to_string())?;
    analyse(deck).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(name: &str, oracle: &str, types: &str) -> moxfield::Card {
        moxfield::Card {
            name: name.to_string(),
            quantity: 1,
            scryfall_id: String::new(),
            oracle_text: oracle.to_string(),
            type_line: types.to_string(),
            color_identity: String::new(),
            usd: None,
            reserved: false,
        }
    }

    #[test]
    fn slugs_match_edhrec() {
        // All four verified against real EDHREC card pages.
        assert_eq!(edhrec_slug("Sol Ring"), "sol-ring");
        assert_eq!(edhrec_slug("Thassa's Oracle"), "thassas-oracle");
        assert_eq!(edhrec_slug("Kibo, Uktabi Prince"), "kibo-uktabi-prince");
        assert_eq!(edhrec_slug("Llanowar Elves"), "llanowar-elves");
        // A double-faced card is filed under its front face.
        assert_eq!(
            edhrec_slug("Malakir Rebirth // Malakir Mire"),
            "malakir-rebirth"
        );
    }

    #[test]
    fn bracket_floors_follow_the_guidelines() {
        use CriterionKind::*;
        // Game changers: none is core, a few is upgraded, dense is optimized.
        assert_eq!(GameChangers.floor(0), 2);
        assert_eq!(GameChangers.floor(1), 3);
        assert_eq!(GameChangers.floor(3), 3);
        assert_eq!(GameChangers.floor(4), 4);
        // Extra turns tolerate a splash but not a chain.
        assert_eq!(ExtraTurns.floor(4), 3);
        assert_eq!(ExtraTurns.floor(5), 4);
        // These two are all-or-nothing.
        assert_eq!(MassLandDenial.floor(0), 2);
        assert_eq!(MassLandDenial.floor(1), 4);
        assert_eq!(EarlyGameInfiniteCombos.floor(1), 4);
        // A clean two-card combo floors at 3, not 4 - that's what the
        // early-infinite criterion is for.
        assert_eq!(TwoCardCombos.floor(1), 3);
    }

    #[test]
    fn detects_the_cards_people_groan_at() {
        let identity = "WU";

        let study = card(
            "Rhystic Study",
            "Whenever an opponent casts a spell, you may draw a card unless that player pays {1}.",
            "Enchantment",
        );
        let kinds = categories_for(&study, identity);
        assert!(kinds.contains(&CategoryKind::PayTheOne));
        assert!(
            kinds.contains(&CategoryKind::Taxes),
            "a pay-or-else tax is also a tax"
        );

        let winter = card(
            "Winter Orb",
            "Lands don't untap during their controllers' untap steps.",
            "Artifact",
        );
        assert!(categories_for(&winter, identity).contains(&CategoryKind::CantUntap));

        let agent = card(
            "Opposition Agent",
            "You may look at and play cards from an opponent's library.",
            "Creature",
        );
        assert!(categories_for(&agent, identity).contains(&CategoryKind::Theft));

        let wrath = card(
            "Wrath of God",
            "Destroy all creatures. They can't be regenerated.",
            "Sorcery",
        );
        assert!(categories_for(&wrath, identity).contains(&CategoryKind::Boardwipes));

        // A vanilla creature is nobody's problem.
        let ape = card(
            "Kird Ape",
            "This creature gets +1/+2 as long as you control a Forest.",
            "Creature — Ape",
        );
        assert!(categories_for(&ape, identity).is_empty());
    }

    #[test]
    fn off_colour_fetches_depend_on_the_deck() {
        let fetch = card(
            "Verdant Catacombs",
            "{T}, Pay 1 life: Search your library for a Swamp or Forest card, put it onto the battlefield, then shuffle.",
            "Land",
        );
        // A Sultai deck plays both types it finds: nothing to see.
        assert!(!categories_for(&fetch, "BGU").contains(&CategoryKind::OffColorFetches));
        // In a mono-black deck the Forest half is bought mana, not built.
        assert!(categories_for(&fetch, "B").contains(&CategoryKind::OffColorFetches));
    }

    #[test]
    fn scores_a_deck_and_multiplies_by_quantity() {
        let mut forest = card("Forest", "", "Basic Land — Forest");
        forest.quantity = 9;
        forest.usd = Some(1.0);

        let mut study = card(
            "Rhystic Study",
            "you may draw a card unless that player pays {1}",
            "Enchantment",
        );
        study.usd = Some(40.0);

        let deck = moxfield::Deck {
            public_id: "abc12345".into(),
            name: "Test".into(),
            url: "https://moxfield.com/decks/abc12345".into(),
            owner_bracket: Some(2),
            auto_bracket: Some(3),
            commanders: vec![card("Kibo, Uktabi Prince", "", "Legendary Creature")],
            mainboard: vec![forest, study],
        };

        let estimate = EstimateResponse {
            cards: Vec::new(),
            combos: Vec::new(),
        };
        let mut salt = HashMap::new();
        salt.insert("Forest".to_string(), Some(0.1));
        salt.insert("Rhystic Study".to_string(), Some(2.7));
        // The commander is the card EDHREC doesn't know.
        salt.insert("Kibo, Uktabi Prince".to_string(), None);

        let a = score(&deck, &estimate, &salt);

        assert_eq!(a.card_count, 11, "nine basics plus two singles");
        assert_eq!(a.unscored, vec!["Kibo, Uktabi Prince".to_string()]);

        let by = |kind: CategoryKind| {
            a.categories
                .iter()
                .find(|c| c.kind == kind)
                .map(|c| c.score)
                .unwrap_or(0.0)
        };
        // 0.1 x 9 basics + 2.7 for the Study.
        assert!((by(CategoryKind::Edhrec) - 3.6).abs() < 1e-9);
        // The $40 Study counts, at $40/50. The $1 basics are under the floor
        // and contribute nothing however many of them there are.
        assert!((by(CategoryKind::CardPrice) - 0.8).abs() < 1e-9);
        assert!((by(CategoryKind::PayTheOne) - 2.0).abs() < 1e-9);
        assert!((by(CategoryKind::Taxes) - 3.0).abs() < 1e-9);

        // The total is the plain sum of the categories, which is what makes
        // the breakdown add up on screen.
        let summed: f64 = a.categories.iter().map(|c| c.score).sum();
        assert!((a.salt_total - summed).abs() < 1e-9);

        // Nothing in this deck trips a criterion, so it floors at 2 - and
        // the owner calling it a 2 is therefore not an understatement.
        assert_eq!(a.bracket, 2);
        assert!(!a.understated());

        // The Study earns salt three ways - it's a tax, it's a pay-the-one,
        // and it's a $40 card - so it stacks. Price counts as a category
        // here just as it does in the total; being expensive is part of why
        // people groan at a card.
        let stacked = a.stacked_offenders();
        assert_eq!(
            stacked.len(),
            1,
            "only the Study is in more than one category"
        );
        assert_eq!(stacked[0].0, "Rhystic Study");
        assert_eq!(stacked[0].1, 3, "tax, pay-the-one and price");
    }

    #[test]
    fn a_two_card_win_for_little_mana_is_bracket_four() {
        let deck = moxfield::Deck {
            public_id: "abc12345".into(),
            name: "Consultation".into(),
            url: String::new(),
            owner_bracket: Some(2),
            auto_bracket: Some(4),
            commanders: vec![card("Tymna the Weaver", "", "Legendary Creature")],
            mainboard: vec![
                card("Thassa's Oracle", "", "Creature"),
                card("Demonic Consultation", "", "Instant"),
            ],
        };

        let json = r#"{
            "cards": [],
            "combos": [{
                "combo": {
                    "uses": [{"card": {"name": "Demonic Consultation"}, "zoneLocations": ["H"]},
                             {"card": {"name": "Thassa's Oracle"}, "zoneLocations": ["H"]}],
                    "produces": [{"feature": {"name": "Win the game"}}],
                    "manaValueNeeded": 3,
                    "manaNeeded": "{U}{U}{B}",
                    "easyPrerequisites": "",
                    "notablePrerequisites": "",
                    "isManaNeededAnAccurateMinimum": true
                },
                "relevant": true,
                "definitelyTwoCard": true,
                "lock": false
            }]
        }"#;
        let estimate: EstimateResponse = serde_json::from_str(json).unwrap();

        let a = score(&deck, &estimate, &HashMap::new());

        assert_eq!(a.bracket, 4, "three mana to win off two cards is optimized");
        assert_eq!(a.combos.len(), 1);
        assert!(a.combos[0].early);
        assert_eq!(a.combos[0].mana_needed, "{U}{U}{B}");
        assert_eq!(a.combos[0].prerequisites, "");
        let saved = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Analysis>(&saved).unwrap(), a);
        // The owner says bracket 2. This is exactly the disagreement the
        // screen exists to surface.
        assert!(a.understated());

        let deciding: Vec<&str> = a.deciding().iter().map(|c| c.kind.label()).collect();
        assert_eq!(deciding, vec!["Low-Cost Combos From Hand"]);
    }

    #[test]
    fn free_activation_does_not_imply_free_setup_or_early_combo() {
        let deck = moxfield::Deck {
            public_id: "test".into(),
            name: "Setup costs".into(),
            url: String::new(),
            owner_bracket: None,
            auto_bracket: None,
            commanders: vec![card("Leader", "", "")],
            mainboard: vec![],
        };
        let estimate: EstimateResponse = serde_json::from_value(serde_json::json!({"cards":[],"combos":[{"relevant":true,"definitelyTwoCard":true,"combo":{"uses":[{"card":{"name":"Expensive piece"},"zoneLocations":["B"]},{"card":{"name":"Another piece"},"zoneLocations":["B"]}],"manaValueNeeded":0,"isManaNeededAnAccurateMinimum":true,"easyPrerequisites":"Both permanents on the battlefield.","produces":[{"feature":{"name":"Infinite life loss"}}]}}]})).unwrap();
        let result = score(&deck, &estimate, &HashMap::new());
        assert_eq!(result.bracket, 3);
        assert!(!result.combos[0].early);
        assert!(!result.combos[0].setup_cost_checked);
        assert_eq!(
            result.combos[0].prerequisites,
            "Both permanents on the battlefield."
        );
        let mut cached = result.clone();
        cached.scoring_version = 0;
        cached.bracket = 4;
        cached.combos[0].early = true;
        let criterion = cached
            .criteria
            .iter_mut()
            .find(|c| c.kind == CriterionKind::EarlyGameInfiniteCombos)
            .unwrap();
        criterion.floor = 4;
        criterion.count = 1;
        let reviewed = cached.review_saved();
        assert_eq!(reviewed.bracket, 3);
        assert_eq!(reviewed.salt_total, result.salt_total);
    }

    #[test]
    fn irrelevant_combos_are_ignored() {
        let deck = moxfield::Deck {
            public_id: "abc12345".into(),
            name: "Fair".into(),
            url: String::new(),
            owner_bracket: None,
            auto_bracket: None,
            commanders: vec![card("Kibo, Uktabi Prince", "", "Legendary Creature")],
            mainboard: vec![card("Sol Ring", "", "Artifact")],
        };
        // Spellbook returns near-misses too; a combo the deck can't actually
        // assemble must not float the bracket.
        let json = r#"{
            "cards": [],
            "combos": [{
                "combo": {
                    "uses": [{"card": {"name": "Basalt Monolith"}},
                             {"card": {"name": "Rings of Brighthearth"}}],
                    "produces": [{"feature": {"name": "Infinite colorless mana"}}],
                    "manaValueNeeded": 0
                },
                "relevant": false,
                "definitelyTwoCard": true,
                "lock": false
            }]
        }"#;
        let estimate: EstimateResponse = serde_json::from_str(json).unwrap();

        let a = score(&deck, &estimate, &HashMap::new());
        assert!(a.combos.is_empty());
        assert_eq!(a.bracket, 2);
    }

    #[test]
    fn mana_value_survives_either_json_shape() {
        assert_eq!(mana_value(&Some(serde_json::json!(4))), Some(4));
        assert_eq!(mana_value(&Some(serde_json::json!(4.0))), Some(4));
        assert_eq!(mana_value(&Some(serde_json::json!("7"))), Some(7));
        assert_eq!(mana_value(&Some(serde_json::json!(null))), None);
        assert_eq!(mana_value(&None), None);
    }

    #[test]
    fn cached_combo_costs_remain_readable_without_refresh() {
        let old = serde_json::json!({
            "cards": ["Exquisite Blood", "Enduring Tenacity"],
            "produces": ["Infinite life loss"], "mana_value": 0,
            "two_card": true, "early": true, "lock": false
        });
        let mut combo: ComboLine = serde_json::from_value(old).unwrap();
        assert_eq!(combo.mana_label(), "No extra mana after setup");
        assert!(combo.prerequisites.is_empty());
        combo.mana_value = None;
        assert_eq!(combo.mana_label(), "Starting mana unknown");
        combo.mana_value = Some(3);
        assert_eq!(combo.mana_label(), "3 mana to start after setup");
    }

    #[test]
    fn invalid_mana_is_unknown_instead_of_free() {
        for raw in [
            serde_json::json!(-1),
            serde_json::json!(2.5),
            serde_json::json!(4294967296u64),
        ] {
            assert_eq!(mana_value(&Some(raw)), None);
        }
        assert_eq!(mana_value(&Some(serde_json::json!(0))), Some(0));
    }

    /// The whole pipeline against the real Moxfield, Spellbook and EDHREC.
    ///
    /// Ignored so the normal suite stays offline and fast - these are other
    /// people's servers and shouldn't be hit on every `cargo test`. Run it
    /// deliberately after touching any of the three clients:
    ///
    /// ```text
    /// cargo test -- --ignored --nocapture live_
    /// ```
    ///
    /// The deck is a public cEDH Najeela list, picked because it trips four
    /// of the five criteria at once and so exercises the parts a casual deck
    /// never reaches.
    #[tokio::test]
    #[ignore = "hits three live APIs"]
    async fn live_analysis_of_a_real_deck() {
        let deck = crate::moxfield::fetch(
            "https://www.moxfield.com/decks/jT8Y9X4tlUmeNZ2AjkD1Vg".to_string(),
        )
        .await
        .expect("Moxfield fetch failed");

        assert!(deck.card_count() >= 99, "got {} cards", deck.card_count());
        assert!(!deck.commanders.is_empty());

        let a = analyse(deck).await.expect("analysis failed");

        println!(
            "\n{} - bracket {} (owner {:?}, moxfield {:?}), salt {:.1} {} ({})",
            a.deck_name,
            a.bracket,
            a.owner_bracket,
            a.auto_bracket,
            a.salt_total,
            a.salt_grade(),
            a.salt_band(),
        );
        for c in &a.criteria {
            println!("  {:24} {:3} -> B{}", c.kind.label(), c.count, c.floor);
        }
        println!("  combos: {}", a.combos.len());
        for combo in a.combos.iter().take(5) {
            println!(
                "    {} -> {} ({:?} mana, two_card={}, early={})",
                combo.label(),
                combo.produces.join(", "),
                combo.mana_value,
                combo.two_card,
                combo.early
            );
        }
        for cat in a.categories.iter().take(8) {
            println!("  {:22} {:7.2}", cat.kind.label(), cat.score);
        }
        println!("  unscored: {:?}", a.unscored);

        // A tournament cEDH list has to come out at the top of the scale. If
        // this ever drops, one of the three sources changed shape.
        assert_eq!(a.bracket, 4, "a cEDH list must calculate as bracket 4");
        assert!(
            a.salt_total > 60.0,
            "cEDH should be off the top of the salt scale, got {}",
            a.salt_total
        );
        assert!(!a.combos.is_empty(), "Spellbook must find its combo lines");
        assert!(
            a.categories.iter().any(|c| c.kind == CategoryKind::Edhrec),
            "EDHREC salt must have come back for at least some cards"
        );
        // The sum has to stay the sum, or the breakdown won't add up on
        // screen.
        let summed: f64 = a.categories.iter().map(|c| c.score).sum();
        assert!((a.salt_total - summed).abs() < 1e-9);
    }

    #[test]
    fn grades_sit_on_the_expected_cuts() {
        let mut a = score(
            &moxfield::Deck {
                public_id: "abc12345".into(),
                name: String::new(),
                url: String::new(),
                owner_bracket: None,
                auto_bracket: None,
                commanders: vec![card("X", "", "")],
                mainboard: Vec::new(),
            },
            &EstimateResponse {
                cards: Vec::new(),
                combos: Vec::new(),
            },
            &HashMap::new(),
        );

        a.salt_total = 0.0;
        assert_eq!(a.salt_grade(), "D-");
        assert_eq!(a.salt_band(), "Mild");

        // A typical casual deck: the ~29 I measured on a real list.
        a.salt_total = 29.07;
        assert_eq!(a.salt_band(), "Moderate");

        // A cEDH list around 198 sits near the top of the scale.
        a.salt_total = 198.17;
        assert_eq!(a.salt_grade(), "B+");
        assert_eq!(a.salt_band(), "Extreme");

        for (score, band) in [
            (14.99, "Mild"),
            (15.0, "Moderate"),
            (44.99, "Moderate"),
            (45.0, "Salty"),
            (75.44, "Salty"),
            (89.99, "Salty"),
            (90.0, "High"),
            (179.99, "High"),
            (180.0, "Extreme"),
        ] {
            a.salt_total = score;
            assert_eq!(a.salt_band(), band, "score {score}");
        }

        // Nothing runs off the end of the meter.
        a.salt_total = 5_000.0;
        assert_eq!(a.salt_percent(), 100.0);
        assert_eq!(a.salt_grade(), "A+");
    }
}
