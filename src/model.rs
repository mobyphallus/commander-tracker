use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub id: i64,
    pub name: String,
}

/// Which part of the art stays visible when it's cropped to a seat tile.
/// How a commander's art is framed inside one seat's tile: how far it's
/// zoomed past "just covers the tile", and where it's panned to.
///
/// Pan is normalised to [-1, 1] on each axis, where +/-1 is as far as the
/// art can move before an edge would show. Storing the fraction rather than
/// pixels means the same framing holds at any tile size, which matters
/// because the same art is drawn into tiles of very different shapes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArtFraming {
    pub zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
}

impl Default for ArtFraming {
    fn default() -> Self {
        Self {
            zoom: MIN_ART_ZOOM,
            pan_x: 0.0,
            pan_y: 0.0,
        }
    }
}

impl ArtFraming {
    pub fn clamped(self) -> Self {
        Self {
            zoom: self.zoom.clamp(MIN_ART_ZOOM, MAX_ART_ZOOM),
            pan_x: self.pan_x.clamp(-1.0, 1.0),
            pan_y: self.pan_y.clamp(-1.0, 1.0),
        }
    }
}



pub const MIN_ART_ZOOM: f32 = 1.0;
pub const MAX_ART_ZOOM: f32 = 3.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Commander {
    pub id: i64,
    /// Scryfall's oracle id: stable across every printing/art of this card.
    /// This, not any single printing, is the commander's identity for stats.
    pub oracle_id: String,
    pub name: String,
    /// Full card image of the currently chosen printing, used as a fallback portrait.
    pub image_url: Option<String>,
    /// Cropped art of the currently chosen printing, used for the player's portrait.
    pub art_crop_url: Option<String>,
    pub color_identity: String,
    /// How this art sits in the tile it's currently being shown in.
    /// Resolved per layout and seat when the commander is placed.
    pub framing: ArtFraming,
}

impl Commander {
    pub fn portrait_url(&self) -> Option<&str> {
        self.art_crop_url.as_deref().or(self.image_url.as_deref())
    }
}

/// One entry in a player's saved commander list: a single commander, or a
/// partner pair they've saved as one deck. Picking either half of a saved
/// pair during setup brings the other with it.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedDeck {
    pub commander: Commander,
    pub partner: Option<Commander>,
}

impl SavedDeck {
    pub fn label(&self) -> String {
        match &self.partner {
            Some(p) => format!("{} + {}", self.commander.name, p.name),
            None => self.commander.name.clone(),
        }
    }
}

/// Lethal commander damage from a single source, per the rules. With a
/// partner pair each commander carries its own threshold, so 20 from one
/// and 20 from the other is survivable.
pub const LETHAL_COMMANDER_DAMAGE: i32 = 21;

/// Which of a seat's commanders something refers to. A seat always has a
/// primary; a partner pair adds the second.
pub const PRIMARY: usize = 0;
pub const PARTNER: usize = 1;
pub const LETHAL_POISON: i32 = 10;

/// One seat at the table for the game currently being set up / played.
#[derive(Debug, Clone)]
pub struct Seat {
    pub player: Player,
    pub commander: Commander,
    /// The second commander of a partner pair, if this deck runs one.
    pub partner: Option<Commander>,
    pub life: i32,
    pub poison: i32,
    /// Commander damage taken by this seat, keyed by which commander dealt
    /// it: `(source seat index, PRIMARY | PARTNER)`. Keyed per commander
    /// rather than per seat because each one has its own lethal threshold.
    pub commander_damage_taken: HashMap<(usize, usize), i32>,
    pub eliminated: bool,
    /// How this seat went out, recorded the moment it happens. Who got the
    /// kill and which turn it was are both impossible to reconstruct at the
    /// end of the game, so they are never inferred later.
    pub elimination: Option<Elimination>,
}

impl Seat {
    pub fn new(player: Player, commander: Commander, starting_life: i32) -> Self {
        Self {
            player,
            commander,
            partner: None,
            life: starting_life,
            poison: 0,
            commander_damage_taken: HashMap::new(),
            eliminated: false,
            elimination: None,
        }
    }

    /// Put a seat out, with the record of why. Always use this rather than
    /// setting `eliminated`: the flag and the record must not drift apart.
    pub fn mark_out(&mut self, elimination: Elimination) {
        self.eliminated = true;
        self.elimination = Some(elimination);
    }

    /// Undo a call. The old record goes with it - a seat that is back in the
    /// game did not die on the turn we thought it did.
    pub fn bring_back(&mut self) {
        self.eliminated = false;
        self.elimination = None;
    }

    pub fn with_partner(mut self, partner: Option<Commander>) -> Self {
        self.partner = partner;
        self
    }

    pub fn commander_in(&self, slot: usize) -> &Commander {
        match slot {
            PARTNER => self.partner.as_ref().unwrap_or(&self.commander),
            _ => &self.commander,
        }
    }

    /// Both commanders' names, as the deck is usually referred to.
    pub fn deck_name(&self) -> String {
        match &self.partner {
            Some(p) => format!("{} + {}", self.commander.name, p.name),
            None => self.commander.name.clone(),
        }
    }

    pub fn damage_from(&self, seat_index: usize, slot: usize) -> i32 {
        *self
            .commander_damage_taken
            .get(&(seat_index, slot))
            .unwrap_or(&0)
    }

}

pub const STARTING_LIFE: i32 = 40;

/// How the winner actually closed out the game, for matchup/stat breakdowns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinReason {
    CommanderDamage,
    Poison,
    CombatDamage,
    InfiniteCombo,
    Concede,
    Other,
}

impl WinReason {
    pub const ALL: [WinReason; 6] = [
        Self::CommanderDamage,
        Self::Poison,
        Self::CombatDamage,
        Self::InfiniteCombo,
        Self::Concede,
        Self::Other,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::CommanderDamage => "Commander Damage",
            Self::Poison => "Poison",
            Self::CombatDamage => "Combat Damage",
            Self::InfiniteCombo => "Infinite Combo",
            Self::Concede => "Concede",
            Self::Other => "Other",
        }
    }

    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::CommanderDamage => "commander_damage",
            Self::Poison => "poison",
            Self::CombatDamage => "combat_damage",
            Self::InfiniteCombo => "infinite_combo",
            Self::Concede => "concede",
            Self::Other => "other",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s {
            "commander_damage" => Self::CommanderDamage,
            "poison" => Self::Poison,
            "combat_damage" => Self::CombatDamage,
            "infinite_combo" => Self::InfiniteCombo,
            "concede" => Self::Concede,
            _ => Self::Other,
        }
    }
}

/// Why a seat went out. The first three are worked out from the board -
/// nobody is asked - and the last two are what a player picks when they are
/// marked out by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutCause {
    LifeLoss,
    CommanderDamage,
    Poison,
    Concede,
    Other,
}

impl OutCause {
    /// The causes a player can actually choose. Dying to life loss, poison
    /// or commander damage is observed, not declared.
    pub const CHOOSABLE: [OutCause; 2] = [Self::Concede, Self::Other];

    pub fn label(&self) -> &'static str {
        match self {
            Self::LifeLoss => "Life Loss",
            Self::CommanderDamage => "Commander Damage",
            Self::Poison => "Poison",
            Self::Concede => "Conceded",
            Self::Other => "Other",
        }
    }

    /// Past-tense phrasing for history lines.
    pub fn past_tense(&self) -> &'static str {
        match self {
            Self::LifeLoss => "died",
            Self::CommanderDamage => "died to commander damage",
            Self::Poison => "died to poison",
            Self::Concede => "conceded",
            Self::Other => "went out",
        }
    }

    /// Whether this cause leaves any doubt about who is responsible. Lethal
    /// commander damage names its own killer; a concede has none by
    /// definition. Everything else has to be asked.
    pub fn needs_killer_prompt(&self) -> bool {
        matches!(self, Self::LifeLoss | Self::Poison | Self::Other)
    }

    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::LifeLoss => "life_loss",
            Self::CommanderDamage => "commander_damage",
            Self::Poison => "poison",
            Self::Concede => "concede",
            Self::Other => "other",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s {
            "life_loss" => Self::LifeLoss,
            "commander_damage" => Self::CommanderDamage,
            "poison" => Self::Poison,
            "concede" => Self::Concede,
            _ => Self::Other,
        }
    }
}

/// How and when a seat went out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elimination {
    pub cause: OutCause,
    /// Who gets the kill. `None` is a real answer, not missing data - a
    /// concede, a board state, or simply nobody in particular.
    pub killer_seat: Option<usize>,
    /// The 1-indexed turn number the seat went out on.
    pub turn: u32,
}

/// The kind of interaction logged against a player mid-game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HateKind {
    CommanderKill,
    BoardWipe,
    Counterspell,
}

impl HateKind {
    pub const ALL: [HateKind; 3] = [
        Self::CommanderKill,
        Self::BoardWipe,
        Self::Counterspell,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::CommanderKill => "Commander Kill",
            Self::BoardWipe => "Board Wipe",
            Self::Counterspell => "Counterspell",
        }
    }

    /// Past-tense phrasing for history lines.
    pub fn past_tense(&self) -> &'static str {
        match self {
            Self::CommanderKill => "had their commander killed by",
            Self::BoardWipe => "got board wiped by",
            Self::Counterspell => "got countered by",
        }
    }

    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::CommanderKill => "commander_kill",
            Self::BoardWipe => "board_wipe",
            Self::Counterspell => "counterspell",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s {
            "board_wipe" => Self::BoardWipe,
            "counterspell" => Self::Counterspell,
            _ => Self::CommanderKill,
        }
    }
}

/// A piece of commander hate aimed at a seat, and who (if anyone) is credited.
#[derive(Debug, Clone, Copy)]
pub struct KillEvent {
    pub victim_seat: usize,
    pub killer_seat: Option<usize>,
    pub kind: HateKind,
}

/// A fully finished game, ready to be persisted.
#[derive(Debug, Clone)]
pub struct FinishedGame {
    pub seats: Vec<Seat>,
    pub winner_seat: Option<usize>,
    pub win_reason: Option<WinReason>,
    /// The 1-indexed turn count when the game ended.
    pub ending_turn: u32,
    pub kills: Vec<KillEvent>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct MatchupStat {
    pub commander_a: String,
    pub commander_b: String,
    pub a_wins: i64,
    pub b_wins: i64,
    pub games: i64,
}

#[derive(Debug, Clone)]
pub struct PlayerStat {
    pub player_name: String,
    pub games: i64,
    pub wins: i64,
}

/// How much commander hate a player has dished out, and of what kind.
#[derive(Debug, Clone)]
pub struct HaterStat {
    pub player_name: String,
    pub total: i64,
    pub kills: i64,
    pub wipes: i64,
    pub counters: i64,
    /// Games they've played, so "hate per game" is comparable across people
    /// who've sat down different numbers of times.
    pub games: i64,
}

impl HaterStat {
    pub fn per_game(&self) -> f64 {
        if self.games == 0 {
            0.0
        } else {
            self.total as f64 / self.games as f64
        }
    }
}

/// How often a commander is on the receiving end.
#[derive(Debug, Clone)]
pub struct HatedCommanderStat {
    pub commander_name: String,
    pub total: i64,
    pub kills: i64,
    pub wipes: i64,
    pub counters: i64,
    /// Times this commander has been at the table, for a "hate per game" rate.
    pub appearances: i64,
}

impl HatedCommanderStat {
    pub fn per_appearance(&self) -> f64 {
        if self.appearances == 0 {
            0.0
        } else {
            self.total as f64 / self.appearances as f64
        }
    }
}

/// A specific grudge: this player keeps going after this commander.
#[derive(Debug, Clone)]
pub struct GrudgeStat {
    pub hater_name: String,
    pub commander_name: String,
    pub victim_name: String,
    pub total: i64,
}

#[derive(Debug, Clone)]
pub struct WinReasonStat {
    pub reason: WinReason,
    pub games: i64,
}

/// One row in the game history list.
#[derive(Debug, Clone)]
pub struct GameSummary {
    pub id: i64,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: chrono::DateTime<chrono::Utc>,
    pub pod_size: i64,
    pub winner_name: Option<String>,
    pub winner_commander: Option<String>,
    pub win_reason: Option<WinReason>,
    pub ending_turn: i64,
}

/// A seat's elimination as it comes back out of the database, with the
/// killer resolved to a name.
#[derive(Debug, Clone)]
pub struct GameDetailOut {
    pub cause: OutCause,
    pub killer_name: Option<String>,
    pub turn: u32,
}

#[derive(Debug, Clone)]
pub struct GameDetailSeat {
    pub player_name: String,
    pub commander_name: String,
    pub final_life: i32,
    pub final_poison: i32,
    pub won: bool,
    /// (source commander name, amount) for commander damage taken this game.
    pub damage_taken: Vec<(String, i32)>,
    /// How and when they went out, if they did. The winner never has one.
    pub out: Option<GameDetailOut>,
}

#[derive(Debug, Clone)]
pub struct GameDetailKill {
    pub victim: String,
    pub killer: Option<String>,
    pub kind: HateKind,
}

/// The full box score for a single past game.
#[derive(Debug, Clone)]
pub struct GameDetail {
    pub id: i64,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: chrono::DateTime<chrono::Utc>,
    pub win_reason: Option<WinReason>,
    pub ending_turn: i64,
    pub seats: Vec<GameDetailSeat>,
    pub kills: Vec<GameDetailKill>,
}
