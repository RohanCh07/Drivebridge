//! Talking to drivebridged over D-Bus. The names match `crates/daemon/src/bus.rs`.

use zbus::blocking::Connection;

const NAME: &str = "io.github.Drivebridge";
const PATH: &str = "/io/github/Drivebridge";
const INTERFACE: &str = "io.github.Drivebridge1";

pub struct DaemonStatus {
    /// "starting", "idle", "syncing", "paused", "blocked", "offline" or "error".
    pub state: String,
    pub message: String,
    /// Unix seconds; 0 if never.
    pub last_sync: i64,
    pub tracked: u32,
}

pub fn connect() -> zbus::Result<Connection> {
    Connection::session()
}

pub fn status(conn: &Connection) -> zbus::Result<DaemonStatus> {
    let reply = conn.call_method(Some(NAME), PATH, Some(INTERFACE), "Status", &())?;
    let (state, message, last_sync, tracked): (String, String, i64, u32) =
        reply.body().deserialize()?;
    Ok(DaemonStatus {
        state,
        message,
        last_sync,
        tracked,
    })
}

/// Calls a method without arguments, such as `SyncNow`, `Pause` or `Resume`.
pub fn call(conn: &Connection, method: &str) -> zbus::Result<()> {
    conn.call_method(Some(NAME), PATH, Some(INTERFACE), method, &())?;
    Ok(())
}
