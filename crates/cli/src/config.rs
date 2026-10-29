//! Where Drivebridge keeps its files, and the names of its settings.

use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

/// The rclone remote name, defined in memory from the keyring credentials.
pub const REMOTE: &str = "drivebridge";

pub const LOCAL_ROOT: &str = "local_root";
pub const DRIVE_FOLDER: &str = "drive_folder";
pub const PAUSED: &str = "paused";
pub const LAST_SYNC: &str = "last_sync";

/// Files deleted or overwritten locally stay in the trash this long.
pub const TRASH_DAYS: i64 = 30;

pub struct Paths {
    pub db: PathBuf,
    /// One folder per sync run, named by its start time.
    pub trash: PathBuf,
    /// rclone bisync's own state.
    pub bisync: PathBuf,
}

impl Paths {
    /// `$DRIVEBRIDGE_DATA_DIR`, else `$XDG_DATA_HOME/drivebridge`, else
    /// `~/.local/share/drivebridge`. Created if missing.
    pub fn new() -> Result<Self> {
        let data = match env::var_os("DRIVEBRIDGE_DATA_DIR") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => match env::var_os("XDG_DATA_HOME") {
                Some(dir) if !dir.is_empty() => PathBuf::from(dir).join("drivebridge"),
                _ => home()?.join(".local/share/drivebridge"),
            },
        };
        let paths = Paths {
            db: data.join("state.db"),
            trash: data.join("trash"),
            bisync: data.join("bisync"),
        };
        for dir in [&data, &paths.trash, &paths.bisync] {
            fs::create_dir_all(dir)
                .with_context(|| format!("could not create {}", dir.display()))?;
        }
        Ok(paths)
    }
}

pub fn home() -> Result<PathBuf> {
    env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .context("HOME is not set")
}

/// `$DRIVEBRIDGE_RCLONE`, else `rclone` from PATH.
pub fn rclone() -> PathBuf {
    env::var_os("DRIVEBRIDGE_RCLONE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rclone"))
}
