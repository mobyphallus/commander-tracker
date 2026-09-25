use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension};

use crate::model::{
    ArtFraming, Commander, FinishedGame, GameDetail, GameDetailKill, GameDetailOut, GameDetailSeat,
    GameSummary, GrudgeStat, HateKind, HatedCommanderStat, HaterStat, MatchupStat, OutCause,
    Player, PlayerStat, SavedDeck, WinReason, WinReasonStat,
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
            oracle_id       TEXT NOT NULL UNIQUE,
            name            TEXT NOT NULL,
            image_url       TEXT,
            art_crop_url    TEXT,
            color_identity  TEXT NOT NULL DEFAULT '',
            art_zoom        REAL NOT NULL DEFAULT 1.0,
            art_anchor      TEXT NOT NULL DEFAULT 'center'
        );

        CREATE TABLE IF NOT EXISTS games (
            id          INTEGER PRIMARY KEY,
            started_at  TEXT NOT NULL,
            ended_at    TEXT NOT NULL,
            pod_size    INTEGER NOT NULL,
            win_reason  TEXT,
            ending_turn INTEGER NOT NULL DEFAULT 1
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

        CREATE TABLE IF NOT EXISTS commander_kills (
            id                     INTEGER PRIMARY KEY,
            game_id                INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
            victim_game_player_id  INTEGER NOT NULL REFERENCES game_players(id),
            killer_game_player_id  INTEGER REFERENCES game_players(id),
            kind                   TEXT NOT NULL DEFAULT 'commander_kill'
        );

        CREATE TABLE IF NOT EXISTS eliminations (
            id                     INTEGER PRIMARY KEY,
            game_id                INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
            victim_game_player_id  INTEGER NOT NULL REFERENCES game_players(id),
            killer_game_player_id  INTEGER REFERENCES game_players(id),
            cause                  TEXT NOT NULL,
            turn                   INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS player_commanders (
            player_id     INTEGER NOT NULL REFERENCES players(id),
            commander_id  INTEGER NOT NULL REFERENCES commanders(id),
            last_used_at  TEXT NOT NULL,
            PRIMARY KEY (player_id, commander_id)
        );
        "#,
    )?;

    migrate_scryfall_id_to_oracle_id(conn)?;
    migrate_add_ending_turn(conn)?;
    migrate_add_hate_kind(conn)?;
    migrate_add_art_framing(conn)?;
    migrate_add_partners(conn)?;
    migrate_add_art_framing_table(conn)?;
    migrate_add_deck_analysis(conn)?;
    migrate_add_borrowed_decks(conn)
}

/// A saved deck can be pointed at its Moxfield list, and once it is we keep
/// what was worked out about it: the bracket, the salt total, and the whole
/// breakdown as JSON.
///
/// Storing the breakdown whole rather than normalised into tables is
/// deliberate. It's a read-only artefact of one moment - nothing queries
/// inside it, the shape belongs to `crate::salt`, and keeping it as one blob
/// means the screen still opens at a table with no wifi. It's keyed the same
/// way `player_commanders` is, because a Moxfield link belongs to one
/// player's deck rather than to the commander in general: two people can
/// both play Atraxa and they are not the same deck.
fn migrate_add_deck_analysis(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS deck_analysis (
            player_id     INTEGER NOT NULL REFERENCES players(id),
            commander_id  INTEGER NOT NULL REFERENCES commanders(id),
            public_id     TEXT NOT NULL,
            url           TEXT NOT NULL,
            deck_name     TEXT NOT NULL DEFAULT '',
            bracket       INTEGER,
            salt_total    REAL,
            breakdown     TEXT,
            analysed_at   TEXT,
            PRIMARY KEY (player_id, commander_id)
        );",
    )
}

/// Framing moved off the commander row and onto a per-tile table: the same
/// art needs a different crop in a tall head-of-table tile than in a short
/// wide one, so it's keyed by layout and seat. The old `commanders.art_zoom`
/// / `art_anchor` columns are left in place but no longer read - the anchor
/// was only ever a 3x3 grid, and can't be translated into a free pan.
fn migrate_add_art_framing_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS art_framing (
            commander_id INTEGER NOT NULL REFERENCES commanders(id),
            layout_name  TEXT NOT NULL,
            seat         INTEGER NOT NULL,
            zoom         REAL NOT NULL,
            pan_x        REAL NOT NULL,
            pan_y        REAL NOT NULL,
            PRIMARY KEY (commander_id, layout_name, seat)
        );",
    )?;
    let has_pair: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('player_commanders') WHERE name = 'partner_commander_id'")?
        .exists([])?;
    if !has_pair {
        conn.execute_batch(
            "ALTER TABLE player_commanders ADD COLUMN partner_commander_id INTEGER REFERENCES commanders(id);",
        )?;
    }
    Ok(())
}

/// Partner pairs put a second commander in a seat, and make commander
/// damage a per-commander total rather than a per-seat one. Existing rows
/// are single-commander games, so they default to no partner and all damage
/// attributed to the primary.
/// Who owned the deck someone played, when it wasn't them.
///
/// NULL means the pilot's own deck, which is what every row written before
/// this column existed was - so there's nothing to backfill. Stats
/// deliberately don't read it: a borrowed game belongs to the deck and to
/// the player who piloted it, and the owner gets no credit for a game they
/// weren't in. It's here so history can say whose deck it was.
fn migrate_add_borrowed_decks(conn: &Connection) -> rusqlite::Result<()> {
    let has_owner: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('game_players') WHERE name = 'owner_player_id'")?
        .exists([])?;
    if !has_owner {
        conn.execute_batch(
            "ALTER TABLE game_players ADD COLUMN owner_player_id INTEGER REFERENCES players(id);",
        )?;
    }
    Ok(())
}

fn migrate_add_partners(conn: &Connection) -> rusqlite::Result<()> {
    let has_partner: bool = conn
        .prepare(
            "SELECT 1 FROM pragma_table_info('game_players') WHERE name = 'partner_commander_id'",
        )?
        .exists([])?;
    if !has_partner {
        conn.execute_batch(
            "ALTER TABLE game_players ADD COLUMN partner_commander_id INTEGER REFERENCES commanders(id);
             ALTER TABLE commander_damage ADD COLUMN source_slot INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    Ok(())
}

/// Early builds keyed `commanders` by a specific printing's Scryfall id. That
/// fragmented stats every time someone picked different art for the same
/// card, so the column was renamed to hold the oracle id instead.
fn migrate_scryfall_id_to_oracle_id(conn: &Connection) -> rusqlite::Result<()> {
    let has_old_column: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('commanders') WHERE name = 'scryfall_id'")?
        .exists([])?;
    if has_old_column {
        conn.execute_batch("ALTER TABLE commanders RENAME COLUMN scryfall_id TO oracle_id;")?;
    }
    Ok(())
}

fn migrate_add_ending_turn(conn: &Connection) -> rusqlite::Result<()> {
    let has_column: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('games') WHERE name = 'ending_turn'")?
        .exists([])?;
    if !has_column {
        conn.execute_batch("ALTER TABLE games ADD COLUMN ending_turn INTEGER NOT NULL DEFAULT 1;")?;
    }
    Ok(())
}

fn migrate_add_art_framing(conn: &Connection) -> rusqlite::Result<()> {
    let has_column: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('commanders') WHERE name = 'art_zoom'")?
        .exists([])?;
    if !has_column {
        conn.execute_batch(
            "ALTER TABLE commanders ADD COLUMN art_zoom REAL NOT NULL DEFAULT 1.0;
             ALTER TABLE commanders ADD COLUMN art_anchor TEXT NOT NULL DEFAULT 'center';",
        )?;
    }
    Ok(())
}

/// Kill logging grew into general "commander hate" (kills, board wipes,
/// counterspells), so existing rows become plain commander kills.
fn migrate_add_hate_kind(conn: &Connection) -> rusqlite::Result<()> {
    let has_column: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('commander_kills') WHERE name = 'kind'")?
        .exists([])?;
    if !has_column {
        conn.execute_batch(
            "ALTER TABLE commander_kills ADD COLUMN kind TEXT NOT NULL DEFAULT 'commander_kill';",
        )?;
    }
    Ok(())
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

pub fn rename_player(conn: &Connection, id: i64, new_name: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE players SET name = ?1 WHERE id = ?2",
        params![new_name, id],
    )?;
    Ok(())
}

/// How many recorded games a player appears in. Deleting someone with
/// history would orphan those rows, so callers check this first.
pub fn player_game_count(conn: &Connection, id: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM game_players WHERE player_id = ?1",
        params![id],
        |row| row.get(0),
    )
}

pub fn delete_player(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM player_commanders WHERE player_id = ?1",
        params![id],
    )?;
    conn.execute("DELETE FROM players WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn remove_player_commander(
    conn: &Connection,
    player_id: i64,
    commander_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM player_commanders WHERE player_id = ?1 AND commander_id = ?2",
        params![player_id, commander_id],
    )?;
    Ok(())
}

fn commander_from_row(row: &rusqlite::Row) -> rusqlite::Result<Commander> {
    Ok(Commander {
        id: row.get(0)?,
        oracle_id: row.get(1)?,
        name: row.get(2)?,
        image_url: row.get(3)?,
        art_crop_url: row.get(4)?,
        color_identity: row.get(5)?,
        // Framing is per layout and seat, so it isn't carried on the
        // commander row; callers resolve it with `load_framing` once they
        // know which tile the art is going into.
        framing: ArtFraming::default(),
    })
}

const COMMANDER_COLUMNS: &str = "id, oracle_id, name, image_url, art_crop_url, color_identity";

/// The framing this commander's art was last given in this exact tile.
/// Missing rows mean "never framed here", which renders as a plain cover.
pub fn load_framing(
    conn: &Connection,
    commander_id: i64,
    layout_name: &str,
    seat: usize,
) -> ArtFraming {
    conn.query_row(
        "SELECT zoom, pan_x, pan_y FROM art_framing
         WHERE commander_id = ?1 AND layout_name = ?2 AND seat = ?3",
        params![commander_id, layout_name, seat as i64],
        |row| {
            Ok(ArtFraming {
                zoom: row.get::<_, f64>(0)? as f32,
                pan_x: row.get::<_, f64>(1)? as f32,
                pan_y: row.get::<_, f64>(2)? as f32,
            })
        },
    )
    .map(ArtFraming::clamped)
    .unwrap_or_default()
}

pub fn save_framing(
    conn: &Connection,
    commander_id: i64,
    layout_name: &str,
    seat: usize,
    framing: ArtFraming,
) -> rusqlite::Result<()> {
    let framing = framing.clamped();
    conn.execute(
        "INSERT INTO art_framing (commander_id, layout_name, seat, zoom, pan_x, pan_y)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(commander_id, layout_name, seat) DO UPDATE SET
            zoom = excluded.zoom, pan_x = excluded.pan_x, pan_y = excluded.pan_y",
        params![
            commander_id,
            layout_name,
            seat as i64,
            framing.zoom as f64,
            framing.pan_x as f64,
            framing.pan_y as f64
        ],
    )?;
    Ok(())
}

/// The partner saved alongside `commander_id` in this player's list, if
/// they've paired the two as a deck.
pub fn saved_partner(conn: &Connection, player_id: i64, commander_id: i64) -> Option<Commander> {
    let partner_id: i64 = conn
        .query_row(
            "SELECT partner_commander_id FROM player_commanders
             WHERE player_id = ?1 AND commander_id = ?2 AND partner_commander_id IS NOT NULL",
            params![player_id, commander_id],
            |row| row.get(0),
        )
        .ok()?;
    conn.query_row(
        &format!("SELECT {COMMANDER_COLUMNS} FROM commanders WHERE id = ?1"),
        params![partner_id],
        commander_from_row,
    )
    .ok()
}

/// Pairs two of a player's commanders as one deck, in both directions so
/// picking either half brings the other along. `partner` of None unpairs.
pub fn set_player_partner(
    conn: &Connection,
    player_id: i64,
    commander_id: i64,
    partner: Option<i64>,
) -> rusqlite::Result<()> {
    // Clear whatever either commander was previously paired with, so a
    // commander can never end up claimed by two different pairs.
    conn.execute(
        "UPDATE player_commanders SET partner_commander_id = NULL
         WHERE player_id = ?1 AND (commander_id = ?2 OR partner_commander_id = ?2)",
        params![player_id, commander_id],
    )?;
    let Some(partner_id) = partner else {
        return Ok(());
    };
    conn.execute(
        "UPDATE player_commanders SET partner_commander_id = NULL
         WHERE player_id = ?1 AND (commander_id = ?2 OR partner_commander_id = ?2)",
        params![player_id, partner_id],
    )?;
    conn.execute(
        "UPDATE player_commanders SET partner_commander_id = ?3
         WHERE player_id = ?1 AND commander_id = ?2",
        params![player_id, commander_id, partner_id],
    )?;
    conn.execute(
        "UPDATE player_commanders SET partner_commander_id = ?3
         WHERE player_id = ?1 AND commander_id = ?2",
        params![player_id, partner_id, commander_id],
    )?;
    Ok(())
}

/// Records that a player picked a commander during setup, regardless of
/// whether the game that follows is ever finished. This is what backs each
/// player's "quick pick" history - deliberately independent of the
/// games/game_players tables, which only reflect completed games.
pub fn record_player_commander_use(
    conn: &Connection,
    player_id: i64,
    commander_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO player_commanders (player_id, commander_id, last_used_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(player_id, commander_id) DO UPDATE SET last_used_at = excluded.last_used_at",
        params![player_id, commander_id, chrono::Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

/// Commanders this specific player has piloted before, most recently used
/// first. Deliberately scoped per player: one person's "quick picks" aren't
/// shared with the rest of the pod.
pub fn player_commander_history(
    conn: &Connection,
    player_id: i64,
) -> rusqlite::Result<Vec<SavedDeck>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.oracle_id, c.name, c.image_url, c.art_crop_url, c.color_identity,
                pc.partner_commander_id
         FROM commanders c
         JOIN player_commanders pc ON pc.commander_id = c.id
         WHERE pc.player_id = ?1
         ORDER BY pc.last_used_at DESC",
    )?;
    let rows: Vec<(Commander, Option<i64>)> = stmt
        .query_map(params![player_id], |row| {
            Ok((commander_from_row(row)?, row.get::<_, Option<i64>>(6)?))
        })?
        .collect::<rusqlite::Result<_>>()?;

    // A pairing is stored on both halves, so walk the list emitting each
    // pair once - otherwise the same deck shows up twice under two names.
    let mut seen: Vec<i64> = Vec::new();
    let mut decks = Vec::new();
    for (commander, partner_id) in rows {
        if seen.contains(&commander.id) {
            continue;
        }
        seen.push(commander.id);
        let partner = match partner_id {
            Some(id) => {
                seen.push(id);
                conn.query_row(
                    &format!("SELECT {COMMANDER_COLUMNS} FROM commanders WHERE id = ?1"),
                    params![id],
                    commander_from_row,
                )
                .ok()
            }
            None => None,
        };
        decks.push(SavedDeck { commander, partner });
    }
    Ok(decks)
}

/// Inserts a commander cached from Scryfall if we haven't seen its oracle
/// card before; otherwise updates the chosen art/name on the existing row so
/// stats keep pointing at the same commander.
pub fn upsert_commander(
    conn: &Connection,
    oracle_id: &str,
    name: &str,
    image_url: Option<&str>,
    art_crop_url: Option<&str>,
    color_identity: &str,
) -> rusqlite::Result<Commander> {
    conn.execute(
        "INSERT INTO commanders (oracle_id, name, image_url, art_crop_url, color_identity)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(oracle_id) DO UPDATE SET
            name = excluded.name,
            image_url = excluded.image_url,
            art_crop_url = excluded.art_crop_url,
            color_identity = excluded.color_identity",
        params![oracle_id, name, image_url, art_crop_url, color_identity],
    )?;
    conn.query_row(
        &format!("SELECT {COMMANDER_COLUMNS} FROM commanders WHERE oracle_id = ?1"),
        params![oracle_id],
        commander_from_row,
    )
}

pub fn record_game(conn: &mut Connection, game: &FinishedGame) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO games (started_at, ended_at, pod_size, win_reason, ending_turn) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            game.started_at.to_rfc3339(),
            game.ended_at.to_rfc3339(),
            game.seats.len() as i64,
            game.win_reason.map(|r| r.as_db_str()),
            game.ending_turn as i64
        ],
    )?;
    let game_id = tx.last_insert_rowid();

    // Map seat index -> the game_players row id, so commander damage and kills can reference it.
    let mut game_player_ids = Vec::with_capacity(game.seats.len());
    for (seat_index, seat) in game.seats.iter().enumerate() {
        let won = game.winner_seat == Some(seat_index);
        tx.execute(
            "INSERT INTO game_players
                (game_id, player_id, commander_id, partner_commander_id, seat,
                 final_life, final_poison, won, owner_player_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                game_id,
                seat.player.id,
                seat.commander.id,
                seat.partner.as_ref().map(|p| p.id),
                seat_index as i64,
                seat.life,
                seat.poison,
                won as i64,
                seat.borrowed_from.as_ref().map(|owner| owner.id)
            ],
        )?;
        game_player_ids.push(tx.last_insert_rowid());
    }

    for (seat_index, seat) in game.seats.iter().enumerate() {
        for (&(source_index, source_slot), &amount) in seat.commander_damage_taken.iter() {
            if amount <= 0 {
                continue;
            }
            tx.execute(
                "INSERT INTO commander_damage
                    (game_id, target_game_player_id, source_game_player_id, source_slot, amount)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    game_id,
                    game_player_ids[seat_index],
                    game_player_ids[source_index],
                    source_slot as i64,
                    amount
                ],
            )?;
        }
    }

    for (seat_index, seat) in game.seats.iter().enumerate() {
        let Some(out) = seat.elimination else {
            continue;
        };
        tx.execute(
            "INSERT INTO eliminations
                (game_id, victim_game_player_id, killer_game_player_id, cause, turn)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                game_id,
                game_player_ids[seat_index],
                out.killer_seat.map(|i| game_player_ids[i]),
                out.cause.as_db_str(),
                out.turn as i64
            ],
        )?;
    }

    for kill in &game.kills {
        let killer_id = kill.killer_seat.map(|i| game_player_ids[i]);
        tx.execute(
            "INSERT INTO commander_kills
                (game_id, victim_game_player_id, killer_game_player_id, kind)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                game_id,
                game_player_ids[kill.victim_seat],
                killer_id,
                kill.kind.as_db_str()
            ],
        )?;
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

/// Who dishes out the most commander hate, with a breakdown by kind and a
/// per-game rate so someone with one brutal night doesn't outrank a repeat
/// offender.
pub fn hater_stats(conn: &Connection) -> rusqlite::Result<Vec<HaterStat>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT p.name,
               COUNT(*) AS total,
               SUM(CASE WHEN ck.kind = 'commander_kill' THEN 1 ELSE 0 END) AS kills,
               SUM(CASE WHEN ck.kind = 'board_wipe' THEN 1 ELSE 0 END) AS wipes,
               SUM(CASE WHEN ck.kind = 'counterspell' THEN 1 ELSE 0 END) AS counters,
               (SELECT COUNT(*) FROM game_players gp WHERE gp.player_id = p.id) AS games
        FROM commander_kills ck
        JOIN game_players kp ON kp.id = ck.killer_game_player_id
        JOIN players p ON p.id = kp.player_id
        GROUP BY p.id, p.name
        ORDER BY total DESC, p.name
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(HaterStat {
            player_name: row.get(0)?,
            total: row.get(1)?,
            kills: row.get(2)?,
            wipes: row.get(3)?,
            counters: row.get(4)?,
            games: row.get(5)?,
        })
    })?;
    rows.collect()
}

/// Which commanders draw the most heat.
pub fn hated_commander_stats(conn: &Connection) -> rusqlite::Result<Vec<HatedCommanderStat>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT c.name,
               COUNT(*) AS total,
               SUM(CASE WHEN ck.kind = 'commander_kill' THEN 1 ELSE 0 END) AS kills,
               SUM(CASE WHEN ck.kind = 'board_wipe' THEN 1 ELSE 0 END) AS wipes,
               SUM(CASE WHEN ck.kind = 'counterspell' THEN 1 ELSE 0 END) AS counters,
               (SELECT COUNT(*) FROM game_players gp WHERE gp.commander_id = c.id) AS appearances
        FROM commander_kills ck
        JOIN game_players vp ON vp.id = ck.victim_game_player_id
        JOIN commanders c ON c.id = vp.commander_id
        GROUP BY c.id, c.name
        ORDER BY total DESC, c.name
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(HatedCommanderStat {
            commander_name: row.get(0)?,
            total: row.get(1)?,
            kills: row.get(2)?,
            wipes: row.get(3)?,
            counters: row.get(4)?,
            appearances: row.get(5)?,
        })
    })?;
    rows.collect()
}

/// Specific grudges: this player keeps targeting this commander.
pub fn grudge_stats(conn: &Connection) -> rusqlite::Result<Vec<GrudgeStat>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT hater.name, c.name, victim.name, COUNT(*) AS total
        FROM commander_kills ck
        JOIN game_players kp ON kp.id = ck.killer_game_player_id
        JOIN players hater ON hater.id = kp.player_id
        JOIN game_players vp ON vp.id = ck.victim_game_player_id
        JOIN players victim ON victim.id = vp.player_id
        JOIN commanders c ON c.id = vp.commander_id
        GROUP BY hater.id, c.id, victim.id
        ORDER BY total DESC, hater.name
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(GrudgeStat {
            hater_name: row.get(0)?,
            commander_name: row.get(1)?,
            victim_name: row.get(2)?,
            total: row.get(3)?,
        })
    })?;
    rows.collect()
}

/// How games in this pod actually end.
pub fn win_reason_stats(conn: &Connection) -> rusqlite::Result<Vec<WinReasonStat>> {
    let mut stmt = conn.prepare(
        "SELECT win_reason, COUNT(*) FROM games
         WHERE win_reason IS NOT NULL
         GROUP BY win_reason ORDER BY COUNT(*) DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let reason: String = row.get(0)?;
        Ok(WinReasonStat {
            reason: WinReason::from_db_str(&reason),
            games: row.get(1)?,
        })
    })?;
    rows.collect()
}

/// Average number of turns a finished game runs to.
pub fn average_game_turns(conn: &Connection) -> rusqlite::Result<Option<f64>> {
    conn.query_row("SELECT AVG(ending_turn) FROM games", [], |row| row.get(0))
}

fn parse_dt(s: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now())
}

pub fn list_games(conn: &Connection) -> rusqlite::Result<Vec<GameSummary>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT g.id, g.started_at, g.ended_at, g.pod_size, g.win_reason, p.name, c.name, g.ending_turn
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
            ending_turn: row.get(7)?,
        })
    })?;
    rows.collect()
}

pub fn game_detail(conn: &Connection, game_id: i64) -> rusqlite::Result<GameDetail> {
    let (started_at, ended_at, win_reason, ending_turn): (String, String, Option<String>, i64) =
        conn.query_row(
            "SELECT started_at, ended_at, win_reason, ending_turn FROM games WHERE id = ?1",
            params![game_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;

    struct SeatRow {
        id: i64,
        player_id: i64,
        commander_id: i64,
        partner_commander_id: Option<i64>,
        final_life: i32,
        final_poison: i32,
        won: bool,
        owner_player_id: Option<i64>,
    }

    let seat_rows: Vec<SeatRow> = {
        let mut stmt = conn.prepare(
            "SELECT id, player_id, commander_id, partner_commander_id,
                    final_life, final_poison, won, owner_player_id
             FROM game_players WHERE game_id = ?1 ORDER BY seat",
        )?;
        let result = stmt
            .query_map(params![game_id], |row| {
                Ok(SeatRow {
                    id: row.get(0)?,
                    player_id: row.get(1)?,
                    commander_id: row.get(2)?,
                    partner_commander_id: row.get(3)?,
                    final_life: row.get(4)?,
                    final_poison: row.get(5)?,
                    won: row.get::<_, i64>(6)? != 0,
                    owner_player_id: row.get(7)?,
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
        let mut commander_name: String = conn.query_row(
            "SELECT name FROM commanders WHERE id = ?1",
            params![r.commander_id],
            |row| row.get(0),
        )?;
        if let Some(partner_id) = r.partner_commander_id {
            let partner: String = conn.query_row(
                "SELECT name FROM commanders WHERE id = ?1",
                params![partner_id],
                |row| row.get(0),
            )?;
            commander_name = format!("{commander_name} + {partner}");
        }

        let damage_taken: Vec<(String, i32)> = {
            let mut stmt = conn.prepare(
                "SELECT CASE WHEN cd.source_slot = 1 THEN COALESCE(pc.name, c.name)
                             ELSE c.name END,
                        cd.amount
                 FROM commander_damage cd
                 JOIN game_players sp ON sp.id = cd.source_game_player_id
                 JOIN commanders c ON c.id = sp.commander_id
                 LEFT JOIN commanders pc ON pc.id = sp.partner_commander_id
                 WHERE cd.target_game_player_id = ?1",
            )?;
            let result = stmt
                .query_map(params![r.id], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            result
        };

        let out = conn
            .query_row(
                "SELECT e.cause, e.turn, p.name
                 FROM eliminations e
                 LEFT JOIN game_players kp ON kp.id = e.killer_game_player_id
                 LEFT JOIN players p ON p.id = kp.player_id
                 WHERE e.victim_game_player_id = ?1",
                params![r.id],
                |row| {
                    Ok(GameDetailOut {
                        cause: OutCause::from_db_str(&row.get::<_, String>(0)?),
                        turn: row.get::<_, i64>(1)? as u32,
                        killer_name: row.get(2)?,
                    })
                },
            )
            .optional()?;

        // Only a borrowed deck names an owner; playing your own says nothing.
        let borrowed_from: Option<String> = match r.owner_player_id {
            Some(owner_id) if owner_id != r.player_id => conn
                .query_row(
                    "SELECT name FROM players WHERE id = ?1",
                    params![owner_id],
                    |row| row.get(0),
                )
                .ok(),
            _ => None,
        };

        seats.push(GameDetailSeat {
            player_name,
            commander_name,
            borrowed_from,
            final_life: r.final_life,
            final_poison: r.final_poison,
            won: r.won,
            damage_taken,
            out,
        });
    }

    let kills: Vec<GameDetailKill> = {
        let mut stmt = conn.prepare(
            "SELECT vp2.name, kp2.name, ck.kind
             FROM commander_kills ck
             JOIN game_players vp ON vp.id = ck.victim_game_player_id
             JOIN players vp2 ON vp2.id = vp.player_id
             LEFT JOIN game_players kp ON kp.id = ck.killer_game_player_id
             LEFT JOIN players kp2 ON kp2.id = kp.player_id
             WHERE ck.game_id = ?1",
        )?;
        let result = stmt
            .query_map(params![game_id], |row| {
                let kind: String = row.get(2)?;
                Ok(GameDetailKill {
                    victim: row.get(0)?,
                    killer: row.get(1)?,
                    kind: HateKind::from_db_str(&kind),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        result
    };

    Ok(GameDetail {
        id: game_id,
        started_at: parse_dt(&started_at),
        ended_at: parse_dt(&ended_at),
        win_reason: win_reason.map(|s| WinReason::from_db_str(&s)),
        ending_turn,
        seats,
        kills,
    })
}

// ---------------------------------------------------------------------------
// Moxfield links and salt analyses
// ---------------------------------------------------------------------------

/// The headline numbers for a linked deck, without paying to deserialise the
/// whole breakdown. This is what the deck tiles and the deck bar read.
#[derive(Debug, Clone, PartialEq)]
pub struct DeckLink {
    pub public_id: String,
    pub url: String,
    pub deck_name: String,
    /// `None` until an analysis has landed, which is also how a link that
    /// was saved while offline is told apart from one that failed.
    pub bracket: Option<u8>,
    pub salt_total: Option<f64>,
}

fn deck_link_from_row(row: &rusqlite::Row) -> rusqlite::Result<(i64, DeckLink)> {
    Ok((
        row.get("commander_id")?,
        DeckLink {
            public_id: row.get("public_id")?,
            url: row.get("url")?,
            deck_name: row.get("deck_name")?,
            bracket: row.get::<_, Option<i64>>("bracket")?.map(|b| b as u8),
            salt_total: row.get("salt_total")?,
        },
    ))
}

/// Points one of a player's decks at a Moxfield list.
///
/// Any previous analysis is dropped on the way through: if the link changed,
/// last week's numbers describe a different deck, and showing them next to a
/// new link would be worse than showing nothing.
pub fn set_deck_link(
    conn: &Connection,
    player_id: i64,
    commander_id: i64,
    public_id: &str,
    url: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO deck_analysis (player_id, commander_id, public_id, url)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(player_id, commander_id) DO UPDATE SET
            public_id   = excluded.public_id,
            url         = excluded.url,
            deck_name   = CASE WHEN deck_analysis.public_id = excluded.public_id
                               THEN deck_analysis.deck_name ELSE '' END,
            bracket     = CASE WHEN deck_analysis.public_id = excluded.public_id
                               THEN deck_analysis.bracket ELSE NULL END,
            salt_total  = CASE WHEN deck_analysis.public_id = excluded.public_id
                               THEN deck_analysis.salt_total ELSE NULL END,
            breakdown   = CASE WHEN deck_analysis.public_id = excluded.public_id
                               THEN deck_analysis.breakdown ELSE NULL END,
            analysed_at = CASE WHEN deck_analysis.public_id = excluded.public_id
                               THEN deck_analysis.analysed_at ELSE NULL END",
        params![player_id, commander_id, public_id, url],
    )?;
    Ok(())
}

pub fn remove_deck_link(
    conn: &Connection,
    player_id: i64,
    commander_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM deck_analysis WHERE player_id = ?1 AND commander_id = ?2",
        params![player_id, commander_id],
    )?;
    Ok(())
}

/// Stores a finished analysis against a deck. The link row is created if it
/// somehow isn't there, so a saved analysis can't be orphaned by one.
pub fn save_deck_analysis(
    conn: &Connection,
    player_id: i64,
    commander_id: i64,
    public_id: &str,
    analysis: &crate::salt::Analysis,
) -> rusqlite::Result<()> {
    let breakdown = serde_json::to_string(analysis).unwrap_or_default();
    conn.execute(
        "INSERT INTO deck_analysis
            (player_id, commander_id, public_id, url, deck_name, bracket, salt_total,
             breakdown, analysed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(player_id, commander_id) DO UPDATE SET
            public_id   = excluded.public_id,
            url         = excluded.url,
            deck_name   = excluded.deck_name,
            bracket     = excluded.bracket,
            salt_total  = excluded.salt_total,
            breakdown   = excluded.breakdown,
            analysed_at = excluded.analysed_at",
        params![
            player_id,
            commander_id,
            public_id,
            analysis.url,
            analysis.deck_name,
            analysis.bracket as i64,
            analysis.salt_total,
            breakdown,
            analysis.analysed_at,
        ],
    )?;
    Ok(())
}

/// Every linked deck for one player, keyed by the deck's primary commander.
pub fn deck_links(
    conn: &Connection,
    player_id: i64,
) -> rusqlite::Result<std::collections::HashMap<i64, DeckLink>> {
    let mut stmt = conn.prepare(
        "SELECT commander_id, public_id, url, deck_name, bracket, salt_total
         FROM deck_analysis WHERE player_id = ?1",
    )?;
    let rows = stmt.query_map(params![player_id], deck_link_from_row)?;
    rows.collect()
}

/// The full stored breakdown for one deck.
///
/// A row whose `breakdown` no longer parses is treated as absent rather than
/// as an error: the only way that happens is `salt::Analysis` changing shape
/// under an old row, and the honest response is to offer a re-analysis, not
/// to fail opening the screen.
pub fn deck_breakdown(
    conn: &Connection,
    player_id: i64,
    commander_id: i64,
) -> Option<crate::salt::Analysis> {
    let json: String = conn
        .query_row(
            "SELECT breakdown FROM deck_analysis
             WHERE player_id = ?1 AND commander_id = ?2 AND breakdown IS NOT NULL",
            params![player_id, commander_id],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()?;
    serde_json::from_str(&json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::salt::Analysis;

    /// A schema-complete throwaway database, so these tests exercise the real
    /// migrations rather than a hand-built subset of them.
    fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        init(&conn).unwrap();
        conn
    }

    /// Creates a player with one saved commander, returning both ids.
    fn player_with_deck(conn: &Connection, name: &str) -> (i64, i64) {
        let player = create_player(conn, name).unwrap();
        let commander = upsert_commander(
            conn,
            &format!("oracle-{name}"),
            "Kibo, Uktabi Prince",
            None,
            None,
            "GR",
        )
        .unwrap();
        record_player_commander_use(conn, player.id, commander.id).unwrap();
        (player.id, commander.id)
    }

    fn analysis(public_id: &str, bracket: u8, salt: f64) -> Analysis {
        Analysis {
            deck_name: "Test Deck".into(),
            public_id: public_id.into(),
            url: format!("https://moxfield.com/decks/{public_id}"),
            card_count: 100,
            bracket,
            criteria: Vec::new(),
            owner_bracket: Some(2),
            auto_bracket: Some(3),
            salt_total: salt,
            categories: Vec::new(),
            combos: Vec::new(),
            unscored: Vec::new(),
            analysed_at: "2026-09-24T12:00:00+00:00".into(),
        }
    }

    #[test]
    fn a_link_and_its_analysis_round_trip() {
        let conn = memory_db();
        let (player, commander) = player_with_deck(&conn, "Dante");

        assert!(deck_links(&conn, player).unwrap().is_empty());
        assert!(deck_breakdown(&conn, player, commander).is_none());

        set_deck_link(
            &conn,
            player,
            commander,
            "abc12345",
            "https://moxfield.com/decks/abc12345",
        )
        .unwrap();

        // A link on its own is a real state: saved, not yet scored.
        let links = deck_links(&conn, player).unwrap();
        let link = &links[&commander];
        assert_eq!(link.public_id, "abc12345");
        assert_eq!(link.bracket, None, "nothing has been worked out yet");
        assert!(deck_breakdown(&conn, player, commander).is_none());

        save_deck_analysis(
            &conn,
            player,
            commander,
            "abc12345",
            &analysis("abc12345", 4, 198.2),
        )
        .unwrap();

        let links = deck_links(&conn, player).unwrap();
        let link = &links[&commander];
        assert_eq!(link.bracket, Some(4));
        assert_eq!(link.salt_total, Some(198.2));
        assert_eq!(link.deck_name, "Test Deck");

        // The whole breakdown comes back, not just the headline numbers.
        let stored = deck_breakdown(&conn, player, commander).unwrap();
        assert_eq!(stored.bracket, 4);
        assert_eq!(stored.public_id, "abc12345");
        assert_eq!(stored.card_count, 100);
    }

    #[test]
    fn relinking_a_different_deck_drops_the_old_numbers() {
        let conn = memory_db();
        let (player, commander) = player_with_deck(&conn, "Dante");

        set_deck_link(&conn, player, commander, "abc12345", "u1").unwrap();
        save_deck_analysis(
            &conn,
            player,
            commander,
            "abc12345",
            &analysis("abc12345", 4, 198.2),
        )
        .unwrap();

        // Re-saving the SAME link must keep the analysis - that's what makes
        // an idle re-check cheap and keeps numbers on screen meanwhile.
        set_deck_link(&conn, player, commander, "abc12345", "u1").unwrap();
        assert_eq!(
            deck_links(&conn, player).unwrap()[&commander].bracket,
            Some(4)
        );
        assert!(deck_breakdown(&conn, player, commander).is_some());

        // Pointing at a DIFFERENT deck must not leave last deck's bracket
        // sitting under the new link.
        set_deck_link(&conn, player, commander, "zzz99999", "u2").unwrap();
        let link = &deck_links(&conn, player).unwrap()[&commander];
        assert_eq!(link.public_id, "zzz99999");
        assert_eq!(link.bracket, None, "stale bracket must be cleared");
        assert_eq!(link.salt_total, None);
        assert!(
            deck_breakdown(&conn, player, commander).is_none(),
            "stale breakdown must be cleared"
        );
    }

    #[test]
    fn links_are_per_player_not_per_commander() {
        let conn = memory_db();
        // Two people playing the same commander are two different decks, and
        // must not share one link.
        let player_a = create_player(&conn, "Dante").unwrap();
        let player_b = create_player(&conn, "Sam").unwrap();
        let commander =
            upsert_commander(&conn, "oracle-shared", "Atraxa", None, None, "WBGU").unwrap();
        record_player_commander_use(&conn, player_a.id, commander.id).unwrap();
        record_player_commander_use(&conn, player_b.id, commander.id).unwrap();

        set_deck_link(&conn, player_a.id, commander.id, "aaaaaaaa", "ua").unwrap();
        set_deck_link(&conn, player_b.id, commander.id, "bbbbbbbb", "ub").unwrap();

        assert_eq!(
            deck_links(&conn, player_a.id).unwrap()[&commander.id].public_id,
            "aaaaaaaa"
        );
        assert_eq!(
            deck_links(&conn, player_b.id).unwrap()[&commander.id].public_id,
            "bbbbbbbb"
        );
    }

    #[test]
    fn unlinking_forgets_everything() {
        let conn = memory_db();
        let (player, commander) = player_with_deck(&conn, "Dante");

        set_deck_link(&conn, player, commander, "abc12345", "u1").unwrap();
        save_deck_analysis(
            &conn,
            player,
            commander,
            "abc12345",
            &analysis("abc12345", 3, 40.0),
        )
        .unwrap();

        remove_deck_link(&conn, player, commander).unwrap();
        assert!(deck_links(&conn, player).unwrap().is_empty());
        assert!(deck_breakdown(&conn, player, commander).is_none());
    }

    #[test]
    fn a_breakdown_that_no_longer_parses_reads_as_absent() {
        let conn = memory_db();
        let (player, commander) = player_with_deck(&conn, "Dante");
        set_deck_link(&conn, player, commander, "abc12345", "u1").unwrap();
        // Simulates an analysis written by an older version of the struct.
        conn.execute(
            "UPDATE deck_analysis SET breakdown = '{\"nonsense\":true}'
             WHERE player_id = ?1 AND commander_id = ?2",
            params![player, commander],
        )
        .unwrap();

        // Absent, rather than an error that stops the page opening: the
        // screen can then offer a re-check.
        assert!(deck_breakdown(&conn, player, commander).is_none());
    }
}
