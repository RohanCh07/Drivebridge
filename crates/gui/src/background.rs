//! Inside a Flatpak there is no systemd user unit, so the app starts the background service
//! itself and asks the Background portal to start it at every login.

use std::collections::HashMap;
use std::process::Command;

use zbus::blocking::Connection;
use zbus::zvariant::Value;

pub fn in_flatpak() -> bool {
    std::env::var_os("FLATPAK_ID").is_some()
}

/// Starts drivebridged if it isn't answering, and asks to be started at login.
pub fn ensure_service(conn: &Connection) {
    if crate::daemon::status(conn).is_ok() {
        return;
    }
    if let Err(e) = Command::new("drivebridged").spawn() {
        eprintln!("could not start drivebridged: {e}");
    }
    if let Err(e) = request_autostart(conn) {
        eprintln!("could not ask to start at login: {e}");
    }
}

fn request_autostart(conn: &Connection) -> zbus::Result<()> {
    let mut options: HashMap<&str, Value> = HashMap::new();
    let reason = "Keep your Drive folder in sync after the window is closed";
    options.insert("reason", Value::from(reason));
    options.insert("autostart", Value::from(true));
    options.insert("commandline", Value::from(vec!["drivebridged"]));
    conn.call_method(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        Some("org.freedesktop.portal.Background"),
        "RequestBackground",
        &("", options),
    )?;
    Ok(())
}
