use std::path::PathBuf;

use rusqlite::{params, Connection};

use crate::model::{
    Commander, FinishedGame, GameDetail, GameDetailSeat, GameSummary, MatchupStat, Player,
    PlayerStat, WinReason,
};

pub fn data_dir() -> PathBuf {
    let mut dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    dir.push("commander_pod");
    std::fs::create_dir_all(&dir).ok();
    dir
}

pub fn open() -> rusqlite::Result<Connection> {
    let mut path = data_dir();
    path.push("pod.db");
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    init(&conn)?;
    Ok(conn)
}

fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS players (
            id   INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS commanders (
            id              INTEGER PRIMARY KEY,
            scryfall_id     TEXT NOT NULL UNIQUE,
            name            TEXT NOT NULL,
            image_url       TEXT,
            art_crop_url    TEXT,
            color_identity  TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS games (
            id          INTEGER PRIMARY KEY,
            started_at  TEXT NOT NULL,
            ended_at    TEXT NOT NULL,
            pod_size    INTEGER NOT NULL,
            win_reason  TEXT
        );

        CREATE TABLE IF NOT EXISTS game_players (
            id              INTEGER PRIMARY KEY,
            game_id         INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
            player_id       INTEGER NOT NULL REFERENCES players(id),
            commander_id    INTEGER NOT NULL REFERENCES commanders(id),
            seat            INTEGER NOT NULL,
            final_life      INTEGER NOT NULL,
            final_poison    INTEGER NOT NULL,
            won             INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS commander_damage (
            id                    INTEGER PRIMARY KEY,
            game_id               INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
            target_game_player_id INTEGER NOT NULL REFERENCES game_players(id),
            source_game_player_id INTEGER NOT NULL REFERENCES game_players(id),
            amount                INTEGER NOT NULL
        );
        "#,
    )
}

pub fn list_players(conn: &Connection) -> rusqlite::Result<Vec<Player>> {
    let mut stmt = conn.prepare("SELECT id, name FROM players ORDER BY name COLLATE NOCASE")?;
    let rows = stmt.query_map([], |row| {
        Ok(Player {
            id: row.get(0)?,
            name: row.get(1)?,
        })
    })?;
    rows.collect()
}

pub fn create_player(conn: &Connection, name: &str) -> rusqlite::Result<Player> {
    conn.execute("INSERT INTO players (name) VALUES (?1)", params![name])?;
    Ok(Player {
        id: conn.last_insert_rowid(),
        name: name.to_string(),
    })
}

pub fn list_commanders(conn: &Connection) -> rusqlite::Result<Vec<Commander>> {
    let mut stmt = conn.prepare(
        "SELECT id, scryfall_id, name, image_url, art_crop_url, color_identity
         FROM commanders ORDER BY name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Commander {
            id: row.get(0)?,
            scryfall_id: row.get(1)?,
            name: row.get(2)?,
            image_url: row.get(3)?,
            art_crop_url: row.get(4)?,
            color_identity: row.get(5)?,
        })
    })?;
    rows.collect()
}

/// Inserts a commander cached from Scryfall if we haven't seen it before,
/// otherwise returns the existing local copy.
pub fn upsert_commander(
    conn: &Connection,
    scryfall_id: &str,
    name: &str,
    image_url: Option<&str>,
    art_crop_url: Option<&str>,
    color_identity: &str,
) -> rusqlite::Result<Commander> {
    conn.execute(
        "INSERT INTO commanders (scryfall_id, name, image_url, art_crop_url, color_identity)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(scryfall_id) DO UPDATE SET
            name = excluded.name,
            image_url = excluded.image_url,
            art_crop_url = excluded.art_crop_url,
            color_identity = excluded.color_identity",
        params![scryfall_id, name, image_url, art_crop_url, color_identity],
    )?;
    conn.query_row(
        "SELECT id, scryfall_id, name, image_url, art_crop_url, color_identity
         FROM commanders WHERE scryfall_id = ?1",
        params![scryfall_id],
        |row| {
            Ok(Commander {
                id: row.get(0)?,
                scryfall_id: row.get(1)?,
                name: row.get(2)?,
                image_url: row.get(3)?,
                art_crop_url: row.get(4)?,
                color_identity: row.get(5)?,
            })
        },
    )
}

pub fn record_game(conn: &mut Connection, game: &FinishedGame) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO games (started_at, ended_at, pod_size, win_reason) VALUES (?1, ?2, ?3, ?4)",
        params![
            game.started_at.to_rfc3339(),
            game.ended_at.to_rfc3339(),
            game.seats.len() as i64,
            game.win_reason.map(|r| r.as_db_str())
        ],
    )?;
    let game_id = tx.last_insert_rowid();

    // Map seat index -> the game_players row id, so commander damage can reference it.
    let mut game_player_ids = Vec::with_capacity(game.seats.len());
    for (seat_index, seat) in game.seats.iter().enumerate() {
        let won = game.winner_seat == Some(seat_index);
        tx.execute(
            "INSERT INTO game_players
                (game_id, player_id, commander_id, seat, final_life, final_poison, won)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                game_id,
                seat.player.id,
                seat.commander.id,
                seat_index as i64,
                seat.life,
                seat.poison,
                won as i64
            ],
        )?;
        game_player_ids.push(tx.last_insert_rowid());
    }

    for (seat_index, seat) in game.seats.iter().enumerate() {
        for (&source_index, &amount) in seat.commander_damage_taken.iter() {
            if amount <= 0 {
                continue;
            }
            tx.execute(
                "INSERT INTO commander_damage
                    (game_id, target_game_player_id, source_game_player_id, amount)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    game_id,
                    game_player_ids[seat_index],
                    game_player_ids[source_index],
                    amount
                ],
            )?;
        }
    }

    tx.commit()
}

pub fn commander_matchup_stats(conn: &Connection) -> rusqlite::Result<Vec<MatchupStat>> {
    // For every pair of commanders that shared a game, tally how often each side won.
    let mut stmt = conn.prepare(
        r#"
        SELECT ca.name, cb.name,
               SUM(CASE WHEN gpa.won = 1 THEN 1 ELSE 0 END) AS a_wins,
               SUM(CASE WHEN gpb.won = 1 THEN 1 ELSE 0 END) AS b_wins,
               COUNT(*) AS games
        FROM game_players gpa
        JOIN game_players gpb
            ON gpa.game_id = gpb.game_id AND gpa.id < gpb.id
        JOIN commanders ca ON ca.id = gpa.commander_id
        JOIN commanders cb ON cb.id = gpb.commander_id
        GROUP BY ca.name, cb.name
        ORDER BY games DESC, ca.name, cb.name
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(MatchupStat {
            commander_a: row.get(0)?,
            commander_b: row.get(1)?,
            a_wins: row.get(2)?,
            b_wins: row.get(3)?,
            games: row.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn player_stats(conn: &Connection) -> rusqlite::Result<Vec<PlayerStat>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT p.name, COUNT(*) AS games, SUM(gp.won) AS wins
        FROM game_players gp
        JOIN players p ON p.id = gp.player_id
        GROUP BY p.name
        ORDER BY wins DESC, games DESC
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PlayerStat {
            player_name: row.get(0)?,
            games: row.get(1)?,
            wins: row.get(2)?,
        })
    })?;
    rows.collect()
}

fn parse_dt(s: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now())
}

pub fn list_games(conn: &Connection) -> rusqlite::Result<Vec<GameSummary>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT g.id, g.started_at, g.ended_at, g.pod_size, g.win_reason, p.name, c.name
        FROM games g
        LEFT JOIN game_players gp ON gp.game_id = g.id AND gp.won = 1
        LEFT JOIN players p ON p.id = gp.player_id
        LEFT JOIN commanders c ON c.id = gp.commander_id
        ORDER BY g.started_at DESC
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        let started_at: String = row.get(1)?;
        let ended_at: String = row.get(2)?;
        let win_reason: Option<String> = row.get(4)?;
        Ok(GameSummary {
            id: row.get(0)?,
            started_at: parse_dt(&started_at),
            ended_at: parse_dt(&ended_at),
            pod_size: row.get(3)?,
            win_reason: win_reason.map(|s| WinReason::from_db_str(&s)),
            winner_name: row.get(5)?,
            winner_commander: row.get(6)?,
        })
    })?;
    rows.collect()
}

pub fn game_detail(conn: &Connection, game_id: i64) -> rusqlite::Result<GameDetail> {
    let (started_at, ended_at, win_reason): (String, String, Option<String>) = conn.query_row(
        "SELECT started_at, ended_at, win_reason FROM games WHERE id = ?1",
        params![game_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;

    struct SeatRow {
        id: i64,
        player_id: i64,
        commander_id: i64,
        final_life: i32,
        final_poison: i32,
        won: bool,
    }

    let seat_rows: Vec<SeatRow> = {
        let mut stmt = conn.prepare(
            "SELECT id, player_id, commander_id, final_life, final_poison, won
             FROM game_players WHERE game_id = ?1 ORDER BY seat",
        )?;
        let result = stmt
            .query_map(params![game_id], |row| {
                Ok(SeatRow {
                    id: row.get(0)?,
                    player_id: row.get(1)?,
                    commander_id: row.get(2)?,
                    final_life: row.get(3)?,
                    final_poison: row.get(4)?,
                    won: row.get::<_, i64>(5)? != 0,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        result
    };

    let mut seats = Vec::with_capacity(seat_rows.len());
    for r in &seat_rows {
        let player_name: String = conn.query_row(
            "SELECT name FROM players WHERE id = ?1",
            params![r.player_id],
            |row| row.get(0),
        )?;
        let commander_name: String = conn.query_row(
            "SELECT name FROM commanders WHERE id = ?1",
            params![r.commander_id],
            |row| row.get(0),
        )?;

        let damage_taken: Vec<(String, i32)> = {
            let mut stmt = conn.prepare(
                "SELECT c.name, cd.amount
                 FROM commander_damage cd
                 JOIN game_players sp ON sp.id = cd.source_game_player_id
                 JOIN commanders c ON c.id = sp.commander_id
                 WHERE cd.target_game_player_id = ?1",
            )?;
            let result = stmt
                .query_map(params![r.id], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            result
        };

        seats.push(GameDetailSeat {
            player_name,
            commander_name,
            final_life: r.final_life,
            final_poison: r.final_poison,
            won: r.won,
            damage_taken,
        });
    }

    Ok(GameDetail {
        id: game_id,
        started_at: parse_dt(&started_at),
        ended_at: parse_dt(&ended_at),
        win_reason: win_reason.map(|s| WinReason::from_db_str(&s)),
        seats,
    })
}
