//! The first-run wizard: the user's own Google OAuth client, the two folders, then sign-in.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use anyhow::{Context, Result};
use drivebridge_engine::settings;
use drivebridge_engine::setup::{self, SetupRequest};
use gtk::{gio, glib};

struct Form {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    client_row: adw::ActionRow,
    choose: gtk::Button,
    local: adw::EntryRow,
    drive: adw::EntryRow,
    sign_in: gtk::Button,
    spinner: gtk::Spinner,
    client_file: RefCell<Option<PathBuf>>,
}

pub fn show(app: &adw::Application) {
    let form = Rc::new(build(app));

    let f = form.clone();
    form.choose.connect_clicked(move |_| choose_client_file(&f));
    let (f, app) = (form.clone(), app.clone());
    form.sign_in.connect_clicked(move |_| sign_in(&f, &app));
}

fn build(app: &adw::Application) -> Form {
    let client_row = adw::ActionRow::builder()
        .title("OAuth client file")
        .subtitle("The JSON file for a \"Desktop app\" client, from Google Cloud Console")
        .use_markup(false)
        .build();
    let choose = gtk::Button::with_label("Choose…");
    choose.set_valign(gtk::Align::Center);
    client_row.add_suffix(&choose);
    let google = adw::PreferencesGroup::new();
    google.set_title("Your Google client");
    google.set_description(Some(
        "Drivebridge signs in with your own Google Cloud OAuth client, \
         so your files never pass through anyone else's app.",
    ));
    google.add(&client_row);

    let home = settings::home().unwrap_or_default();
    let default_local = home.join("GoogleDrive").to_string_lossy().into_owned();
    let local = adw::EntryRow::builder()
        .title("Folder on this computer")
        .build();
    local.set_text(&default_local);
    let drive = adw::EntryRow::builder()
        .title("Folder on Google Drive")
        .build();
    drive.set_text("Drivebridge");
    let folders = adw::PreferencesGroup::new();
    folders.set_title("What to sync");
    folders.set_description(Some("Both folders are created if they don't exist yet."));
    folders.add(&local);
    folders.add(&drive);

    let sign_in = gtk::Button::with_label("Sign in with Google");
    sign_in.add_css_class("suggested-action");
    sign_in.add_css_class("pill");
    sign_in.set_sensitive(false);
    let spinner = gtk::Spinner::new();
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    actions.set_halign(gtk::Align::Center);
    actions.set_margin_top(12);
    actions.append(&sign_in);
    actions.append(&spinner);
    folders.add(&actions);

    let page = adw::PreferencesPage::new();
    page.add(&google);
    page.add(&folders);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&page));
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&toasts));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Set up Drivebridge")
        .default_width(560)
        .default_height(560)
        .content(&view)
        .build();
    window.present();

    Form {
        window,
        toasts,
        client_row,
        choose,
        local,
        drive,
        sign_in,
        spinner,
        client_file: RefCell::new(None),
    }
}

fn toast(form: &Form, text: &str) {
    form.toasts.add_toast(adw::Toast::new(text));
}

fn choose_client_file(form: &Rc<Form>) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("JSON files"));
    filter.add_pattern("*.json");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder()
        .title("Choose your OAuth client file")
        .modal(true)
        .filters(&filters)
        .build();

    let f = form.clone();
    dialog.open(Some(&form.window), gio::Cancellable::NONE, move |result| {
        let Some(path) = result.ok().and_then(|file| file.path()) else {
            return;
        };
        f.client_row.set_subtitle(&path.display().to_string());
        *f.client_file.borrow_mut() = Some(path);
        f.sign_in.set_sensitive(true);
    });
}

fn sign_in(form: &Rc<Form>, app: &adw::Application) {
    let Some(client_file) = form.client_file.borrow().clone() else {
        return;
    };
    let local = PathBuf::from(form.local.text().as_str());
    let drive_folder = form.drive.text().to_string();

    form.sign_in.set_sensitive(false);
    form.spinner.start();
    toast(form, "Finish signing in in your browser…");

    let (f, app) = (form.clone(), app.clone());
    glib::spawn_future_local(async move {
        let work = move || connect(&client_file, local, drive_folder);
        let result = gio::spawn_blocking(work).await;
        f.spinner.stop();
        f.sign_in.set_sensitive(true);
        match result {
            Ok(Ok(_)) => {
                // If this fails, the status window says the service isn't running and how
                // to start it.
                if let Err(e) = setup::enable_service() {
                    eprintln!("{e}");
                }
                // Open the main window first: closing the last window would quit the app.
                crate::status::show(&app);
                f.window.close();
            }
            Ok(Err(e)) => toast(&f, &format!("{e:#}")),
            Err(_) => toast(&f, "Setup stopped unexpectedly; please try again"),
        }
    });
}

fn connect(client_file: &Path, local: PathBuf, drive_folder: String) -> Result<PathBuf> {
    let client_secret_json = fs::read_to_string(client_file)
        .with_context(|| format!("could not read {}", client_file.display()))?;
    setup::connect(SetupRequest {
        client_secret_json,
        local,
        drive_folder,
    })
}
