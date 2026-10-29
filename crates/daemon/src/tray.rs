//! The tray icon (StatusNotifierItem, via ksni). Desktops without a tray, such as stock
//! GNOME, simply don't show it; everything it offers is also in the app.

use std::process::Command;
use std::sync::mpsc::Sender;

use drivebridge_engine::settings::{Paths, PAUSED};
use drivebridge_store::Store;
use ksni::menu::StandardItem;
use ksni::{MenuItem, ToolTip};

use crate::bus::Wake;
use crate::report::{State, Status};

pub type TrayHandle = ksni::Handle<Tray>;

pub struct Tray {
    pub status: Status,
    wake: Sender<Wake>,
}

/// Starts the tray icon on its own thread. `None` when `DRIVEBRIDGE_NO_TRAY` is set.
pub fn spawn(wake: Sender<Wake>) -> Option<TrayHandle> {
    if std::env::var_os("DRIVEBRIDGE_NO_TRAY").is_some() {
        return None;
    }
    let tray = Tray {
        status: Status::new(State::Starting, "Starting"),
        wake,
    };
    let service = ksni::TrayService::new(tray);
    let handle = service.handle();
    service.spawn();
    Some(handle)
}

/// The icon for each state, from the freedesktop icon naming spec.
fn icon(state: State) -> &'static str {
    match state {
        State::Starting | State::Syncing => "emblem-synchronizing",
        State::Idle => "folder-remote",
        State::Paused => "media-playback-pause",
        State::Blocked | State::Failed => "dialog-warning",
        State::Offline => "network-offline",
    }
}

fn open_app() {
    if let Err(e) = Command::new("drivebridge-gui").spawn() {
        eprintln!("could not open drivebridge-gui: {e}");
    }
}

impl Tray {
    fn set_paused(&mut self, paused: bool) {
        let saved = Paths::new()
            .map_err(|e| format!("{e:#}"))
            .and_then(|paths| Store::open(&paths.db).map_err(|e| e.to_string()))
            .and_then(|store| {
                let value = if paused { "true" } else { "false" };
                store.set_setting(PAUSED, value).map_err(|e| e.to_string())
            });
        match saved {
            // The main loop picks the setting up and publishes the new status.
            Ok(()) => {
                let _ = self.wake.send(Wake::SyncNow);
            }
            Err(e) => eprintln!("could not change the paused setting: {e}"),
        }
    }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "drivebridge".into()
    }

    fn title(&self) -> String {
        "Drivebridge".into()
    }

    fn icon_name(&self) -> String {
        icon(self.status.state).into()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "Drivebridge".into(),
            description: self.status.message.clone(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        open_app();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let paused = self.status.state == State::Paused;
        let toggle = if paused { "Resume" } else { "Pause" };
        vec![
            StandardItem {
                label: self.status.message.clone(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Sync now".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.wake.send(Wake::SyncNow);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: toggle.into(),
                activate: Box::new(move |tray: &mut Self| tray.set_paused(!paused)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Open Drivebridge".into(),
                activate: Box::new(|_: &mut Self| open_app()),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray as _;
    use std::sync::mpsc;

    fn tray(state: State) -> Tray {
        let (wake, _) = mpsc::channel();
        Tray {
            status: Status::new(state, "Up to date"),
            wake,
        }
    }

    fn labels(tray: &Tray) -> Vec<String> {
        tray.menu()
            .into_iter()
            .filter_map(|item| match item {
                MenuItem::Standard(item) => Some(item.label),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn menu_offers_pause_or_resume() {
        let idle = labels(&tray(State::Idle));
        assert_eq!(idle, ["Up to date", "Sync now", "Pause", "Open Drivebridge"]);
        assert!(labels(&tray(State::Paused)).contains(&"Resume".to_string()));
    }

    #[test]
    fn problems_show_a_warning_icon() {
        assert_eq!(tray(State::Blocked).icon_name(), "dialog-warning");
        assert_eq!(tray(State::Offline).icon_name(), "network-offline");
    }
}
