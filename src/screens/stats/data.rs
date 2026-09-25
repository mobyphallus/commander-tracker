//! Read-only multiplayer statistics. A seat is an appearance; a shared game
//! counts once per rivalry, regardless of duplicate commander decks at a table.
use crate::model::{HateKind, WinReason};
use rusqlite::Connection;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Players,
    Commanders,
    Pilots,
}
impl Scope {
    pub const ALL: [Self; 3] = [Self::Players, Self::Commanders, Self::Pilots];
    pub fn label(self) -> &'static str {
        match self {
            Self::Players => "Players",
            Self::Commanders => "Commanders",
            Self::Pilots => "Pilot + commander",
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::Players => 0,
            Self::Commanders => 1,
            Self::Pilots => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntityKey {
    pub player: Option<i64>,
    pub commanders: Vec<i64>,
}
#[derive(Debug, Clone)]
pub struct Identity {
    pub key: EntityKey,
    pub name: String,
    pub detail: String,
}
#[derive(Debug, Clone, Default)]
pub struct Counts {
    pub kills: u32,
    pub wipes: u32,
    pub counters: u32,
}
impl Counts {
    pub fn total(&self) -> u32 {
        self.kills + self.wipes + self.counters
    }
    fn add(&mut self, kind: HateKind) {
        match kind {
            HateKind::CommanderKill => self.kills += 1,
            HateKind::BoardWipe => self.wipes += 1,
            HateKind::Counterspell => self.counters += 1,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Record {
    pub identity: Identity,
    pub appearances: u32,
    pub wins: u32,
    pub losses: u32,
    pub unresolved: u32,
    pub expected_wins: f64,
    pub given: Counts,
    pub received: Counts,
}
impl Record {
    pub fn decided(&self) -> u32 {
        self.wins + self.losses
    }
    pub fn rate(&self) -> f64 {
        ratio(self.wins, self.decided())
    }
}
#[derive(Debug, Clone)]
pub struct Rivalry {
    pub a: Identity,
    pub b: Identity,
    pub shared: u32,
    pub a_wins: u32,
    pub b_wins: u32,
    pub others: u32,
    pub unresolved: u32,
}
impl Rivalry {
    pub fn decided(&self) -> u32 {
        self.a_wins + self.b_wins
    }
    pub fn margin(&self) -> u32 {
        self.a_wins.abs_diff(self.b_wins)
    }
    pub fn close_rivalry(&self) -> bool {
        self.shared >= 3
            && self.a_wins > 0
            && self.b_wins > 0
            && self.margin() as f64 / self.decided() as f64 <= 0.34
    }
    pub fn dominance(&self) -> bool {
        self.shared >= 3
            && self.a_wins.max(self.b_wins) as f64 / self.shared as f64 >= 0.5
            && self.a_wins.max(self.b_wins) >= 3
            && self.a_wins.max(self.b_wins) as f64 / self.decided().max(1) as f64 >= 0.75
    }
}
#[derive(Debug, Clone)]
pub struct Grudge {
    pub source: Identity,
    pub target: Identity,
    pub events: u32,
    pub shared: u32,
}
#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub records: Vec<Record>,
    pub rivalries: Vec<Rivalry>,
    pub grudges: Vec<Grudge>,
}
#[derive(Debug, Clone)]
pub struct Appearance {
    pub id: i64,
    pub game: i64,
    pub player: i64,
    pub player_name: String,
    pub commander: i64,
    pub commander_name: String,
    pub partner: Option<i64>,
    pub partner_name: Option<String>,
    pub won: bool,
    pub owner_name: Option<String>,
}
impl Appearance {
    pub fn identity(&self, scope: Scope) -> Identity {
        let mut commanders = vec![(self.commander, self.commander_name.clone())];
        if let Some(id) = self.partner {
            if id != self.commander {
                commanders.push((
                    id,
                    self.partner_name
                        .clone()
                        .unwrap_or_else(|| "Unknown partner".into()),
                ));
            }
        }
        commanders.sort_by_key(|c| c.0);
        let deck = commanders
            .iter()
            .map(|c| c.1.as_str())
            .collect::<Vec<_>>()
            .join(" + ");
        Identity {
            key: EntityKey {
                player: if scope == Scope::Commanders {
                    None
                } else {
                    Some(self.player)
                },
                commanders: if scope == Scope::Players {
                    vec![]
                } else {
                    commanders.iter().map(|c| c.0).collect()
                },
            },
            name: if scope == Scope::Commanders {
                deck.clone()
            } else {
                self.player_name.clone()
            },
            detail: if scope == Scope::Pilots {
                deck
            } else {
                String::new()
            },
        }
    }
}
#[derive(Debug, Clone)]
pub struct Game {
    pub id: i64,
    pub date: String,
    pub reason: Option<String>,
    pub turns: i64,
    pub minutes: f64,
}
#[derive(Debug, Clone)]
pub struct Event {
    pub game: i64,
    pub source: Option<i64>,
    pub target: i64,
    pub kind: HateKind,
}
#[derive(Debug, Clone, Default)]
pub struct Data {
    pub games: Vec<Game>,
    pub seats: Vec<Appearance>,
    pub events: Vec<Event>,
    pub summaries: [Summary; 3],
}
pub fn ratio(n: u32, d: u32) -> f64 {
    if d == 0 {
        0.0
    } else {
        n as f64 / d as f64
    }
}
impl Data {
    pub fn load(conn: &Connection) -> rusqlite::Result<Self> {
        let games = conn.prepare("SELECT id, started_at, win_reason, ending_turn, MAX(0, (julianday(ended_at)-julianday(started_at))*1440) FROM games ORDER BY started_at DESC, id DESC")?.query_map([], |r| Ok(Game { id: r.get(0)?, date: r.get(1)?, reason: r.get(2)?, turns: r.get(3)?, minutes: r.get::<_, Option<f64>>(4)?.unwrap_or(0.0) }))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let seats = conn.prepare("SELECT gp.id, gp.game_id, p.id, p.name, c.id, c.name, pc.id, pc.name, gp.won, owner.name FROM game_players gp JOIN players p ON p.id=gp.player_id JOIN commanders c ON c.id=gp.commander_id LEFT JOIN commanders pc ON pc.id=gp.partner_commander_id LEFT JOIN players owner ON owner.id=gp.owner_player_id ORDER BY gp.game_id, gp.seat")?.query_map([], |r| Ok(Appearance { id:r.get(0)?, game:r.get(1)?, player:r.get(2)?, player_name:r.get(3)?, commander:r.get(4)?, commander_name:r.get(5)?, partner:r.get(6)?, partner_name:r.get(7)?, won:r.get::<_, i64>(8)? != 0, owner_name:r.get(9)? }))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let events = conn.prepare("SELECT game_id, killer_game_player_id, victim_game_player_id, kind FROM commander_kills ORDER BY game_id, id")?.query_map([], |r| Ok(Event { game:r.get(0)?, source:r.get(1)?, target:r.get(2)?, kind:HateKind::from_db_str(&r.get::<_, String>(3)?) }))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Self::from_rows(games, seats, events))
    }
    pub fn from_rows(games: Vec<Game>, seats: Vec<Appearance>, events: Vec<Event>) -> Self {
        let summaries = Scope::ALL.map(|scope| aggregate(scope, &seats, &events));
        Self {
            games,
            seats,
            events,
            summaries,
        }
    }
    pub fn summary(&self, scope: Scope) -> &Summary {
        &self.summaries[scope.index()]
    }
    pub fn resolved_games(&self) -> usize {
        self.seats
            .iter()
            .filter(|s| s.won)
            .map(|s| s.game)
            .collect::<BTreeSet<_>>()
            .len()
    }
    pub fn endings(&self) -> Vec<(String, u32)> {
        let mut counts = BTreeMap::<String, u32>::new();
        for game in &self.games {
            let label = if !self.seats.iter().any(|s| s.game == game.id && s.won) {
                "No recorded winner".into()
            } else {
                game.reason
                    .as_ref()
                    .map(|r| WinReason::from_db_str(r).label().to_string())
                    .unwrap_or_else(|| "Unspecified ending".into())
            };
            *counts.entry(label).or_default() += 1;
        }
        let mut rows: Vec<_> = counts.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        rows
    }
}

fn aggregate(scope: Scope, seats: &[Appearance], events: &[Event]) -> Summary {
    let mut games = BTreeMap::<i64, Vec<&Appearance>>::new();
    for seat in seats {
        games.entry(seat.game).or_default().push(seat);
    }
    let mut records = BTreeMap::<EntityKey, Record>::new();
    let mut rivalries = BTreeMap::<(EntityKey, EntityKey), Rivalry>::new();
    for participants in games.values() {
        let winners = participants.iter().filter(|s| s.won).count();
        let mut entities = BTreeMap::<EntityKey, (Identity, bool)>::new();
        for seat in participants {
            let identity = seat.identity(scope);
            let record = records
                .entry(identity.key.clone())
                .or_insert_with(|| Record {
                    identity: identity.clone(),
                    appearances: 0,
                    wins: 0,
                    losses: 0,
                    unresolved: 0,
                    expected_wins: 0.0,
                    given: Counts::default(),
                    received: Counts::default(),
                });
            record.appearances += 1;
            if seat.won {
                record.wins += 1;
            } else if winners > 0 {
                record.losses += 1;
            } else {
                record.unresolved += 1;
            }
            if winners > 0 {
                record.expected_wins += winners as f64 / participants.len() as f64;
            }
            entities
                .entry(identity.key.clone())
                .and_modify(|e| e.1 |= seat.won)
                .or_insert((identity, seat.won));
        }
        let entities: Vec<_> = entities.into_values().collect();
        for (i, (a, a_won)) in entities.iter().enumerate() {
            for (b, b_won) in &entities[i + 1..] {
                let r = rivalries
                    .entry((a.key.clone(), b.key.clone()))
                    .or_insert_with(|| Rivalry {
                        a: a.clone(),
                        b: b.clone(),
                        shared: 0,
                        a_wins: 0,
                        b_wins: 0,
                        others: 0,
                        unresolved: 0,
                    });
                r.shared += 1;
                match (*a_won, *b_won) {
                    (true, false) => r.a_wins += 1,
                    (false, true) => r.b_wins += 1,
                    (false, false) if winners > 0 => r.others += 1,
                    _ => r.unresolved += 1, // no winner, or both entities share the win
                }
            }
        }
    }
    let by_id: BTreeMap<_, _> = seats.iter().map(|s| (s.id, s)).collect();
    let mut grudges = BTreeMap::<(EntityKey, EntityKey), Grudge>::new();
    for event in events {
        let Some(target) = by_id.get(&event.target).filter(|s| s.game == event.game) else {
            continue;
        };
        let target = target.identity(scope);
        if let Some(r) = records.get_mut(&target.key) {
            r.received.add(event.kind);
        }
        let Some(source) = event
            .source
            .and_then(|id| by_id.get(&id))
            .filter(|s| s.game == event.game)
        else {
            continue;
        };
        let source = source.identity(scope);
        if let Some(r) = records.get_mut(&source.key) {
            r.given.add(event.kind);
        }
        let key = (source.key.clone(), target.key.clone());
        let pair = if source.key < target.key {
            key.clone()
        } else {
            (key.1.clone(), key.0.clone())
        };
        // Mirror decks can target one another; their exposure is the number
        // of games with at least two seats using that commander configuration.
        let shared = if source.key == target.key {
            games
                .values()
                .filter(|ss| {
                    ss.iter()
                        .filter(|s| s.identity(scope).key == source.key)
                        .count()
                        >= 2
                })
                .count() as u32
        } else {
            rivalries.get(&pair).map(|r| r.shared).unwrap_or(0)
        };
        grudges
            .entry(key)
            .or_insert_with(|| Grudge {
                source,
                target,
                events: 0,
                shared,
            })
            .events += 1;
    }
    let mut records: Vec<_> = records.into_values().collect();
    records.sort_by(|a, b| {
        b.wins
            .cmp(&a.wins)
            .then(b.appearances.cmp(&a.appearances))
            .then(a.identity.key.cmp(&b.identity.key))
    });
    let mut rivalries: Vec<_> = rivalries.into_values().collect();
    rivalries.sort_by(|a, b| {
        b.shared
            .cmp(&a.shared)
            .then(a.a.key.cmp(&b.a.key))
            .then(a.b.key.cmp(&b.b.key))
    });
    let mut grudges: Vec<_> = grudges.into_values().collect();
    grudges.sort_by(|a, b| {
        b.events
            .cmp(&a.events)
            .then(a.source.key.cmp(&b.source.key))
            .then(a.target.key.cmp(&b.target.key))
    });
    Summary {
        records,
        rivalries,
        grudges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seat(id: i64, game: i64, player: i64, commander: i64, won: bool) -> Appearance {
        Appearance {
            id,
            game,
            player,
            player_name: format!("Player {player}"),
            commander,
            commander_name: format!("Commander {commander}"),
            partner: None,
            partner_name: None,
            won,
            owner_name: None,
        }
    }
    fn sample() -> Data {
        let specifications = [
            vec![(1, 10, true), (2, 20, false), (3, 30, false)],
            vec![(2, 20, true), (3, 30, false), (1, 10, false)],
            vec![(3, 30, true), (1, 10, false), (2, 20, false)],
            vec![(4, 40, true), (2, 20, false)],
            vec![(1, 10, false), (2, 20, false)],
            vec![(1, 10, true), (2, 10, false), (3, 30, false)],
        ];
        let mut seats = Vec::new();
        let mut games = Vec::new();
        for (i, players) in specifications.iter().enumerate() {
            let id = i as i64 + 1;
            games.push(Game {
                id,
                date: format!("2026-09-{id:02}"),
                reason: None,
                turns: 8,
                minutes: 60.0,
            });
            for &(p, c, w) in players {
                seats.push(seat(seats.len() as i64 + 1, id, p, c, w));
            }
        }
        Data::from_rows(games, seats, vec![])
    }
    #[test]
    fn attendance_and_pilots_have_separate_denominators() {
        let d = sample();
        let players = &d.summary(Scope::Players).records;
        let a = players
            .iter()
            .find(|r| r.identity.key.player == Some(1))
            .unwrap();
        assert_eq!(
            (a.appearances, a.wins, a.losses, a.unresolved),
            (5, 2, 2, 1)
        );
        assert_eq!(a.rate(), 0.5);
        let b = players
            .iter()
            .find(|r| r.identity.key.player == Some(2))
            .unwrap();
        assert_eq!(
            (b.appearances, b.wins, b.losses, b.unresolved),
            (6, 1, 4, 1)
        );
        let pilots = &d.summary(Scope::Pilots).records;
        let b10 = pilots
            .iter()
            .find(|r| r.identity.key.player == Some(2) && r.identity.key.commanders == vec![10])
            .unwrap();
        assert_eq!((b10.appearances, b10.wins, b10.losses), (1, 0, 1));
        assert_eq!(d.resolved_games(), 5);
    }
    #[test]
    fn rivalries_ignore_seat_order_and_separate_third_party_winners() {
        let d = sample();
        let rivals = &d.summary(Scope::Players).rivalries;
        let pair: Vec<_> = rivals
            .iter()
            .filter(|r| r.a.key.player == Some(1) && r.b.key.player == Some(2))
            .collect();
        assert_eq!(pair.len(), 1);
        let r = pair[0];
        assert_eq!(
            (r.shared, r.a_wins, r.b_wins, r.others, r.unresolved),
            (5, 2, 1, 1, 1)
        );
        assert!(r.close_rivalry());
        assert!(!r.dominance());
    }
    #[test]
    fn mirror_commanders_count_appearances_but_not_duplicate_shared_games() {
        let d = sample();
        let summary = d.summary(Scope::Commanders);
        let c = summary
            .records
            .iter()
            .find(|r| r.identity.key.commanders == vec![10])
            .unwrap();
        assert_eq!(
            (c.appearances, c.wins, c.losses, c.unresolved),
            (6, 2, 3, 1)
        );
        let r = summary
            .rivalries
            .iter()
            .find(|r| r.a.key.commanders == vec![10] && r.b.key.commanders == vec![30])
            .unwrap();
        assert_eq!((r.shared, r.a_wins, r.b_wins, r.others), (4, 2, 1, 1));
        assert!(summary.rivalries.iter().all(|r| r.a.key != r.b.key));
    }
    #[test]
    fn partner_order_and_borrowing_do_not_split_the_same_pilot_record() {
        let mut a = seat(1, 1, 1, 10, true);
        a.partner = Some(11);
        a.partner_name = Some("Partner".into());
        a.owner_name = Some("Someone else".into());
        let mut b = seat(2, 2, 1, 11, true);
        b.partner = Some(10);
        b.partner_name = Some("Commander 10".into());
        b.commander_name = "Partner".into();
        let c = seat(3, 3, 1, 10, true);
        let d = Data::from_rows(vec![], vec![a, b, c], vec![]);
        let r = &d.summary(Scope::Pilots).records;
        assert_eq!(r.len(), 2);
        let pair = r
            .iter()
            .find(|r| r.identity.key.commanders == vec![10, 11])
            .unwrap();
        assert_eq!(pair.wins, 2);
        assert_eq!(pair.identity.key.player, Some(1));
    }
    #[test]
    fn hate_rates_use_actual_exposure_and_unknown_sources_still_count_for_targets() {
        let mut d = sample();
        d.events = vec![
            Event {
                game: 1,
                source: Some(1),
                target: 2,
                kind: HateKind::CommanderKill,
            },
            Event {
                game: 1,
                source: None,
                target: 2,
                kind: HateKind::BoardWipe,
            },
            Event {
                game: 2,
                source: Some(4),
                target: 6,
                kind: HateKind::Counterspell,
            },
        ];
        let d = Data::from_rows(d.games, d.seats, d.events);
        let summary = d.summary(Scope::Players);
        let b = summary
            .records
            .iter()
            .find(|r| r.identity.key.player == Some(2))
            .unwrap();
        assert_eq!(
            (b.received.total(), b.given.total(), b.appearances),
            (2, 1, 6)
        );
        assert_eq!(ratio(b.received.total(), b.appearances), 1.0 / 3.0);
        let a_to_b = summary
            .grudges
            .iter()
            .find(|g| g.source.key.player == Some(1) && g.target.key.player == Some(2))
            .unwrap();
        assert_eq!((a_to_b.events, a_to_b.shared), (1, 5));
    }
    #[test]
    fn no_winner_is_not_a_loss_and_shared_wins_are_not_head_to_head_victories() {
        let d = Data::from_rows(
            vec![],
            vec![
                seat(1, 1, 1, 10, false),
                seat(2, 1, 2, 20, false),
                seat(3, 2, 1, 10, true),
                seat(4, 2, 2, 20, true),
            ],
            vec![],
        );
        let s = d.summary(Scope::Players);
        assert!(s
            .records
            .iter()
            .all(|r| r.losses == 0 && r.wins == 1 && r.unresolved == 1));
        let r = &s.rivalries[0];
        assert_eq!((r.a_wins, r.b_wins, r.unresolved), (0, 0, 2));
    }
    #[test]
    fn baseline_accounts_for_pod_size_and_highlights_require_repeat_evidence() {
        let d = Data::from_rows(
            vec![],
            vec![
                seat(1, 1, 1, 10, true),
                seat(2, 1, 2, 20, false),
                seat(3, 2, 1, 10, false),
                seat(4, 2, 2, 20, false),
                seat(5, 2, 3, 30, false),
                seat(6, 2, 4, 40, true),
            ],
            vec![],
        );
        let r = d
            .summary(Scope::Players)
            .records
            .iter()
            .find(|r| r.identity.key.player == Some(1))
            .unwrap();
        assert_eq!(r.expected_wins, 0.75);
        let mut pair = d.summary(Scope::Players).rivalries[0].clone();
        pair.shared = 2;
        pair.a_wins = 2;
        pair.b_wins = 0;
        assert!(!pair.dominance());
        pair.shared = 5;
        pair.a_wins = 3;
        pair.b_wins = 1;
        assert!(pair.dominance());
        assert!(!pair.close_rivalry());
    }
}
