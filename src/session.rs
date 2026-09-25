//! Versioned, local recovery and bounded undo. No network or alternate data path.
use crate::{
    app::Message,
    db,
    screens::game::{self, Action, GameMessage, GameState},
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct UndoEntry {
    pub label: String,
    snapshot: String,
}
#[derive(Serialize, Deserialize)]
struct SavedGame {
    version: u32,
    game: GameState,
    #[serde(default)]
    undo: Vec<UndoEntry>,
}

pub fn save(conn: &Connection, state: &GameState) -> Result<(), String> {
    let payload = serde_json::json!({"version": 1, "game": state, "undo": state.undo});
    db::save_active_game(conn, &payload.to_string())
        .map_err(|e| format!("Autosave failed: {e}. Keep the app open and try saving again."))
}

pub fn load(conn: &Connection) -> Result<Option<GameState>, String> {
    db::active_game(conn)
        .map_err(|e| e.to_string())?
        .map(|raw| decode(&raw))
        .transpose()
}

fn decode(raw: &str) -> Result<GameState, String> {
    let saved: SavedGame =
        serde_json::from_str(raw).map_err(|e| format!("Couldn’t read the saved game: {e}"))?;
    if saved.version != 1 {
        return Err("This saved game needs a newer version of Commander Pod.".into());
    }
    let mut state = saved.game;
    let n = state.seats.len();
    let valid_order = |indices: Vec<usize>| {
        let mut v = indices;
        v.sort_unstable();
        v == (0..n).collect::<Vec<_>>()
    };
    if !(2..=8).contains(&n)
        || !valid_order(state.turn_order.clone())
        || !valid_order(
            state
                .table_layout
                .columns
                .iter()
                .flatten()
                .copied()
                .collect(),
        )
        || state
            .table_layout
            .columns
            .iter()
            .any(|c| c.is_empty() || c.len() > 2)
        || state.turn_index >= n
        || state.active_seat != state.turn_order[state.turn_index]
        || !state.valid_recovery_lengths()
        || state.seats.iter().any(|s| {
            s.commander_damage_taken
                .keys()
                .any(|&(i, slot)| i >= n || slot > 1)
        })
    {
        return Err(
            "The saved game has invalid seating data. Its saved copy has been kept.".into(),
        );
    }
    state.undo = saved.undo.into_iter().rev().take(40).collect();
    state.undo.reverse();
    state.paused = true;
    state.game_menu_open = true;
    state.pending_abandon = false;
    Ok(state)
}

fn label(message: &GameMessage) -> Option<&'static str> {
    match message {
        GameMessage::CounterPressEnd(..) | GameMessage::HoldTick => Some("Changed a counter"),
        GameMessage::NextTurn => Some("Advanced the turn"),
        GameMessage::ConfirmHate(_) => Some("Logged commander hate"),
        GameMessage::ToggleEliminated(_)
        | GameMessage::PickOutCause(_)
        | GameMessage::ConfirmOutKiller(_) => Some("Changed player elimination"),
        _ => None,
    }
}

pub fn update(
    state: &mut GameState,
    conn: &mut Connection,
    message: GameMessage,
) -> (iced::Task<Message>, Option<Action>) {
    let tick = matches!(message, GameMessage::Tick);
    let label = label(&message);
    let core_before = serde_json::json!([
        state.seats,
        state.kills,
        state.turn_index,
        state.turn_number
    ]);
    let before = serde_json::to_string(state).ok();
    let (task, action) = game::update_inner(state, conn, message);
    if action.is_some() {
        return (task, action);
    }
    let after = serde_json::to_string(state).ok();
    if before != after {
        let core_after = serde_json::json!([
            state.seats,
            state.kills,
            state.turn_index,
            state.turn_number
        ]);
        if let (Some(label), Some(snapshot)) = (label.filter(|_| core_before != core_after), before)
        {
            state.undo.push(UndoEntry {
                label: describe_action(&snapshot, state, label),
                snapshot,
            });
            if state.undo.len() > 40 {
                state.undo.remove(0);
            }
        }
        if !tick || state.game_seconds % 5 == 0 {
            if let Err(e) = save(conn, state) {
                state.error = Some(e);
            }
        }
    }
    (task, action)
}

fn describe_action(snapshot: &str, after: &GameState, fallback: &str) -> String {
    let Ok(before) = serde_json::from_str::<GameState>(snapshot) else {
        return fallback.into();
    };
    if before.active_seat != after.active_seat || before.turn_number != after.turn_number {
        return format!(
            "Turn passed to {} · turn {}",
            after.seats[after.active_seat].player.name, after.turn_number
        );
    }
    if after.kills.len() > before.kills.len() {
        if let Some(event) = after.kills.last() {
            return format!(
                "{} · {}",
                after.seats[event.victim_seat].player.name,
                event.kind.label()
            );
        }
    }
    for (a, b) in before.seats.iter().zip(&after.seats) {
        if a.commander_damage_taken != b.commander_damage_taken {
            return format!("{} · commander damage changed", b.player.name);
        }
        if a.life != b.life {
            return format!("{} · life {} → {}", b.player.name, a.life, b.life);
        }
        if a.poison != b.poison {
            return format!("{} · poison {} → {}", b.player.name, a.poison, b.poison);
        }
        if a.eliminated != b.eliminated {
            return format!(
                "{} · {}",
                b.player.name,
                if b.eliminated {
                    "marked out"
                } else {
                    "returned to game"
                }
            );
        }
    }
    fallback.into()
}

pub fn undo(state: &mut GameState) {
    let Some(entry) = state.undo.pop() else {
        return;
    };
    match serde_json::from_str::<GameState>(&entry.snapshot) {
        Ok(mut previous) => {
            previous.game_seconds = state.game_seconds;
            if previous.active_seat == state.active_seat
                && previous.turn_number == state.turn_number
            {
                previous.turn_seconds = state.turn_seconds;
            }
            previous.paused = state.paused;
            previous.undo = std::mem::take(&mut state.undo);
            previous.undo_open = true;
            previous.pending_winner = None;
            previous.pending_reason = None;
            previous.pending_life_check = None;
            previous.out_flow = None;
            previous.hate_flow = None;
            previous.press_hold = None;
            *state = previous;
        }
        Err(e) => {
            state.undo.push(entry);
            state.error = Some(format!("Couldn’t undo this action: {e}"));
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        layout,
        model::{HateKind, Seat, WinReason},
        screens::game::CounterTarget,
    };

    pub fn fixture() -> (Connection, GameState) {
        let conn = Connection::open_in_memory().unwrap();
        db::init(&conn).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        let seats = ["Ada", "Bo", "Cy"]
            .into_iter()
            .map(|name| {
                let p = db::create_player(&conn, name).unwrap();
                let c = db::upsert_commander(
                    &conn,
                    name,
                    &format!("{name} commander"),
                    None,
                    None,
                    "UB",
                )
                .unwrap();
                Seat::new(p, c, 40)
            })
            .collect();
        (
            conn,
            GameState::new(seats, layout::options_for(3)[0].clone(), vec![2, 0, 1]),
        )
    }
    fn send(conn: &mut Connection, state: &mut GameState, msg: GameMessage) {
        let _ = update(state, conn, msg);
    }
    fn tap(conn: &mut Connection, state: &mut GameState, target: CounterTarget, sign: i32) {
        send(conn, state, GameMessage::CounterPressStart(0, target, sign));
        send(conn, state, GameMessage::CounterPressEnd(target, sign));
    }

    #[test]
    fn recovery_round_trips_partners_borrowing_damage_turns_and_undo() {
        let (mut conn, mut game) = fixture();
        game.seats[0].partner = Some(game.seats[1].commander.clone());
        game.seats[0].borrowed_from = Some(game.seats[2].player.clone());
        tap(&mut conn, &mut game, CounterTarget::Damage(1, 0, 1), 1);
        send(&mut conn, &mut game, GameMessage::NextTurn);
        game.turn_seconds = 31;
        game.game_seconds = 250;
        save(&conn, &game).unwrap();
        let loaded = load(&conn).unwrap().unwrap();
        assert_eq!(loaded.seats[1].commander_damage_taken[&(0, 1)], 1);
        assert_eq!(loaded.seats[1].life, 39);
        assert_eq!(loaded.seats[0].partner, game.seats[0].partner);
        assert_eq!(loaded.seats[0].borrowed_from, game.seats[0].borrowed_from);
        assert_eq!(
            (loaded.active_seat, loaded.turn_seconds, loaded.game_seconds),
            (0, 31, 250)
        );
        assert!(loaded.paused);
        assert_eq!(loaded.undo.len(), 2);
        assert!(db::delete_player(&conn, game.seats[0].player.id).is_err());
    }

    #[test]
    fn undo_restores_lethal_damage_hate_and_turn_timer() {
        let (mut conn, mut game) = fixture();
        game.seats[1].commander_damage_taken.insert((0, 0), 20);
        tap(&mut conn, &mut game, CounterTarget::Damage(1, 0, 0), 1);
        assert!(game.seats[1].eliminated);
        send(&mut conn, &mut game, GameMessage::Undo);
        assert!(!game.seats[1].eliminated);
        assert_eq!(game.seats[1].life, 40);
        assert_eq!(game.seats[1].commander_damage_taken[&(0, 0)], 20);
        send(&mut conn, &mut game, GameMessage::StartHate(0));
        send(
            &mut conn,
            &mut game,
            GameMessage::PickHateKind(HateKind::Counterspell),
        );
        send(&mut conn, &mut game, GameMessage::ConfirmHate(Some(1)));
        assert_eq!(game.kills.len(), 1);
        send(&mut conn, &mut game, GameMessage::Undo);
        assert!(game.kills.is_empty());
        game.turn_seconds = 42;
        send(&mut conn, &mut game, GameMessage::NextTurn);
        assert_eq!(game.turn_seconds, 0);
        send(&mut conn, &mut game, GameMessage::Undo);
        assert_eq!(
            (game.active_seat, game.turn_number, game.turn_seconds),
            (2, 1, 42)
        );
    }

    #[test]
    fn retrying_autosave_does_not_confirm_an_unfinished_winner_dialog() {
        let (mut conn, mut game) = fixture();
        conn.pragma_update(None, "query_only", true).unwrap();
        send(&mut conn, &mut game, GameMessage::StartDeclareWinner(0));
        send(
            &mut conn,
            &mut game,
            GameMessage::PickWinReason(WinReason::Poison),
        );
        assert!(game.error.is_some());
        conn.pragma_update(None, "query_only", false).unwrap();
        assert!(update(&mut game, &mut conn, GameMessage::RetrySave)
            .1
            .is_none());
        assert!(db::list_games(&conn).unwrap().is_empty());
        assert!(db::active_game(&conn).unwrap().is_some());
    }

    #[test]
    fn swipe_is_not_an_undo_action_and_history_is_bounded() {
        let (mut conn, mut game) = fixture();
        send(&mut conn, &mut game, GameMessage::StartDamageFocus(0));
        assert!(game.undo.is_empty());
        for _ in 0..45 {
            tap(&mut conn, &mut game, CounterTarget::Life(0), 1);
        }
        assert_eq!(game.undo.len(), 40);
        assert!(game.undo.last().unwrap().label.contains("Ada"));
    }

    #[test]
    fn rematch_preserves_decks_and_requires_new_turn_order() {
        let (conn, mut game) = fixture();
        game.seats[0].partner = Some(game.seats[1].commander.clone());
        game.seats[0].borrowed_from = Some(game.seats[2].player.clone());
        game.seats[0].life = 3;
        let setup = crate::screens::setup::SetupState::rematch(&game, &conn);
        assert_eq!(setup.stage, crate::screens::setup::SetupStage::TurnOrder);
        assert_eq!(setup.first_seat, None);
        assert_eq!(setup.seats[0].partner, game.seats[0].partner);
        assert_eq!(setup.seats[0].borrowed_from, game.seats[0].borrowed_from);
        assert!(setup.all_seats_ready());
    }

    #[test]
    fn touch_help_pauses_and_remembers_dismissal() {
        let (mut conn, mut game) = fixture();
        send(&mut conn, &mut game, GameMessage::ShowHelp);
        assert!(game.paused);
        send(&mut conn, &mut game, GameMessage::Tick);
        assert_eq!(game.game_seconds, 0);
        send(&mut conn, &mut game, GameMessage::DismissHelp);
        assert!(!game.paused);
        assert_eq!(
            db::setting(&conn, "gestures_seen").unwrap().as_deref(),
            Some("yes")
        );
        game.paused = true;
        send(&mut conn, &mut game, GameMessage::ShowHelp);
        send(&mut conn, &mut game, GameMessage::DismissHelp);
        assert!(game.paused);
    }

    #[test]
    fn invalid_or_newer_recovery_is_retained_not_overwritten() {
        let (conn, game) = fixture();
        save(&conn, &game).unwrap();
        let mut raw: serde_json::Value =
            serde_json::from_str(&db::active_game(&conn).unwrap().unwrap()).unwrap();
        raw["game"]["turn_order"] = serde_json::json!([0, 0, 1]);
        db::save_active_game(&conn, &raw.to_string()).unwrap();
        assert!(load(&conn).is_err());
        assert!(db::active_game(&conn).unwrap().is_some());
        raw["version"] = serde_json::json!(99);
        assert!(decode(&raw.to_string()).is_err());
    }

    #[test]
    fn save_failure_is_visible_and_finish_is_atomic() {
        let (mut conn, mut game) = fixture();
        save(&conn, &game).unwrap();
        conn.pragma_update(None, "query_only", true).unwrap();
        tap(&mut conn, &mut game, CounterTarget::Life(0), -1);
        assert_eq!(game.seats[0].life, 39);
        assert!(game.error.is_some());
        game.pending_winner = Some(0);
        game.pending_reason = Some(WinReason::CombatDamage);
        assert!(update(&mut game, &mut conn, GameMessage::ConfirmEndGame)
            .1
            .is_none());
        assert!(db::active_game(&conn).unwrap().is_some());
        conn.pragma_update(None, "query_only", false).unwrap();
        game.game_seconds = 120;
        assert!(matches!(
            update(&mut game, &mut conn, GameMessage::RetrySave).1,
            Some(Action::Finished)
        ));
        assert!(db::active_game(&conn).unwrap().is_none());
        let games = db::list_games(&conn).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(
            db::game_detail(&conn, games[0].id).unwrap().elapsed_seconds,
            Some(120)
        );
        let stats = crate::screens::stats::data::Data::load(&conn).unwrap();
        assert_eq!(stats.games[0].minutes, 2.0);
    }
}
