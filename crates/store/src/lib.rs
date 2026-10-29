//! Local state in SQLite: settings, the snapshot from the last sync, and sync history.

use std::path::{Path, PathBuf};

use drivebridge_core::{FileMeta, Snapshot, SyncAction};
use rusqlite::{params, Connection, OptionalExtension};

pub use rusqlite::Error;
pub type Result<T> = rusqlite::Result<T>;

const SCHEMA_VERSION: i64 = 1;
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS files (
    path  TEXT PRIMARY KEY,
    size  INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    hash  TEXT
);
CREATE TABLE IF NOT EXISTS history (
    id     INTEGER PRIMARY KEY,
    at     INTEGER NOT NULL,
    action TEXT NOT NULL,
    path   TEXT NOT NULL
);
";

/// One recorded sync action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// Seconds since the Unix epoch.
    pub at: i64,
    pub action: String,
    pub path: String,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < SCHEMA_VERSION {
            conn.execute_batch(SCHEMA)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(Self { conn })
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        let sql = "SELECT value FROM settings WHERE key = ?1";
        self.conn.query_row(sql, [key], |r| r.get(0)).optional()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let sql = "INSERT INTO settings (key, value) VALUES (?1, ?2)
                   ON CONFLICT(key) DO UPDATE SET value = excluded.value";
        self.conn.execute(sql, [key, value])?;
        Ok(())
    }

    /// The files both sides agreed on after the last sync.
    pub fn base(&self) -> Result<Snapshot> {
        let mut stmt = self.conn.prepare("SELECT path, size, mtime, hash FROM files")?;
        let rows = stmt.query_map([], |r| {
            let path: String = r.get(0)?;
            let size: i64 = r.get(1)?;
            let meta = FileMeta {
                size: size as u64,
                mtime: r.get(2)?,
                hash: r.get(3)?,
            };
            Ok((PathBuf::from(path), meta))
        })?;
        let snapshot = rows.collect::<Result<Snapshot>>()?;
        Ok(snapshot)
    }

    /// Replaces the last-sync snapshot in one transaction.
    pub fn replace_base(&mut self, snapshot: &Snapshot) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM files", [])?;
        {
            let sql = "INSERT INTO files (path, size, mtime, hash) VALUES (?1, ?2, ?3, ?4)";
            let mut insert = tx.prepare(sql)?;
            for (path, meta) in snapshot {
                let path = path.to_string_lossy().into_owned();
                insert.execute(params![path, meta.size as i64, meta.mtime, meta.hash])?;
            }
        }
        tx.commit()
    }

    /// Records every action except skips, all at time `at`.
    pub fn record(&mut self, at: i64, actions: &[SyncAction]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let sql = "INSERT INTO history (at, action, path) VALUES (?1, ?2, ?3)";
            let mut insert = tx.prepare(sql)?;
            for action in actions {
                if matches!(action, SyncAction::Skip(_)) {
                    continue;
                }
                let path = action.path().to_string_lossy().into_owned();
                insert.execute(params![at, action.kind(), path])?;
            }
        }
        tx.commit()
    }

    /// The most recent history entries, newest first.
    pub fn recent_history(&self, limit: usize) -> Result<Vec<HistoryEntry>> {
        let sql = "SELECT at, action, path FROM history ORDER BY id DESC LIMIT ?1";
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([limit as i64], |r| {
            Ok(HistoryEntry {
                at: r.get(0)?,
                action: r.get(1)?,
                path: r.get(2)?,
            })
        })?;
        let entries = rows.collect::<Result<Vec<_>>>()?;
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(size: u64, mtime: i64, hash: Option<&str>) -> FileMeta {
        FileMeta {
            size,
            mtime,
            hash: hash.map(str::to_string),
        }
    }

    #[test]
    fn settings_round_trip() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.setting("local_root").unwrap(), None);
        store.set_setting("local_root", "/home/me/Drive").unwrap();
        store.set_setting("local_root", "/home/me/GoogleDrive").unwrap();
        let value = store.setting("local_root").unwrap();
        assert_eq!(value.as_deref(), Some("/home/me/GoogleDrive"));
    }

    #[test]
    fn base_is_replaced_not_merged() {
        let mut store = Store::open_in_memory().unwrap();
        let first = Snapshot::from([
            ("a.txt".into(), meta(1, 10, None)),
            ("b.txt".into(), meta(2, 20, Some("abc"))),
        ]);
        store.replace_base(&first).unwrap();
        assert_eq!(store.base().unwrap(), first);

        let second = Snapshot::from([("c.txt".into(), meta(3, 30, None))]);
        store.replace_base(&second).unwrap();
        assert_eq!(store.base().unwrap(), second);
    }

    #[test]
    fn history_skips_skips_and_is_newest_first() {
        let mut store = Store::open_in_memory().unwrap();
        let first = [
            SyncAction::Upload("a.txt".into()),
            SyncAction::Skip("b.txt".into()),
        ];
        store.record(100, &first).unwrap();
        store.record(200, &[SyncAction::DeleteLocal("c.txt".into())]).unwrap();

        let history = store.recent_history(10).unwrap();
        let summary: Vec<_> = history
            .iter()
            .map(|h| (h.at, h.action.as_str(), h.path.as_str()))
            .collect();
        assert_eq!(
            summary,
            vec![(200, "delete-local", "c.txt"), (100, "upload", "a.txt")]
        );
        assert_eq!(store.recent_history(1).unwrap().len(), 1);
    }

    #[test]
    fn data_survives_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        {
            let mut store = Store::open(&path).unwrap();
            store.set_setting("drive_folder", "Drivebridge").unwrap();
            let base = Snapshot::from([("a.txt".into(), meta(1, 10, None))]);
            store.replace_base(&base).unwrap();
        }
        let store = Store::open(&path).unwrap();
        let folder = store.setting("drive_folder").unwrap();
        assert_eq!(folder.as_deref(), Some("Drivebridge"));
        assert_eq!(store.base().unwrap().len(), 1);
    }
}
