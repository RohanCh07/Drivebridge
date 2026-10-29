//! Google credentials in the system keyring (Secret Service: GNOME Keyring, KWallet).

use anyhow::{Context, Result};
use drivebridge_rclone::drive::DriveCredentials;

const SERVICE: &str = "drivebridge";
const ACCOUNT: &str = "google-drive";

fn entry() -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, ACCOUNT).context("could not open the system keyring")
}

pub fn save(creds: &DriveCredentials) -> Result<()> {
    let json = serde_json::to_string(creds)?;
    entry()?
        .set_password(&json)
        .context("could not save Google credentials to the system keyring")
}

pub fn load() -> Result<DriveCredentials> {
    let json = entry()?
        .get_password()
        .context("no Google credentials in the keyring; run `drivebridge setup` first")?;
    serde_json::from_str(&json)
        .context("the saved Google credentials are unreadable; run `drivebridge setup` again")
}
