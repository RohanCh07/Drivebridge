//! drivebridge-gui: the desktop app. A setup wizard on first run, then a status window with
//! controls for the background service and a "Recently removed" list.

mod daemon;
mod setup;
mod status;

use adw::prelude::*;
use drivebridge_engine::settings::{self, Paths};
use drivebridge_store::Store;
use gtk::glib;

const APP_ID: &str = "io.github.Drivebridge.App";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(|app| {
        if let Some(window) = app.active_window() {
            window.present();
        } else if is_set_up() {
            status::show(app);
        } else {
            setup::show(app);
        }
    });
    app.run()
}

fn is_set_up() -> bool {
    let Ok(paths) = Paths::new() else {
        return false;
    };
    Store::open(&paths.db)
        .ok()
        .and_then(|store| settings::is_set_up(&store).ok())
        .unwrap_or(false)
}
