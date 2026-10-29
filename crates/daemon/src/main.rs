//! drivebridged: the background sync service, run as a systemd user service.
//!
//! Syncs at startup, a few seconds after local changes settle, and every few minutes to pick
//! up changes made on Drive. Failures back off; a blocked plan waits for the user. Logs go to
//! stderr, which systemd sends to the journal.

mod schedule;

use std::path::Path;
use std::process::ExitCode;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Instant;

use anyhow::{bail, Result};
use chrono::{Local, Utc};
use drivebridge_core::SyncAction;
use drivebridge_engine::settings::{self, Paths, LAST_SYNC};
use drivebridge_engine::{trash, Options, Outcome, SyncConfig};
use drivebridge_store::Store;

use crate::schedule::{Schedule, Scheduler};

/// sysexits' EX_CONFIG. The unit file tells systemd not to restart on it.
const NOT_SET_UP: u8 = 78;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let paths = Paths::new()?;
    let mut store = Store::open(&paths.db)?;
    if !settings::is_set_up(&store)? {
        eprintln!("not set up yet; run `drivebridge setup`, then start drivebridged again");
        return Ok(ExitCode::from(NOT_SET_UP));
    }
    let config = settings::sync_config(&paths, &store)?;
    let root = Path::new(&config.local_root);
    let (_watcher, changes) = drivebridge_watcher::watch(root, config.ignore.clone())?;
    eprintln!("watching {} <-> {}", root.display(), config.remote_root);

    let mut scheduler = Scheduler::new(Schedule::default(), Instant::now());
    loop {
        let wait = scheduler
            .next_wake()
            .saturating_duration_since(Instant::now());
        match changes.recv_timeout(wait) {
            Ok(()) => scheduler.local_change(Instant::now()),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => bail!("the file watcher stopped"),
        }
        if !scheduler.due(Instant::now()) {
            continue;
        }
        let ok = settings::is_paused(&store)? || sync_once(&config, &mut store, &paths);
        scheduler.finished(Instant::now(), ok);
    }
}

/// One logged sync. Returns false when it failed and should be retried with backoff.
fn sync_once(config: &SyncConfig, store: &mut Store, paths: &Paths) -> bool {
    match drivebridge_engine::sync(config, store, Options::default(), Local::now()) {
        Ok(Outcome::Synced { actions, .. }) => {
            let changes = actions
                .iter()
                .filter(|a| !matches!(a, SyncAction::Skip(_)))
                .count();
            eprintln!("synced {changes} change(s)");
        }
        Ok(Outcome::UpToDate | Outcome::Preview { .. }) => {}
        Ok(Outcome::Blocked { violation, .. }) => {
            // Waiting on the user, not a failure: retrying sooner would not help.
            eprintln!("sync blocked: {violation}; nothing was changed");
            eprintln!("review it with `drivebridge sync --dry-run`");
            return true;
        }
        Err(e) => {
            eprintln!("sync failed: {:#}", anyhow::Error::from(e));
            return false;
        }
    }
    let now = Utc::now().timestamp().to_string();
    if let Err(e) = store.set_setting(LAST_SYNC, &now) {
        eprintln!("could not record the sync time: {e}");
    }
    let today = Local::now().naive_local();
    if let Err(e) = trash::prune(&paths.trash, today, settings::TRASH_DAYS) {
        eprintln!("could not empty old trash: {e}");
    }
    true
}
