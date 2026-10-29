//! `drivebridge sync`: one sync run, reported for a person at a terminal.

use std::process::ExitCode;

use anyhow::{Context, Result};
use chrono::{Local, Utc};
use clap::Args;
use drivebridge_core::{IgnoreRules, SafetyPolicy, SyncAction};
use drivebridge_engine::{trash, Options, Outcome, SyncConfig};
use drivebridge_store::Store;

use crate::config::{self, Paths, DRIVE_FOLDER, LAST_SYNC, LOCAL_ROOT, PAUSED, REMOTE};
use crate::secrets;

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

    let config = SyncConfig {
        rclone: config::rclone(),
        env: secrets::load()?.env(REMOTE),
        local_root,
        remote_root: format!("{REMOTE}:{folder}"),
        workdir: paths.bisync.clone(),
        trash: paths.trash.clone(),
        ignore: IgnoreRules::new(),
        policy: SafetyPolicy::default(),
    };
    let options = Options {
        dry_run: args.dry_run,
        force: args.force,
    };
    let outcome = drivebridge_engine::sync(&config, &mut store, options, Local::now())?;
    let code = report(&outcome);

    if matches!(outcome, Outcome::UpToDate | Outcome::Synced { .. }) {
        store.set_setting(LAST_SYNC, &Utc::now().timestamp().to_string())?;
        let now = Local::now().naive_local();
        let removed = trash::prune(&paths.trash, now, config::TRASH_DAYS)?;
        if removed > 0 {
            let days = config::TRASH_DAYS;
            println!("Emptied {removed} trash folder(s) older than {days} days.");
        }
    }
    Ok(code)
}

fn report(outcome: &Outcome) -> ExitCode {
    match outcome {
        Outcome::Preview { actions, violation } => {
            println!("Plan: {}", summarize(actions));
            for action in actions.iter().filter(|a| !matches!(a, SyncAction::Skip(_))) {
                println!("  {:<13} {}", action.kind(), action.path().display());
            }
            if let Some(v) = violation {
                println!("A real sync would be blocked: {v}.");
                println!("If this is what you want, run `drivebridge sync --force`.");
            }
            ExitCode::SUCCESS
        }
        Outcome::Blocked { violation, actions } => {
            println!("Plan: {}", summarize(actions));
            eprintln!("Sync blocked: {violation}. Nothing was changed.");
            eprintln!("Review it with `drivebridge sync --dry-run`.");
            eprintln!("If it is what you want, run `drivebridge sync --force`.");
            ExitCode::from(BLOCKED)
        }
        Outcome::UpToDate => {
            println!("Everything is up to date.");
            ExitCode::SUCCESS
        }
        Outcome::Synced {
            actions,
            overridden,
        } => {
            if let Some(v) = overridden {
                eprintln!("Safety limit overridden with --force: {v}.");
            }
            println!("Synced: {}", summarize(actions));
            ExitCode::SUCCESS
        }
    }
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
