//! On-disk cache for what we pull from Scryfall.
//!
//! Scryfall asks that applications cache downloaded data for at least 24
//! hours instead of re-fetching it. Art bytes are keyed by their URL, and
//! Scryfall's image URLs are per-printing and immutable, so a hit can never
//! go stale - we keep those indefinitely, which is also what makes the app
//! work at a table with no wifi. Printing lists are gameplay data that only
//! changes on a reprint, so they expire after `PRINTS_TTL`.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::db;
use crate::scryfall::ScryfallCard;

const PRINTS_TTL: Duration = Duration::from_secs(24 * 60 * 60);

fn subdir(name: &str) -> PathBuf {
    let dir = db::data_dir().join(name);
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// FNV-1a, so a URL maps to a stable filename. `DefaultHasher` would do the
/// job but isn't guaranteed stable across Rust releases, which would
/// silently orphan the whole cache after a toolchain upgrade.
fn key(s: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn art(url: &str) -> Option<Vec<u8>> {
    let bytes = std::fs::read(subdir("art").join(key(url))).ok()?;
    // A truncated write from a previous crash would decode as a broken
    // image; treat anything empty as a miss and re-download.
    (!bytes.is_empty()).then_some(bytes)
}

pub fn store_art(url: &str, bytes: &[u8]) {
    let _ = std::fs::write(subdir("art").join(key(url)), bytes);
}

pub fn prints(oracle_id: &str) -> Option<Vec<ScryfallCard>> {
    let path = subdir("prints").join(format!("{}.json", key(oracle_id)));
    let age = SystemTime::now()
        .duration_since(std::fs::metadata(&path).ok()?.modified().ok()?)
        .ok()?;
    if age > PRINTS_TTL {
        return None;
    }
    serde_json::from_slice(&std::fs::read(&path).ok()?).ok()
}

pub fn store_prints(oracle_id: &str, cards: &[ScryfallCard]) {
    if let Ok(json) = serde_json::to_vec(cards) {
        let _ = std::fs::write(subdir("prints").join(format!("{}.json", key(oracle_id))), json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uses the real cache directory with a URL nothing else can collide
    /// with, then cleans up after itself.
    #[test]
    fn art_round_trips() {
        let url = "https://example.invalid/commander_pod-test-art";
        let path = subdir("art").join(key(url));
        let _ = std::fs::remove_file(&path);

        assert_eq!(art(url), None, "should start as a miss");
        store_art(url, b"some jpeg bytes");
        assert_eq!(art(url).as_deref(), Some(&b"some jpeg bytes"[..]));

        // A truncated write from a crash must read back as a miss so the
        // bytes get re-fetched rather than decoded as a broken image.
        store_art(url, b"");
        assert_eq!(art(url), None);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn prints_round_trip_and_expire() {
        let oracle = "commander_pod-test-oracle";
        let path = subdir("prints").join(format!("{}.json", key(oracle)));
        let _ = std::fs::remove_file(&path);

        assert!(prints(oracle).is_none());
        store_prints(oracle, &[]);
        assert_eq!(prints(oracle).map(|p| p.len()), Some(0));

        // Backdate past the TTL: a stale list must miss so we re-ask.
        let stale = std::time::SystemTime::now() - PRINTS_TTL - Duration::from_secs(60);
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(stale).unwrap();
        assert!(prints(oracle).is_none(), "expired list should be a miss");

        let _ = std::fs::remove_file(&path);
    }
}
