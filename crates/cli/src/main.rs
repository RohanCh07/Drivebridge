//! drivebridge: command-line control for the sync service.

mod config;
mod restore;
mod secrets;
mod setup;
mod status;
mod sync;
mod trash;

use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use drivebridge_rclone::Rcd;

#[derive(Parser)]
#[command(
    name = "drivebridge",
    version,
    about = "Two-way Google Drive sync for Linux"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Connect a Google account and choose folders to sync
    Setup(setup::SetupArgs),
    /// Run a sync now
    Sync(sync::SyncArgs),
    /// Show sync status and recent changes
    Status,
    /// Pause syncing
    Pause,
    /// Resume syncing
    Resume,
    /// Restore a file that a sync deleted or overwrote locally
    Restore(restore::RestoreArgs),
    /// List a folder through rclone, e.g. `drivebridge ls gdrive:Documents`
    Ls {
        /// An rclone path: a configured remote like `gdrive:Folder`, or a local folder
        target: String,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("drivebridge: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<ExitCode> {
    match command {
        Command::Setup(args) => setup::run(args)?,
        Command::Sync(args) => return sync::run(args),
        Command::Status => status::show()?,
        Command::Pause => status::set_paused(true)?,
        Command::Resume => status::set_paused(false)?,
        Command::Restore(args) => restore::run(args)?,
        Command::Ls { target } => ls(&target)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn ls(target: &str) -> Result<()> {
    let rcd = Rcd::start(&config::rclone(), &[])?;
    eprintln!("using rclone {}", rcd.client().version()?);
    for item in rcd.client().list(target)? {
        if item.is_dir {
            println!("{:>12}  {}/", "-", item.name);
        } else {
            println!("{:>12}  {}", item.size, item.name);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
