//! `drivebridge setup`: sign in to Google and choose what to sync.

use std::fs;
use std::path::PathBuf;

use anyhow::{ensure, Context, Result};
use clap::Args;
use drivebridge_core::Snapshot;
use drivebridge_rclone::drive::{self, DriveCredentials};
use drivebridge_rclone::Rcd;
use drivebridge_store::Store;

use crate::config::{self, Paths, DRIVE_FOLDER, LOCAL_ROOT, REMOTE};
use crate::secrets;

#[derive(Args)]
pub struct SetupArgs {
    /// The OAuth client JSON from Google Cloud Console (a "Desktop app" client)
    #[arg(long, value_name = "FILE")]
    client_secret: PathBuf,
    /// Local folder to sync [default: ~/GoogleDrive]
    #[arg(long, value_name = "DIR")]
    local: Option<PathBuf>,
    /// Folder on Drive to sync with; created if missing
    #[arg(long, value_name = "NAME", default_value = "Drivebridge")]
    drive_folder: String,
}

pub fn run(args: SetupArgs) -> Result<()> {
    let folder = args.drive_folder.trim_matches('/').to_string();
    ensure!(
        !folder.is_empty(),
        "syncing your whole Drive isn't supported yet; pick a folder with --drive-folder"
    );

    let path = &args.client_secret;
    let json = fs::read_to_string(path)
        .with_context(|| format!("could not read {}", path.display()))?;
    let (client_id, client_secret) = drive::parse_client_secret_file(&json)
        .context("that file isn't a Google OAuth client (expected a Desktop app client)")?;

    let local = match args.local {
        Some(dir) => dir,
        None => config::home()?.join("GoogleDrive"),
    };
    fs::create_dir_all(&local)
        .with_context(|| format!("could not create {}", local.display()))?;
    let local = local.canonicalize()?;

    let rclone = config::rclone();
    println!("Opening Google sign-in in your browser...");
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

    println!("Connected. {} will sync with Drive folder \"{folder}\".", local.display());
    println!("Preview the first sync with `drivebridge sync --dry-run`.");
    Ok(())
}
