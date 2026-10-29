//! The daemon's D-Bus interface on the session bus, used by the desktop app.
//!
//! Name `io.github.Drivebridge`, object `/io/github/Drivebridge`, interface
//! `io.github.Drivebridge1`. Methods: `Status`, `SyncNow`, `Pause`, `Resume`,
//! `RecentActivity`. Signals: `StatusChanged`, `Blocked`, `Conflicts`.

use std::fmt::Display;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use drivebridge_engine::settings::{Paths, LAST_SYNC, PAUSED};
use drivebridge_store::Store;
use zbus::blocking::Connection;
use zbus::{fdo, interface, SignalContext};

use crate::report::{Event, State, Status};

pub const NAME: &str = "io.github.Drivebridge";
pub const PATH: &str = "/io/github/Drivebridge";
pub const INTERFACE: &str = "io.github.Drivebridge1";

/// Why the main loop should wake up.
#[derive(Debug, PartialEq, Eq)]
pub enum Wake {
    LocalChange,
    SyncNow,
}

/// The daemon's current status, shared with the D-Bus interface.
pub type SharedStatus = Arc<Mutex<Status>>;

struct Daemon {
    status: SharedStatus,
    wake: Sender<Wake>,
}

fn failed(error: impl Display) -> fdo::Error {
    fdo::Error::Failed(error.to_string())
}

fn open_store() -> fdo::Result<Store> {
    let paths = Paths::new().map_err(|e| failed(format!("{e:#}")))?;
    Store::open(&paths.db).map_err(failed)
}

#[interface(name = "io.github.Drivebridge1")]
impl Daemon {
    /// The state ("starting", "idle", "syncing", "paused", "blocked", "offline" or "error"),
    /// a message for people, the last successful sync in Unix seconds (0 if never), and how
    /// many files are in sync.
    fn status(&self) -> fdo::Result<(String, String, i64, u32)> {
        let status = self.status.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let store = open_store()?;
        let last_sync = store
            .setting(LAST_SYNC)
            .map_err(failed)?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let tracked = store.base().map_err(failed)?.len();
        let tracked = u32::try_from(tracked).unwrap_or(u32::MAX);
        Ok((status.state.as_str().into(), status.message, last_sync, tracked))
    }

    fn sync_now(&self) -> fdo::Result<()> {
        self.wake.send(Wake::SyncNow).map_err(failed)
    }

    fn pause(&self) -> fdo::Result<()> {
        open_store()?.set_setting(PAUSED, "true").map_err(failed)?;
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        *status = Status::new(State::Paused, "Paused");
        Ok(())
    }

    fn resume(&self) -> fdo::Result<()> {
        open_store()?.set_setting(PAUSED, "false").map_err(failed)?;
        self.wake.send(Wake::SyncNow).map_err(failed)
    }

    /// Up to `limit` recent changes, newest first: (Unix seconds, action, path).
    fn recent_activity(&self, limit: u32) -> fdo::Result<Vec<(i64, String, String)>> {
        let history = open_store()?
            .recent_history(limit as usize)
            .map_err(failed)?;
        Ok(history
            .into_iter()
            .map(|entry| (entry.at, entry.action, entry.path))
            .collect())
    }

    #[zbus(signal)]
    async fn status_changed(
        ctxt: &SignalContext<'_>,
        state: &str,
        message: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn blocked(ctxt: &SignalContext<'_>, message: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn conflicts(ctxt: &SignalContext<'_>, paths: Vec<String>) -> zbus::Result<()>;
}

/// Our connection to the session bus. Dropping it releases the name.
pub struct Bus {
    conn: Connection,
}

impl Bus {
    pub fn connect(status: SharedStatus, wake: Sender<Wake>) -> zbus::Result<Self> {
        let daemon = Daemon { status, wake };
        let conn = zbus::blocking::connection::Builder::session()?
            .name(NAME)?
            .serve_at(PATH, daemon)?
            .build()?;
        Ok(Bus { conn })
    }

    fn context(&self) -> zbus::Result<SignalContext<'static>> {
        SignalContext::new(self.conn.inner(), PATH)
    }

    pub fn status_changed(&self, status: &Status) -> zbus::Result<()> {
        let ctxt = self.context()?;
        let signal = Daemon::status_changed(&ctxt, status.state.as_str(), &status.message);
        zbus::block_on(signal)
    }

    pub fn event(&self, event: &Event) -> zbus::Result<()> {
        let ctxt = self.context()?;
        match event {
            Event::Blocked(message) => zbus::block_on(Daemon::blocked(&ctxt, message)),
            Event::Conflicts(paths) => zbus::block_on(Daemon::conflicts(&ctxt, paths.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    #[ignore = "needs a session bus: dbus-run-session -- cargo test -- --include-ignored"]
    fn serves_status_and_sync_now() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("DRIVEBRIDGE_DATA_DIR", dir.path());
        let status = Arc::new(Mutex::new(Status::new(State::Idle, "Up to date")));
        let (wake, woken) = mpsc::channel();
        let _bus = Bus::connect(status, wake).unwrap();

        let client = Connection::session().unwrap();
        let call = |method: &str| {
            client
                .call_method(Some(NAME), PATH, Some(INTERFACE), method, &())
                .unwrap()
        };
        let reply = call("Status");
        let body: (String, String, i64, u32) = reply.body().deserialize().unwrap();
        let expected = ("idle".to_string(), "Up to date".to_string(), 0, 0);
        assert_eq!(body, expected);

        call("SyncNow");
        let wake = woken.recv_timeout(Duration::from_secs(5));
        assert_eq!(wake, Ok(Wake::SyncNow));
    }
}
