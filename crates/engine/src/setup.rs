//! First-time setup, shared by `drivebridge setup` and the desktop app's wizard.

use std::fs;
use std::path::PathBuf;

use anyhow::{ensure, Context, Result};
use drivebridge_core::Snapshot;
use drivebridge_rclone::drive::{self, DriveCredentials};
use drivebridge_rclone::Rcd;
use drivebridge_store::Store;

use crate::secrets;
use crate::settings::{self, Paths, DRIVE_FOLDER, LOCAL_ROOT, REMOTE};

pub struct SetupRequest {
    /// The JSON file Google Cloud Console downloads for a "Desktop app" OAuth client.
    pub client_secret_json: String,
    pub local: PathBuf,
    pub drive_folder: String,
}

/// Signs in through the browser (this blocks until the user finishes), proves the account
/// works by creating the Drive folder, then saves the credentials to the keyring and the
/// folders to the store. Returns the local folder, created if needed.
pub fn connect(request: SetupRequest) -> Result<PathBuf> {
    let folder = request.drive_folder.trim_matches('/').to_string();
    ensure!(
        !folder.is_empty(),
        "syncing your whole Drive isn't supported yet; pick a Drive folder"
    );
    let (client_id, client_secret) =
        drive::parse_client_secret_file(&request.client_secret_json)
            .context("that file isn't a Google OAuth client (expected a Desktop app client)")?;

    let local = request.local;
    fs::create_dir_all(&local).with_context(|| format!("could not create {}", local.display()))?;
    let local = local.canonicalize()?;

    let rclone = settings::rclone();
    let token = drive::authorize(&rclone, &client_id, &client_secret)?;
    let creds = DriveCredentials {
        client_id,
        client_secret,
        token,
    };

    // Prove the account works before saving anything.
    let rcd = Rcd::start(&rclone, &creds.env(REMOTE))?;
    rcd.client()
        .mkdir(&format!("{REMOTE}:"), &folder)
        .context("signed in, but could not create the Drive folder")?;
    drop(rcd);

    secrets::save(&creds)?;
    let paths = Paths::new()?;
    let mut store = Store::open(&paths.db)?;
    store.set_setting(LOCAL_ROOT, &local.to_string_lossy())?;
    store.set_setting(DRIVE_FOLDER, &folder)?;
    // New folders mean a fresh start: the next sync is a first sync.
    store.replace_base(&Snapshot::new())?;
    Ok(local)
}
