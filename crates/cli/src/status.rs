//! `drivebridge status`, `pause` and `resume`.

use anyhow::Result;
use chrono::{DateTime, Local};
use drivebridge_store::Store;

use crate::config::{Paths, DRIVE_FOLDER, LAST_SYNC, LOCAL_ROOT, PAUSED};

pub fn show() -> Result<()> {
    let paths = Paths::new()?;
    let store = Store::open(&paths.db)?;
    let (Some(local), Some(folder)) = (store.setting(LOCAL_ROOT)?, store.setting(DRIVE_FOLDER)?)
    else {
        println!("Not set up. Run `drivebridge setup --client-secret <FILE>`.");
        return Ok(());
    };
    let paused = store.setting(PAUSED)?.as_deref() == Some("true");
    let last_sync = store
        .setting(LAST_SYNC)?
        .and_then(|s| s.parse::<i64>().ok())
        .map_or_else(|| "never".to_string(), format_time);

    println!("Local folder:  {local}");
    println!("Drive folder:  {folder}");
    println!(
        "State:         {}",
        if paused { "paused" } else { "active" }
    );
    println!("Last sync:     {last_sync}");
    println!("Tracked files: {}", store.base()?.len());

    let history = store.recent_history(10)?;
    if !history.is_empty() {
        println!("\nRecent changes:");
        for entry in history {
            let at = format_time(entry.at);
            println!("  {at}  {:<13} {}", entry.action, entry.path);
        }
    }
    Ok(())
}

pub fn set_paused(paused: bool) -> Result<()> {
    let paths = Paths::new()?;
    let store = Store::open(&paths.db)?;
    store.set_setting(PAUSED, if paused { "true" } else { "false" })?;
    println!(
        "{}",
        if paused {
            "Syncing paused."
        } else {
            "Syncing resumed."
        }
    );
    Ok(())
}

fn format_time(timestamp: i64) -> String {
    DateTime::from_timestamp(timestamp, 0)
        .map(|t| t.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
