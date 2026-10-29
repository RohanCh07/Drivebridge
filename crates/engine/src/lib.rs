//! One sync run, shared by the CLI and the daemon: list both sides, plan, check the safety
//! rules, let rclone bisync carry out the plan, and remember the result.

pub mod trash;

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use drivebridge_core::{
    plan, FileMeta, IgnoreRules, SafetyPolicy, SafetyViolation, Snapshot, SyncAction,
};
use drivebridge_rclone::bisync::Bisync;
use drivebridge_rclone::{ListItem, RcClient, Rcd};
use drivebridge_store::Store;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not list {path}")]
    LocalUnreadable {
        path: String,
        source: drivebridge_rclone::Error,
    },
    #[error("could not list your Drive folder; treating Drive as offline, nothing was changed")]
    Offline(#[source] drivebridge_rclone::Error),
    #[error(transparent)]
    Rclone(#[from] drivebridge_rclone::Error),
    #[error(transparent)]
    Store(#[from] drivebridge_store::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// What to sync and how.
pub struct SyncConfig {
    pub rclone: PathBuf,
    /// Environment for rclone, such as the in-memory Drive remote's credentials.
    pub env: Vec<(String, String)>,
    /// The local folder.
    pub local_root: String,
    /// The Drive folder as an rclone path, e.g. `drivebridge:Drivebridge`.
    pub remote_root: String,
    /// rclone bisync's own state.
    pub workdir: PathBuf,
    /// Local files a run deletes or overwrites go to `trash/<run time>/`.
    pub trash: PathBuf,
    pub ignore: IgnoreRules,
    pub policy: SafetyPolicy,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Plan only; change nothing.
    pub dry_run: bool,
    /// Carry out a plan even if it breaks the safety policy.
    pub force: bool,
}

#[derive(Debug)]
pub enum Outcome {
    /// Dry run: what a real run would do, and whether it would be blocked.
    Preview {
        actions: Vec<SyncAction>,
        violation: Option<SafetyViolation>,
    },
    /// The plan broke a safety rule, so nothing was changed.
    Blocked {
        violation: SafetyViolation,
        actions: Vec<SyncAction>,
    },
    UpToDate,
    Synced {
        actions: Vec<SyncAction>,
        /// Set when `force` let a plan through that the policy would have blocked.
        overridden: Option<SafetyViolation>,
    },
}

/// Runs one sync. `now` names the trash folder and timestamps the history.
pub fn sync(
    config: &SyncConfig,
    store: &mut Store,
    options: Options,
    now: DateTime<Local>,
) -> Result<Outcome> {
    let rcd = Rcd::start(&config.rclone, &config.env)?;
    let (local, remote) = list_both(rcd.client(), config)?;
    let base = store.base()?;
    let actions = plan(&local, &remote, &base, &config.ignore);
    let violation = config.policy.check_plan(&actions, base.len()).err();

    if options.dry_run {
        return Ok(Outcome::Preview { actions, violation });
    }
    if let Some(v) = &violation {
        if !options.force {
            return Ok(Outcome::Blocked {
                violation: v.clone(),
                actions,
            });
        }
    }

    let has_work = actions.iter().any(|a| !matches!(a, SyncAction::Skip(_)));
    let listed_before = has_listings(&config.workdir);
    if !has_work && (listed_before || (local.is_empty() && remote.is_empty())) {
        store.replace_base(&agreed(&local, &remote))?;
        return Ok(Outcome::UpToDate);
    }

    // bisync aborts on its own when every file on a side changed, even if that is one file.
    // Our policy has already vetted this plan; keep bisync's check only for big plans.
    let changes = actions.iter().filter(|a| !matches!(a, SyncAction::Skip(_)));
    let small_plan = changes.count() <= config.policy.max_delete_count;

    let stamp = now.format(trash::STAMP_FORMAT).to_string();
    let run = Bisync {
        path1: config.local_root.clone(),
        path2: config.remote_root.clone(),
        workdir: config.workdir.clone(),
        backup_dir1: Some(config.trash.join(stamp)),
        // bisync needs a resync when it has no listings of its own, even if we have a base.
        resync: base.is_empty() || !listed_before,
        force: options.force || small_plan,
        excludes: config.ignore.rclone_excludes(),
    };
    run.run(&config.rclone, &config.env)?;

    let (local, remote) = list_both(rcd.client(), config)?;
    store.replace_base(&agreed(&local, &remote))?;
    store.record(now.timestamp(), &actions)?;
    Ok(Outcome::Synced {
        actions,
        overridden: violation,
    })
}

/// Lists both sides. A failed Drive listing stops the run: offline must never look like
/// "everything was deleted".
fn list_both(client: &RcClient, config: &SyncConfig) -> Result<(Snapshot, Snapshot)> {
    let local = &config.local_root;
    let local_items = client
        .list_recursive(local)
        .map_err(|source| Error::LocalUnreadable {
            path: local.clone(),
            source,
        })?;
    let remote_items = client
        .list_recursive(&config.remote_root)
        .map_err(Error::Offline)?;
    Ok((snapshot(local_items), snapshot(remote_items)))
}

/// True once bisync has run here and kept listings it can compare against.
fn has_listings(workdir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(workdir) else {
        return false;
    };
    entries
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().ends_with(".path1.lst"))
}

fn snapshot(items: Vec<ListItem>) -> Snapshot {
    items
        .into_iter()
        // Google Docs have no size; rclone skips them (--drive-skip-gdocs) and so do we.
        .filter(|item| !item.is_dir && item.size >= 0)
        .map(|item| {
            let mtime = DateTime::parse_from_rfc3339(&item.mod_time)
                .map(|t| t.timestamp())
                .unwrap_or(0);
            let meta = FileMeta {
                size: item.size as u64,
                mtime,
                hash: None,
            };
            (PathBuf::from(item.path), meta)
        })
        .collect()
}

/// Files that are the same on both sides: the starting point for the next run.
fn agreed(local: &Snapshot, remote: &Snapshot) -> Snapshot {
    local
        .iter()
        .filter(|(path, meta)| remote.get(*path).is_some_and(|r| r.same_content(meta)))
        .map(|(path, meta)| (path.clone(), meta.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str, size: i64, mod_time: &str, is_dir: bool) -> ListItem {
        ListItem {
            path: path.into(),
            name: path.rsplit('/').next().unwrap().into(),
            size,
            mime_type: String::new(),
            mod_time: mod_time.into(),
            is_dir,
            id: None,
        }
    }

    #[test]
    fn snapshot_keeps_files_with_whole_second_mtimes() {
        let items = vec![
            item("docs/a.txt", 5, "2026-10-20T09:15:00.250Z", false),
            item("docs", -1, "2026-10-20T09:15:00Z", true),
            item("Budget", -1, "2026-10-20T09:15:00Z", false),
        ];
        let snap = snapshot(items);
        assert_eq!(snap.len(), 1);
        let meta = &snap[&PathBuf::from("docs/a.txt")];
        assert_eq!(meta.size, 5);
        assert_eq!(meta.mtime, 1_792_487_700);
    }

    #[test]
    fn local_and_drive_timestamps_compare_equal() {
        let local_time = "2026-10-20T14:45:00.123456789+05:30";
        let local = snapshot(vec![item("a", 1, local_time, false)]);
        let drive = snapshot(vec![item("a", 1, "2026-10-20T09:15:00.123Z", false)]);
        assert_eq!(agreed(&local, &drive).len(), 1);
    }

    #[test]
    fn agreed_drops_files_that_differ() {
        let meta = |size| FileMeta {
            size,
            mtime: 1,
            hash: None,
        };
        let local = Snapshot::from([("same".into(), meta(1)), ("diff".into(), meta(1))]);
        let remote = Snapshot::from([("same".into(), meta(1)), ("diff".into(), meta(2))]);
        let base = agreed(&local, &remote);
        let paths: Vec<_> = base.keys().collect();
        assert_eq!(paths, vec![&PathBuf::from("same")]);
    }

    #[test]
    fn listings_are_detected_by_name() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_listings(dir.path()));
        assert!(!has_listings(&dir.path().join("missing")));
        fs::write(dir.path().join("local..drive.path1.lst"), "").unwrap();
        assert!(has_listings(dir.path()));
    }
}
