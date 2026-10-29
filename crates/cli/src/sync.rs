//! `drivebridge sync`: plan, check, then let rclone bisync carry it out.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use clap::Args;
use drivebridge_core::{check_plan, plan, FileMeta, IgnoreRules, Snapshot, SyncAction};
use drivebridge_rclone::bisync::Bisync;
use drivebridge_rclone::{ListItem, RcClient, Rcd};
use drivebridge_store::Store;

use crate::config::{self, Paths, DRIVE_FOLDER, LAST_SYNC, LOCAL_ROOT, PAUSED, REMOTE};
use crate::{secrets, trash};

/// Exit code when the safety check stops a sync.
const BLOCKED: u8 = 2;

#[derive(Args)]
pub struct SyncArgs {
    /// Show what would change, without changing anything
    #[arg(long)]
    dry_run: bool,
    /// Sync even if the plan deletes more than the safety limit
    #[arg(long)]
    force: bool,
}

pub fn run(args: SyncArgs) -> Result<ExitCode> {
    let paths = Paths::new()?;
    let mut store = Store::open(&paths.db)?;
    if store.setting(PAUSED)?.as_deref() == Some("true") {
        println!("Syncing is paused. Run `drivebridge resume` to continue.");
        return Ok(ExitCode::SUCCESS);
    }
    let not_set_up = "not set up yet; run `drivebridge setup` first";
    let local_root = store.setting(LOCAL_ROOT)?.context(not_set_up)?;
    let folder = store.setting(DRIVE_FOLDER)?.context(not_set_up)?;
    let remote_root = format!("{REMOTE}:{folder}");

    let env = secrets::load()?.env(REMOTE);
    let rclone = config::rclone();
    let ignore = IgnoreRules::new();
    let rcd = Rcd::start(&rclone, &env)?;

    let (local, remote) = list_both(rcd.client(), &local_root, &remote_root)?;
    let base = store.base()?;
    let first_sync = base.is_empty();
    let actions = plan(&local, &remote, &base, &ignore);
    println!("Plan: {}", summarize(&actions));

    if let Err(violation) = check_plan(&actions, base.len()) {
        if !args.force {
            eprintln!("Sync blocked: {violation}. Nothing was changed.");
            eprintln!("Review it with `drivebridge sync --dry-run`.");
            eprintln!("If it is what you want, run `drivebridge sync --force`.");
            return Ok(ExitCode::from(BLOCKED));
        }
        eprintln!("Safety limit overridden with --force: {violation}.");
    }

    if args.dry_run {
        for action in actions.iter().filter(|a| !matches!(a, SyncAction::Skip(_))) {
            println!("  {:<13} {}", action.kind(), action.path().display());
        }
        return Ok(ExitCode::SUCCESS);
    }

    let now = Utc::now().timestamp();
    let has_work = actions.iter().any(|a| !matches!(a, SyncAction::Skip(_)));
    if has_work {
        let stamp = Local::now().format(trash::STAMP_FORMAT).to_string();
        let run = Bisync {
            path1: local_root.clone(),
            path2: remote_root.clone(),
            workdir: paths.bisync.clone(),
            backup_dir1: Some(paths.trash.join(stamp)),
            resync: first_sync,
            force: args.force,
            excludes: ignore.rclone_excludes(),
        };
        run.run(&rclone, &env)?;
        let (local, remote) = list_both(rcd.client(), &local_root, &remote_root)?;
        store.replace_base(&agreed(&local, &remote))?;
        store.record(now, &actions)?;
    }
    store.set_setting(LAST_SYNC, &now.to_string())?;

    let removed = trash::prune(&paths.trash, Local::now().naive_local(), config::TRASH_DAYS)?;
    if removed > 0 {
        println!(
            "Emptied {removed} trash folder(s) older than {} days.",
            config::TRASH_DAYS
        );
    }
    println!(
        "{}",
        if has_work {
            "Sync complete."
        } else {
            "Everything is up to date."
        }
    );
    Ok(ExitCode::SUCCESS)
}

/// Lists both sides. A failed Drive listing stops the sync: offline must never look like
/// "everything was deleted".
fn list_both(client: &RcClient, local: &str, remote: &str) -> Result<(Snapshot, Snapshot)> {
    let local_items = client
        .list_recursive(local)
        .with_context(|| format!("could not list {local}"))?;
    let remote_items = client
        .list_recursive(remote)
        .context("could not list your Drive folder; treating Drive as offline, nothing changed")?;
    Ok((snapshot(local_items), snapshot(remote_items)))
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

/// Files that are the same on both sides: the starting point for the next sync.
fn agreed(local: &Snapshot, remote: &Snapshot) -> Snapshot {
    local
        .iter()
        .filter(|(path, meta)| remote.get(*path).is_some_and(|r| r.same_content(meta)))
        .map(|(path, meta)| (path.clone(), meta.clone()))
        .collect()
}

fn summarize(actions: &[SyncAction]) -> String {
    let kinds = [
        "upload",
        "download",
        "delete-local",
        "delete-remote",
        "conflict",
    ];
    let parts: Vec<String> = kinds
        .iter()
        .map(|kind| (kind, actions.iter().filter(|a| a.kind() == *kind).count()))
        .filter(|(_, count)| *count > 0)
        .map(|(kind, count)| format!("{count} {kind}"))
        .collect();
    if parts.is_empty() {
        "nothing to change".to_string()
    } else {
        parts.join(", ")
    }
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
        let local = snapshot(vec![item(
            "a",
            1,
            "2026-10-20T14:45:00.123456789+05:30",
            false,
        )]);
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
        assert_eq!(
            base.keys().collect::<Vec<_>>(),
            vec![&PathBuf::from("same")]
        );
    }

    #[test]
    fn summary_counts_changes_only() {
        let actions = [
            SyncAction::Upload("a".into()),
            SyncAction::Upload("b".into()),
            SyncAction::Skip("c".into()),
            SyncAction::DeleteRemote("d".into()),
        ];
        assert_eq!(summarize(&actions), "2 upload, 1 delete-remote");
        assert_eq!(summarize(&[]), "nothing to change");
    }
}
