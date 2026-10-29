//! `drivebridge restore`: bring back files a sync deleted or overwrote locally.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Args;
use drivebridge_engine::trash;
use drivebridge_store::Store;

use crate::config::{Paths, LOCAL_ROOT, TRASH_DAYS};

#[derive(Args)]
pub struct RestoreArgs {
    /// File to restore, relative to your sync folder; omit to list what can be restored
    path: Option<PathBuf>,
}

pub fn run(args: RestoreArgs) -> Result<()> {
    let paths = Paths::new()?;
    let files = trash::list(&paths.trash)?;
    let Some(wanted) = args.path else {
        if files.is_empty() {
            println!("The trash is empty.");
        }
        for file in &files {
            println!("{}  {}", file.stamp, file.path.display());
        }
        return Ok(());
    };

    let store = Store::open(&paths.db)?;
    let local_root = store
        .setting(LOCAL_ROOT)?
        .context("not set up yet; run `drivebridge setup` first")?;
    // The newest copy wins when a file was trashed more than once.
    let Some(file) = files.iter().find(|f| f.path == wanted) else {
        bail!(
            "{} is not in the trash (files stay {TRASH_DAYS} days; files deleted on Drive \
             are in Drive's own trash)",
            wanted.display()
        );
    };
    let to = trash::restore(&paths.trash, Path::new(&local_root), file)
        .with_context(|| format!("could not restore {}", wanted.display()))?;
    println!("Restored {} (from {}).", to.display(), file.stamp);
    Ok(())
}
