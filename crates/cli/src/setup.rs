//! `drivebridge setup`: sign in to Google and choose what to sync.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use drivebridge_engine::setup::{self, SetupRequest};

use crate::config;

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
    let path = &args.client_secret;
    let client_secret_json =
        fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
    let local = match args.local {
        Some(dir) => dir,
        None => config::home()?.join("GoogleDrive"),
    };

    println!("Opening Google sign-in in your browser...");
    let request = SetupRequest {
        client_secret_json,
        local,
        drive_folder: args.drive_folder,
    };
    let local = setup::connect(request)?;
    println!("Connected. {} is ready to sync.", local.display());
    println!("Preview the first sync with `drivebridge sync --dry-run`.");
    Ok(())
}
