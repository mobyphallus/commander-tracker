//! Match-linked, per-player feedback. Phone links grant submission only; they
//! never expose other players' responses or the rest of the database.
use crate::screens::game::GameState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
#[path = "feedback_web.rs"]
mod web;
pub use web::Server;

pub fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS feedback_matches (
        match_key TEXT PRIMARY KEY, game_id INTEGER UNIQUE REFERENCES games(id) ON DELETE CASCADE,
        status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','finished','abandoned')));
    CREATE TABLE IF NOT EXISTS feedback_members (
        match_key TEXT NOT NULL REFERENCES feedback_matches(match_key) ON DELETE CASCADE,
        seat INTEGER NOT NULL, player_id INTEGER NOT NULL REFERENCES players(id), name TEXT NOT NULL,
        token TEXT NOT NULL UNIQUE, eligible INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY(match_key,seat));
    CREATE TABLE IF NOT EXISTS game_feedback (
        match_key TEXT NOT NULL, respondent_seat INTEGER NOT NULL,
        rating INTEGER NOT NULL CHECK(rating BETWEEN 1 AND 5),
        problem_seat INTEGER, kingmaker_seat INTEGER,
        notes TEXT NOT NULL CHECK(length(notes)<=2000), submitted_at TEXT NOT NULL, updated_at TEXT NOT NULL,
        PRIMARY KEY(match_key, respondent_seat),
        FOREIGN KEY(match_key,respondent_seat) REFERENCES feedback_members(match_key,seat) ON DELETE CASCADE,
        FOREIGN KEY(match_key,problem_seat) REFERENCES feedback_members(match_key,seat),
        FOREIGN KEY(match_key,kingmaker_seat) REFERENCES feedback_members(match_key,seat));")
}

pub fn sync(conn: &Connection, game: &GameState) -> rusqlite::Result<()> {
    let key = game.started_at.to_rfc3339();
    conn.execute(
        "INSERT OR IGNORE INTO feedback_matches(match_key) VALUES (?1)",
        [&key],
    )?;
    for (seat, player) in game.seats.iter().enumerate() {
        conn.execute(
            "INSERT INTO feedback_members(match_key,seat,player_id,name,token,eligible)
            VALUES (?1,?2,?3,?4,lower(hex(randomblob(16))),?5)
            ON CONFLICT(match_key,seat) DO UPDATE SET eligible=excluded.eligible",
            params![
                key,
                seat,
                player.player.id,
                player.player.name,
                player.eliminated
            ],
        )?;
    }
    Ok(())
}

pub fn abandon(conn: &Connection, game: &GameState) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    conn.execute(
        "UPDATE feedback_matches SET status='abandoned' WHERE match_key=?1 AND game_id IS NULL",
        [game.started_at.to_rfc3339()],
    )?;
    crate::db::clear_active_game(&tx)?;
    tx.commit()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub player_id: i64,
    pub player_name: String,
    pub rating: u8,
    pub problem_player: Option<String>,
    pub kingmaker: Option<String>,
    pub notes: String,
    pub updated_at: String,
}
pub fn for_game(conn: &Connection, game_id: i64) -> rusqlite::Result<Vec<Response>> {
    let mut stmt = conn.prepare(
        "SELECT r.player_id, r.name, f.rating, p.name, k.name, f.notes, f.updated_at
        FROM game_feedback f JOIN feedback_matches m USING(match_key)
        JOIN feedback_members r ON r.match_key=f.match_key AND r.seat=f.respondent_seat
        LEFT JOIN feedback_members p ON p.match_key=f.match_key AND p.seat=f.problem_seat
        LEFT JOIN feedback_members k ON k.match_key=f.match_key AND k.seat=f.kingmaker_seat
        WHERE m.game_id=?1 ORDER BY f.updated_at",
    )?;
    let rows = stmt.query_map([game_id], |row| {
        Ok(Response {
            player_id: row.get(0)?,
            player_name: row.get(1)?,
            rating: row.get(2)?,
            problem_player: row.get(3)?,
            kingmaker: row.get(4)?,
            notes: row.get(5)?,
            updated_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

#[derive(Debug, Clone, Default)]
pub(super) struct Draft {
    pub rating: String,
    pub problem: String,
    pub kingmaker: String,
    pub notes: String,
}
pub(super) struct Form {
    pub key: String,
    pub seat: i64,
    pub name: String,
    pub players: Vec<(i64, String)>,
    pub draft: Draft,
    pub submitted: bool,
}

pub(super) fn form(conn: &Connection, token: &str) -> Result<Form, String> {
    let member: Option<(String,i64,String,bool,String)> = conn.query_row(
        "SELECT r.match_key,r.seat,r.name,r.eligible,m.status FROM feedback_members r JOIN feedback_matches m USING(match_key) WHERE r.token=?1", [token],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(|_| "Feedback is temporarily unavailable. Try again.".to_string())?;
    let Some((key, seat, name, eligible, status)) = member else {
        return Err("This feedback link wasn't found.".into());
    };
    if !eligible || status == "abandoned" {
        return Err(
            "This feedback link is not active. Ask the table for a link after you are marked out."
                .into(),
        );
    }
    let mut stmt = conn
        .prepare("SELECT seat,name FROM feedback_members WHERE match_key=?1 ORDER BY seat")
        .map_err(|_| "Couldn't load players.".to_string())?;
    let players = stmt
        .query_map([&key], |r| Ok((r.get(0)?, r.get(1)?)))
        .and_then(|rows| rows.collect())
        .map_err(|_| "Couldn't load players.".to_string())?;
    let saved = conn.query_row("SELECT rating,problem_seat,kingmaker_seat,notes FROM game_feedback WHERE match_key=?1 AND respondent_seat=?2", params![key,seat], |r| Ok(Draft {
        rating: r.get::<_,i64>(0)?.to_string(), problem: r.get::<_,Option<i64>>(1)?.map(|v|v.to_string()).unwrap_or_default(),
        kingmaker: r.get::<_,Option<i64>>(2)?.map(|v|v.to_string()).unwrap_or_default(), notes: r.get(3)?,
    })).optional().map_err(|_| "Couldn't load your feedback.".to_string())?;
    Ok(Form {
        key,
        seat,
        name,
        players,
        submitted: saved.is_some(),
        draft: saved.unwrap_or_default(),
    })
}

pub(super) fn submit(conn: &mut Connection, token: &str, draft: &Draft) -> Result<(), String> {
    // Re-check eligibility under the write transaction: undo/Back In cannot race
    // a stale phone page into accepting a response from an active player.
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| "The table is saving. Please try again.".to_string())?;
    let current = form(&tx, token)?;
    let rating = draft
        .rating
        .parse::<u8>()
        .ok()
        .filter(|r| (1..=5).contains(r))
        .ok_or("Choose a rating from 1 to 5.")?;
    let selected = |value: &str| -> Result<Option<i64>, String> {
        if value.is_empty() {
            return Ok(None);
        }
        let seat = value
            .parse::<i64>()
            .map_err(|_| "Choose a player from this match.".to_string())?;
        if current.players.iter().any(|p| p.0 == seat) {
            Ok(Some(seat))
        } else {
            Err("Choose a player from this match.".into())
        }
    };
    let problem = selected(&draft.problem)?;
    let kingmaker = selected(&draft.kingmaker)?;
    if draft.notes.chars().count() > 2000 {
        return Err("Keep notes to 2,000 characters or fewer.".into());
    }
    let now = chrono::Utc::now().to_rfc3339();
    tx.execute("INSERT INTO game_feedback(match_key,respondent_seat,rating,problem_seat,kingmaker_seat,notes,submitted_at,updated_at)
        VALUES (?1,?2,?3,?4,?5,?6,?7,?7)
        ON CONFLICT(match_key,respondent_seat) DO UPDATE SET rating=excluded.rating,problem_seat=excluded.problem_seat,kingmaker_seat=excluded.kingmaker_seat,notes=excluded.notes,updated_at=excluded.updated_at",
        params![current.key,current.seat,rating,problem,kingmaker,draft.notes.trim(),now]).map_err(|_| "Couldn't save your feedback. Please try again.".to_string())?;
    tx.commit()
        .map_err(|_| "Couldn't save your feedback. Please try again.".into())
}

#[derive(Default)]
pub struct Service {
    pub server: Option<Server>,
    pub error: Option<String>,
}
impl Service {
    pub fn start(conn: &Connection) -> Self {
        let Some(path) = conn.path().filter(|p| !p.is_empty()) else {
            return Self {
                server: None,
                error: Some("Phone feedback needs a saved database.".into()),
            };
        };
        match Server::start(path.into(), ([0, 0, 0, 0], 8787).into()) {
            Ok(server) => Self {
                server: Some(server),
                error: None,
            },
            Err(e) => Self {
                server: None,
                error: Some(format!("Couldn't start phone feedback on port 8787: {e}")),
            },
        }
    }
}

pub struct Panel {
    pub name: String,
    pub url: Option<String>,
    pub qr: Option<iced::widget::image::Handle>,
    pub error: Option<String>,
}
pub fn panel(conn: &Connection, game: &GameState, seat: usize, service: &Service) -> Panel {
    let mut panel = Panel {
        name: game
            .seats
            .get(seat)
            .map(|s| s.player.name.clone())
            .unwrap_or_default(),
        url: None,
        qr: None,
        error: None,
    };
    if !game.seats.get(seat).is_some_and(|s| s.eliminated) {
        panel.error = Some("This player is still in the game.".into());
        return panel;
    }
    let Some(server) = &service.server else {
        panel.error = service
            .error
            .clone()
            .or_else(|| Some("Phone feedback is unavailable. Reopen the app to retry.".into()));
        return panel;
    };
    let token: rusqlite::Result<String> = conn.query_row(
        "SELECT token FROM feedback_members WHERE match_key=?1 AND seat=?2 AND eligible=1",
        params![game.started_at.to_rfc3339(), seat],
        |r| r.get(0),
    );
    match token {
        Ok(token) => {
            let url = format!("{}/f/{token}", server.base_url());
            panel.qr = qr(&url);
            panel.url = Some(url);
        }
        Err(_) => panel.error = Some("Save this game before opening its feedback link.".into()),
    }
    panel
}
/// Generate each eliminated player's QR once, outside rendering and timer ticks.
pub fn refresh_tiles(conn: &Connection, game: &mut GameState, service: &Service) {
    game.feedback_tiles
        .retain(|seat, _| game.seats.get(*seat).is_some_and(|s| s.eliminated));
    for seat in 0..game.seats.len() {
        if game.seats[seat].eliminated && !game.feedback_tiles.contains_key(&seat) {
            let tile = panel(conn, game, seat, service);
            game.feedback_tiles.insert(seat, tile);
        }
    }
}

pub(crate) fn qr(url: &str) -> Option<iced::widget::image::Handle> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("qrencode")
        .args(["-t", "PNG", "-o", "-", "-s", "6", "-m", "3"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(url.as_bytes()).ok()?;
    let result = child.wait_with_output().ok()?;
    result
        .status
        .success()
        .then(|| iced::widget::image::Handle::from_bytes(result.stdout))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::{
        db,
        model::{Elimination, FinishedGame, OutCause, WinReason},
        session,
    };
    pub(super) fn eliminated() -> (Connection, GameState, String) {
        let (conn, mut game) = session::tests::fixture();
        game.seats[0].mark_out(Elimination {
            cause: OutCause::Concede,
            killer_seat: None,
            turn: 1,
        });
        session::save(&conn, &game).unwrap();
        let token = conn
            .query_row("SELECT token FROM feedback_members WHERE seat=0", [], |r| {
                r.get(0)
            })
            .unwrap();
        (conn, game, token)
    }
    fn draft() -> Draft {
        Draft {
            rating: "4".into(),
            problem: "1".into(),
            kingmaker: "2".into(),
            notes: "Enjoyed the game; too many long turns.".into(),
        }
    }
    fn finished(game: &GameState) -> FinishedGame {
        FinishedGame {
            elapsed_seconds: game.game_seconds,
            seats: game.seats.clone(),
            winner_seat: Some(1),
            win_reason: Some(WinReason::CombatDamage),
            ending_turn: game.turn_number,
            kills: game.kills.clone(),
            started_at: game.started_at,
            ended_at: chrono::Utc::now(),
        }
    }
    #[test]
    fn feedback_survives_recovery_and_attaches_atomically_to_finished_match() {
        let (mut conn, game, token) = eliminated();
        assert_eq!(token.len(), 32);
        submit(&mut conn, &token, &draft()).unwrap();
        let recovered = session::load(&conn).unwrap().unwrap();
        session::save(&conn, &recovered).unwrap();
        let same: String = conn
            .query_row("SELECT token FROM feedback_members WHERE seat=0", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(token, same);
        conn.pragma_update(None, "query_only", true).unwrap();
        assert!(db::record_game(&mut conn, &finished(&game)).is_err());
        conn.pragma_update(None, "query_only", false).unwrap();
        assert!(db::active_game(&conn).unwrap().is_some());
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM feedback_matches WHERE game_id IS NOT NULL",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        db::record_game(&mut conn, &finished(&game)).unwrap();
        let id = db::list_games(&conn).unwrap()[0].id;
        let detail = db::game_detail(&conn, id).unwrap();
        assert_eq!(detail.feedback.len(), 1);
        assert_eq!(
            detail.feedback_players,
            vec![(detail.seats[0].game_player_id, "Ada".into())]
        );
        assert_eq!(detail.feedback[0].player_id, game.seats[0].player.id);
        assert_eq!(detail.feedback[0].problem_player.as_deref(), Some("Bo"));
        assert_eq!(detail.feedback[0].kingmaker.as_deref(), Some("Cy"));
        let mut revised = draft();
        revised.rating = "5".into();
        revised.problem.clear();
        revised.notes = "Changed my mind after the finish.".into();
        submit(&mut conn, &token, &revised).unwrap();
        let rows = for_game(&conn, id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].rating, 5);
        assert!(rows[0].problem_player.is_none());
        let second = GameState::new(
            game.seats.clone(),
            game.table_layout.clone(),
            game.turn_order.clone(),
        );
        session::save(&conn, &second).unwrap();
        let second_token: String = conn
            .query_row(
                "SELECT token FROM feedback_members WHERE match_key=?1 AND seat=0",
                [second.started_at.to_rfc3339()],
                |r| r.get(0),
            )
            .unwrap();
        assert_ne!(token, second_token);
        assert!(!form(&conn, &second_token).unwrap().submitted);
    }
    #[test]
    fn back_in_abandon_and_invalid_submissions_do_not_change_feedback() {
        let (mut conn, mut game, token) = eliminated();
        let active_token: String = conn
            .query_row("SELECT token FROM feedback_members WHERE seat=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(form(&conn, &active_token).is_err());
        for invalid in [
            Draft {
                rating: "0".into(),
                ..draft()
            },
            Draft {
                problem: "9".into(),
                ..draft()
            },
            Draft {
                kingmaker: "-1".into(),
                ..draft()
            },
            Draft {
                notes: "x".repeat(2001),
                ..draft()
            },
        ] {
            assert!(submit(&mut conn, &token, &invalid).is_err());
        }
        assert!(!form(&conn, &token).unwrap().submitted);
        submit(&mut conn, &token, &draft()).unwrap();
        let _ = crate::screens::game::update(
            &mut game,
            &mut conn,
            crate::screens::game::GameMessage::ToggleEliminated(0),
        );
        assert!(!game.seats[0].eliminated);
        assert!(submit(&mut conn, &token, &draft()).is_err());
        let _ = crate::screens::game::update(
            &mut game,
            &mut conn,
            crate::screens::game::GameMessage::Undo,
        );
        assert!(game.seats[0].eliminated);
        assert!(form(&conn, &token).unwrap().submitted);
        abandon(&conn, &game).unwrap();
        assert!(form(&conn, &token).is_err());
        assert_eq!(
            conn.query_row("SELECT count(*) FROM game_feedback", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn backup_and_restore_preserve_feedback_and_personal_links() {
        let (mut source, game, token) = eliminated();
        submit(&mut source, &token, &draft()).unwrap();
        db::record_game(&mut source, &finished(&game)).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "feedback-backup-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let path = crate::storage::backup(&source, &dir).unwrap();
        let (mut target, _) = session::tests::fixture();
        crate::storage::restore(&mut target, &path, &dir).unwrap();
        assert!(form(&target, &token).unwrap().submitted);
        let id = db::list_games(&target).unwrap()[0].id;
        assert_eq!(for_game(&target, id).unwrap()[0].rating, 4);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

pub fn invites_for_game(conn: &Connection, game_id: i64) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT gp.id,r.name FROM feedback_matches m
        JOIN feedback_members r USING(match_key)
        JOIN game_players gp ON gp.game_id=m.game_id AND gp.seat=r.seat
        WHERE m.game_id=?1 AND m.status='finished' AND r.eligible=1 ORDER BY r.seat",
    )?;
    let rows = stmt.query_map([game_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}
pub fn saved_panel(conn: &Connection, game_player: i64, service: &Service) -> Panel {
    let member:rusqlite::Result<(String,String)>=conn.query_row("SELECT r.name,r.token FROM game_players gp
        JOIN feedback_matches m ON m.game_id=gp.game_id JOIN feedback_members r ON r.match_key=m.match_key AND r.seat=gp.seat
        WHERE gp.id=?1 AND r.eligible=1 AND m.status='finished'",[game_player],|r|Ok((r.get(0)?,r.get(1)?)));
    match member {
        Ok((name, token)) => {
            if let Some(server) = &service.server {
                let url = format!("{}/f/{token}", server.base_url());
                Panel {
                    name,
                    qr: qr(&url),
                    url: Some(url),
                    error: None,
                }
            } else {
                Panel {
                    name,
                    url: None,
                    qr: None,
                    error: service.error.clone().or_else(|| {
                        Some("Phone feedback is unavailable. Reopen the app to retry.".into())
                    }),
                }
            }
        }
        Err(_) => Panel {
            name: "Feedback".into(),
            url: None,
            qr: None,
            error: Some("No feedback link is available for this player in this match.".into()),
        },
    }
}
pub fn view(
    panel: &Panel,
    on_close: crate::app::Message,
) -> iced::Element<'_, crate::app::Message> {
    use crate::style;
    use iced::{
        widget::{column, container, image, scrollable, text},
        Alignment, Length,
    };
    let mut content = column![
        text(format!("{} · Rate this game", panel.name)).size(style::T_HEADING),
        text("Scan with your phone on the same Wi-Fi or local network.")
            .size(style::T_BODY)
            .color(style::TEXT_MUTED),
        text("Your rating and notes are linked to you and saved with this match.")
            .size(style::T_BODY),
    ]
    .spacing(style::GAP)
    .align_x(Alignment::Center);
    if let Some(qr) = &panel.qr {
        content = content.push(image(qr.clone()).width(260).height(260));
    }
    if let Some(url) = &panel.url {
        content = content.push(text(url).size(style::T_CAPTION));
        if url.contains("127.0.0.1") {
            content=content.push(text("No local-network address found. Connect the laptop to Wi-Fi and reopen the app.").color(style::DANGER));
        }
    }
    if let Some(error) = &panel.error {
        content = content.push(text(error).color(style::DANGER));
    }
    content = content.push(
        style::touch_button("Back", style::T_ACTION)
            .style(style::primary)
            .on_press(on_close),
    );
    container(
        container(scrollable(content).height(Length::Shrink))
            .padding(24)
            .max_width(640)
            .style(style::panel),
    )
    .padding(24)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}
