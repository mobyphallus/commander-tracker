use crate::{app::Message, db, style};
use iced::{
    widget::{column, container, text},
    Element, Length, Task,
};
use rusqlite::{backup::Backup, Connection, DatabaseName, OpenFlags};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct StorageState {
    pub status: Option<String>,
    pub error: Option<String>,
    candidate: Option<PathBuf>,
    choosing: bool,
}
#[derive(Debug, Clone)]
pub enum StorageMessage {
    Backup,
    ChooseRestore,
    Chosen(Result<Option<PathBuf>, String>),
    ConfirmRestore,
    CancelRestore,
}

pub fn backup(conn: &Connection, directory: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let path = directory.join(format!(
        "commander-pod-{}.db",
        chrono::Utc::now().format("%Y%m%d-%H%M%S-%f")
    ));
    conn.backup(DatabaseName::Main, &path, None)
        .map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn restore(conn: &mut Connection, source: &Path, directory: &Path) -> Result<PathBuf, String> {
    // Validate and migrate a staging copy; never migrate the user's backup in place.
    let original = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let check: String = original
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if check != "ok" {
        return Err("This backup did not pass the database integrity check.".into());
    }
    for query in [
        "SELECT id,name FROM players",
        "SELECT id,oracle_id,name FROM commanders",
        "SELECT id,started_at,ended_at,pod_size FROM games",
        "SELECT game_id,player_id,commander_id,seat,won FROM game_players",
    ] {
        original
            .prepare(query)
            .map_err(|_| "This file is not a Commander Pod backup.".to_string())?;
    }
    let mut staged = Connection::open_in_memory().map_err(|e| e.to_string())?;
    Backup::new(&original, &mut staged)
        .and_then(|b| b.run_to_completion(100, std::time::Duration::from_millis(10), None))
        .map_err(|e| e.to_string())?;
    db::init(&staged).map_err(|e| e.to_string())?;
    let violations: i64 = staged
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    if violations != 0 {
        return Err("This backup contains broken references and was not restored.".into());
    }
    db::list_players(&staged).map_err(|e| e.to_string())?;
    db::list_games(&staged).map_err(|e| e.to_string())?;
    crate::session::load(&staged)?;
    crate::screens::stats::data::Data::load(&staged).map_err(|e| e.to_string())?;
    let safety = backup(conn, directory)?;
    Backup::new(&staged, conn)
        .and_then(|b| b.run_to_completion(100, std::time::Duration::from_millis(10), None))
        .map_err(|e| {
            format!(
                "Restore failed: {e}. Your safety copy is at {}",
                safety.display()
            )
        })?;
    Ok(safety)
}

pub fn update(
    state: &mut StorageState,
    conn: &mut Connection,
    msg: StorageMessage,
) -> Task<Message> {
    match msg {
        StorageMessage::Backup => {
            state.error = None;
            state.status = None;
            match backup(conn, &db::data_dir().join("backups")) {
                Ok(path) => state.status = Some(format!("Backup saved: {}", path.display())),
                Err(e) => state.error = Some(format!("Couldn’t create backup: {e}")),
            }
        }
        StorageMessage::ChooseRestore => {
            state.error = None;
            state.status = None;
            state.choosing = true;
            return Task::perform(
                async {
                    tokio::task::spawn_blocking(|| {
                        let output = std::process::Command::new("zenity")
                            .args([
                                "--file-selection",
                                "--title=Choose a Commander Pod backup",
                                "--file-filter=SQLite backups | *.db",
                            ])
                            .output()
                            .map_err(|e| format!("Couldn’t open the file picker: {e}"))?;
                        if output.status.success() {
                            Ok(Some(PathBuf::from(
                                String::from_utf8_lossy(&output.stdout).trim(),
                            )))
                        } else if output.status.code() == Some(1) {
                            Ok(None)
                        } else {
                            Err("The file picker could not open. Please try again.".into())
                        }
                    })
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|v| v)
                },
                |r| Message::Storage(StorageMessage::Chosen(r)),
            );
        }
        StorageMessage::Chosen(result) => {
            state.choosing = false;
            match result {
                Ok(p) => state.candidate = p,
                Err(e) => state.error = Some(e),
            }
        }
        StorageMessage::CancelRestore => state.candidate = None,
        StorageMessage::ConfirmRestore => {
            if let Some(path) = state.candidate.take() {
                state.error = None;
                match restore(conn, &path, &db::data_dir().join("backups")) {
                    Ok(safety) => {
                        state.status = Some(format!(
                            "Backup restored. Your previous data is preserved at {}",
                            safety.display()
                        ))
                    }
                    Err(e) => state.error = Some(e),
                }
            }
        }
    }
    Task::none()
}

pub fn view(state: &StorageState) -> Element<'_, Message> {
    let mut body = column![
        style::touch_button("Back", style::T_LABEL)
            .style(style::ghost)
            .on_press(Message::GoHome),
        text("Backup & restore").size(style::T_TITLE),
        text("Keep your players, pictures, decks, history, and current game together.")
            .size(style::T_BODY),
        style::touch_button("Create backup", style::T_ACTION)
            .style(style::primary)
            .on_press(Message::Storage(StorageMessage::Backup)),
    ]
    .spacing(style::GAP);
    let mut choose = style::touch_button(
        if state.choosing {
            "Choosing backup…"
        } else {
            "Choose backup to restore"
        },
        style::T_ACTION,
    )
    .style(style::secondary);
    if !state.choosing {
        choose = choose.on_press(Message::Storage(StorageMessage::ChooseRestore));
    }
    body = body.push(choose);
    if let Some(path) = &state.candidate {
        body = body.push(container(column![text("Replace current data with this backup?").size(style::T_SUBHEAD), text(path.display().to_string()), text("A safety backup of your current data will be created first. Restoring replaces players, decks, history, and any unfinished game."),
            style::touch_button("Cancel", style::T_ACTION).on_press(Message::Storage(StorageMessage::CancelRestore)),
            style::touch_button("Restore this backup", style::T_ACTION).style(style::danger).on_press(Message::Storage(StorageMessage::ConfirmRestore))].spacing(style::GAP)).padding(24).style(style::panel));
    }
    if let Some(s) = &state.status {
        body = body.push(text(s).color(style::ACCENT_BRIGHT));
    }
    if let Some(e) = &state.error {
        body = body.push(text(e).color(style::DANGER));
    }
    container(iced::widget::scrollable(body))
        .padding(32)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restore_preserves_photos_and_recovery_and_backs_up_previous_data() {
        let (source, game) = crate::session::tests::fixture();
        crate::session::save(&source, &game).unwrap();
        db::set_player_picture(&source, game.seats[0].player.id, b"photo").unwrap();
        let directory = std::env::temp_dir().join(format!(
            "pod-backup-test-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let source_path = backup(&source, &directory).unwrap();
        let mut target = Connection::open_in_memory().unwrap();
        db::init(&target).unwrap();
        db::create_player(&target, "Previous data").unwrap();
        let safety = restore(&mut target, &source_path, &directory).unwrap();
        assert_eq!(db::list_players(&target).unwrap().len(), 3);
        assert_eq!(db::player_pictures(&target).unwrap()[0].1, b"photo");
        assert!(crate::session::load(&target).unwrap().is_some());
        let previous =
            Connection::open_with_flags(safety, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(
            db::list_players(&previous).unwrap()[0].name,
            "Previous data"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn unrelated_database_is_rejected_without_changing_current_data() {
        let (mut target, _) = crate::session::tests::fixture();
        let directory = std::env::temp_dir().join(format!(
            "pod-invalid-backup-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let bad = directory.join("unrelated.db");
        Connection::open(&bad)
            .unwrap()
            .execute_batch("CREATE TABLE unrelated(id INTEGER);")
            .unwrap();
        assert!(restore(&mut target, &bad, &directory).is_err());
        assert_eq!(db::list_players(&target).unwrap().len(), 3);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
