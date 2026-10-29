//! The main window: what the background service is doing, and files to restore.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDateTime};
use drivebridge_engine::settings::{Paths, LOCAL_ROOT, TRASH_DAYS};
use drivebridge_engine::trash::{self, TrashedFile};
use drivebridge_store::Store;
use gtk::glib;
use zbus::blocking::Connection;

use crate::daemon::{self, DaemonStatus};

/// How many removed files the list shows.
const SHOWN: usize = 50;

struct Widgets {
    toasts: adw::ToastOverlay,
    state: adw::ActionRow,
    last_sync: adw::ActionRow,
    files: adw::ActionRow,
    sync_now: gtk::Button,
    pause: gtk::Button,
    removed: gtk::ListBox,
    /// The last sync time we showed, to refresh the removed list after each sync.
    seen_sync: Cell<i64>,
}

pub fn show(app: &adw::Application) {
    let widgets = Rc::new(build(app));
    let bus = Rc::new(daemon::connect().ok());

    let (w, b) = (widgets.clone(), bus.clone());
    widgets.sync_now.connect_clicked(move |_| act(&w, &b, "SyncNow"));
    let (w, b) = (widgets.clone(), bus.clone());
    widgets.pause.connect_clicked(move |button| {
        let resume = button.label().as_deref() == Some("Resume");
        act(&w, &b, if resume { "Resume" } else { "Pause" });
    });

    refresh(&widgets, &bus);
    refresh_removed(&widgets);
    let (w, b) = (widgets.clone(), bus.clone());
    glib::timeout_add_seconds_local(2, move || {
        refresh(&w, &b);
        glib::ControlFlow::Continue
    });
}

fn build(app: &adw::Application) -> Widgets {
    let state = row("Status", "Connecting…");
    let last_sync = row("Last sync", "—");
    let files = row("Files in sync", "—");

    let sync_now = gtk::Button::with_label("Sync now");
    sync_now.add_css_class("suggested-action");
    sync_now.add_css_class("pill");
    let pause = gtk::Button::with_label("Pause");
    pause.add_css_class("pill");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    buttons.set_halign(gtk::Align::Center);
    buttons.set_margin_top(12);
    buttons.append(&sync_now);
    buttons.append(&pause);

    let sync_group = adw::PreferencesGroup::new();
    sync_group.set_title("Sync");
    sync_group.add(&state);
    sync_group.add(&last_sync);
    sync_group.add(&files);
    sync_group.add(&buttons);

    let removed = gtk::ListBox::new();
    removed.add_css_class("boxed-list");
    removed.set_selection_mode(gtk::SelectionMode::None);
    let removed_group = adw::PreferencesGroup::new();
    removed_group.set_title("Recently removed");
    let note = format!(
        "Files a sync deleted or overwrote on this computer. They are kept for {TRASH_DAYS} days."
    );
    removed_group.set_description(Some(&note));
    removed_group.add(&removed);

    let page = adw::PreferencesPage::new();
    page.add(&sync_group);
    page.add(&removed_group);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&page));
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&toasts));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Drivebridge")
        .default_width(520)
        .default_height(680)
        .content(&view)
        .build();
    window.present();

    Widgets {
        toasts,
        state,
        last_sync,
        files,
        sync_now,
        pause,
        removed,
        seen_sync: Cell::new(-1),
    }
}

fn row(title: &str, subtitle: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .use_markup(false)
        .build()
}

fn toast(w: &Widgets, text: &str) {
    w.toasts.add_toast(adw::Toast::new(text));
}

fn act(w: &Rc<Widgets>, bus: &Option<Connection>, method: &str) {
    let result = match bus {
        Some(conn) => daemon::call(conn, method).map_err(|e| e.to_string()),
        None => Err("there is no D-Bus session".to_string()),
    };
    if let Err(e) = result {
        toast(w, &format!("Could not reach the background service: {e}"));
    }
    refresh(w, bus);
}

fn refresh(w: &Rc<Widgets>, bus: &Option<Connection>) {
    match bus.as_ref().map(daemon::status) {
        Some(Ok(status)) => show_status(w, &status),
        _ => {
            w.state.set_subtitle(
                "The background service isn't running. Start it with \
                 `systemctl --user start drivebridged`.",
            );
            w.last_sync.set_subtitle("—");
            w.files.set_subtitle("—");
            w.sync_now.set_sensitive(false);
            w.pause.set_sensitive(false);
        }
    }
}

fn show_status(w: &Rc<Widgets>, status: &DaemonStatus) {
    w.state.set_subtitle(&status.message);
    w.last_sync.set_subtitle(&describe_time(status.last_sync));
    w.files.set_subtitle(&status.tracked.to_string());
    w.sync_now.set_sensitive(status.state != "syncing");
    w.pause.set_sensitive(true);
    let paused = status.state == "paused";
    w.pause.set_label(if paused { "Resume" } else { "Pause" });
    if w.seen_sync.replace(status.last_sync) != status.last_sync {
        refresh_removed(w);
    }
}

fn describe_time(timestamp: i64) -> String {
    if timestamp == 0 {
        return "Never".to_string();
    }
    DateTime::from_timestamp(timestamp, 0)
        .map(|t| t.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

fn refresh_removed(w: &Rc<Widgets>) {
    w.removed.remove_all();
    let files = Paths::new()
        .ok()
        .and_then(|paths| trash::list(&paths.trash).ok())
        .unwrap_or_default();
    if files.is_empty() {
        w.removed.append(&row("Nothing has been removed", ""));
        return;
    }
    for file in files.into_iter().take(SHOWN) {
        let title = file.path.to_string_lossy().into_owned();
        let when = format!("Removed {}", describe_stamp(&file.stamp));
        let item = row(&title, &when);
        let button = gtk::Button::with_label("Restore");
        button.set_valign(gtk::Align::Center);
        button.add_css_class("flat");
        let w2 = w.clone();
        button.connect_clicked(move |_| {
            match restore(&file) {
                Ok(to) => toast(&w2, &format!("Restored {}", to.display())),
                Err(e) => toast(&w2, &format!("{e:#}")),
            }
            refresh_removed(&w2);
        });
        item.add_suffix(&button);
        w.removed.append(&item);
    }
}

fn describe_stamp(stamp: &str) -> String {
    NaiveDateTime::parse_from_str(stamp, trash::STAMP_FORMAT)
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| stamp.to_string())
}

fn restore(file: &TrashedFile) -> Result<PathBuf> {
    let paths = Paths::new()?;
    let store = Store::open(&paths.db)?;
    let local_root = store.setting(LOCAL_ROOT)?.context("not set up yet")?;
    let to = trash::restore(&paths.trash, Path::new(&local_root), file)
        .with_context(|| format!("could not restore {}", file.path.display()))?;
    Ok(to)
}
