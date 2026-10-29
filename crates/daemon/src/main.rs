//! drivebridged: the background sync service, run as a systemd user service.
//!
//! Syncs at startup, a few seconds after local changes settle, every few minutes to pick up
//! changes made on Drive, and when the desktop app asks over D-Bus. Failures back off; a
//! blocked plan waits for the user. Logs go to stderr, which systemd sends to the journal.

mod bus;
mod report;
mod schedule;

use std::path::Path;
use std::process::ExitCode;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use anyhow::Result;
use chrono::{Local, Utc};
use drivebridge_engine::settings::{self, Paths, LAST_SYNC};
use drivebridge_engine::{trash, Options, SyncConfig};
use drivebridge_store::Store;

use crate::bus::{Bus, SharedStatus, Wake};
use crate::report::{State, Status};
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

    let (wake, woken) = mpsc::channel();
    let (_watcher, changes) = drivebridge_watcher::watch(root, config.ignore.clone())?;
    let forward = wake.clone();
    thread::spawn(move || {
        for () in changes {
            if forward.send(Wake::LocalChange).is_err() {
                break;
            }
        }
    });

    let status = Arc::new(Mutex::new(Status::new(State::Starting, "Starting")));
    let bus = match Bus::connect(status.clone(), wake.clone()) {
        Ok(bus) => Some(bus),
        Err(e) => {
            eprintln!("no D-Bus session ({e}); the desktop app won't see this service");
            None
        }
    };
    eprintln!("watching {} <-> {}", root.display(), config.remote_root);

    let mut scheduler = Scheduler::new(Schedule::default(), Instant::now());
    loop {
        let wait = scheduler
            .next_wake()
            .saturating_duration_since(Instant::now());
        // `wake` lives here, so the channel never disconnects.
        match woken.recv_timeout(wait) {
            Ok(Wake::LocalChange) => scheduler.local_change(Instant::now()),
            Ok(Wake::SyncNow) => scheduler.sync_now(Instant::now()),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
        }
        if !scheduler.due(Instant::now()) {
            continue;
        }
        if settings::is_paused(&store)? {
            set_status(&status, bus.as_ref(), Status::new(State::Paused, "Paused"));
            scheduler.finished(Instant::now(), true);
            continue;
        }
        set_status(&status, bus.as_ref(), Status::new(State::Syncing, "Syncing"));
        let ok = sync_once(&config, &mut store, &paths, &status, bus.as_ref());
        scheduler.finished(Instant::now(), ok);
    }
}

/// One sync, logged and published. Returns false when it should be retried with backoff.
fn sync_once(
    config: &SyncConfig,
    store: &mut Store,
    paths: &Paths,
    status: &SharedStatus,
    bus: Option<&Bus>,
) -> bool {
    let was_blocked = current(status).state == State::Blocked;
    let result = drivebridge_engine::sync(config, store, Options::default(), Local::now());
    let report = report::report(&result);
    eprintln!("{}", report.status.message);

    if report.ok && report.status.state == State::Idle {
        let now = Utc::now().timestamp().to_string();
        if let Err(e) = store.set_setting(LAST_SYNC, &now) {
            eprintln!("could not record the sync time: {e}");
        }
        let today = Local::now().naive_local();
        if let Err(e) = trash::prune(&paths.trash, today, settings::TRASH_DAYS) {
            eprintln!("could not empty old trash: {e}");
        }
    }
    // A blocked plan is re-checked every poll; tell the desktop only the first time.
    let still_blocked = was_blocked && report.status.state == State::Blocked;
    if let Some(bus) = bus.filter(|_| !still_blocked) {
        for event in &report.events {
            if let Err(e) = bus.event(event) {
                eprintln!("could not signal the desktop: {e}");
            }
        }
    }
    set_status(status, bus, report.status);
    report.ok
}

fn current(status: &SharedStatus) -> Status {
    status.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Updates the shared status and signals the desktop if it changed.
fn set_status(status: &SharedStatus, bus: Option<&Bus>, new: Status) {
    let mut shared = status.lock().unwrap_or_else(|e| e.into_inner());
    if *shared == new {
        return;
    }
    *shared = new.clone();
    drop(shared);
    if let Some(bus) = bus {
        if let Err(e) = bus.status_changed(&new) {
            eprintln!("could not signal the desktop: {e}");
        }
    }
}
