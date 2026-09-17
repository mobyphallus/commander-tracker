use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub id: i64,
    pub name: String,
}

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
}

impl Commander {
    pub fn portrait_url(&self) -> Option<&str> {
        self.art_crop_url.as_deref().or(self.image_url.as_deref())
    }
}

/// Lethal commander damage from a single source, per the rules.
pub const LETHAL_COMMANDER_DAMAGE: i32 = 21;
pub const LETHAL_POISON: i32 = 10;

/// One seat at the table for the game currently being set up / played.
#[derive(Debug, Clone)]
pub struct Seat {
    pub player: Player,
    pub commander: Commander,
    pub life: i32,
    pub poison: i32,
    /// Commander damage taken by this seat, keyed by the seat index it came from.
    pub commander_damage_taken: HashMap<usize, i32>,
    pub eliminated: bool,
}

impl Seat {
    pub fn new(player: Player, commander: Commander, starting_life: i32) -> Self {
        Self {
            player,
            commander,
            life: starting_life,
            poison: 0,
            commander_damage_taken: HashMap::new(),
            eliminated: false,
        }
    }

    pub fn damage_from(&self, seat_index: usize) -> i32 {
        *self.commander_damage_taken.get(&seat_index).unwrap_or(&0)
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

/// A commander being killed mid-game, and who (if anyone) is credited.
#[derive(Debug, Clone, Copy)]
pub struct KillEvent {
    pub victim_seat: usize,
    pub killer_seat: Option<usize>,
}

/// A fully finished game, ready to be persisted.
#[derive(Debug, Clone)]
pub struct FinishedGame {
    pub seats: Vec<Seat>,
    pub winner_seat: Option<usize>,
    pub win_reason: Option<WinReason>,
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
}

#[derive(Debug, Clone)]
pub struct GameDetailKill {
    pub victim: String,
    pub killer: Option<String>,
}

/// The full box score for a single past game.
#[derive(Debug, Clone)]
pub struct GameDetail {
    pub id: i64,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: chrono::DateTime<chrono::Utc>,
    pub win_reason: Option<WinReason>,
    pub seats: Vec<GameDetailSeat>,
    pub kills: Vec<GameDetailKill>,
}
