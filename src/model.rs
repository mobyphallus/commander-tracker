use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Commander {
    pub id: i64,
    pub scryfall_id: String,
    pub name: String,
    /// Full card image, used for search result previews.
    pub image_url: Option<String>,
    /// Cropped art, used for the player's portrait in the pod.
    pub art_crop_url: Option<String>,
    pub color_identity: String,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartingLife {
    Forty,
}

impl StartingLife {
    pub const VALUE: i32 = 40;
}

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

/// A fully finished game, ready to be persisted.
#[derive(Debug, Clone)]
pub struct FinishedGame {
    pub seats: Vec<Seat>,
    pub winner_seat: Option<usize>,
    pub win_reason: Option<WinReason>,
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

/// The full box score for a single past game.
#[derive(Debug, Clone)]
pub struct GameDetail {
    pub id: i64,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: chrono::DateTime<chrono::Utc>,
    pub win_reason: Option<WinReason>,
    pub seats: Vec<GameDetailSeat>,
}
