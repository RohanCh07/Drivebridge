//! drivebridge: command-line control for the sync service.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

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
    Setup,
    /// Run a sync now
    Sync,
    /// Show sync status
    Status,
    /// Pause syncing
    Pause,
    /// Resume syncing
    Resume,
    /// Restore a deleted or overwritten file
    Restore,
    /// List a folder through rclone, e.g. `drivebridge ls gdrive:Documents`
    Ls {
        /// An rclone path: a configured remote like `gdrive:Folder`, or a local folder
        target: String,
    },
}

fn main() -> ExitCode {
    let name = match Cli::parse().command {
        Command::Ls { target } => return ls(&target),
        Command::Setup => "setup",
        Command::Sync => "sync",
        Command::Status => "status",
        Command::Pause => "pause",
        Command::Resume => "resume",
        Command::Restore => "restore",
    };
    println!("drivebridge {name}: not implemented yet");
    ExitCode::SUCCESS
}

fn ls(target: &str) -> ExitCode {
    // Lets you point at a bundled or newer rclone instead of the one on PATH.
    let rclone = std::env::var_os("DRIVEBRIDGE_RCLONE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rclone"));
    match print_listing(&rclone, target) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("drivebridge ls: {e}");
            ExitCode::FAILURE
        }
    }
}

fn print_listing(rclone: &Path, target: &str) -> drivebridge_rclone::Result<()> {
    let rcd = Rcd::start(rclone, &[])?;
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
