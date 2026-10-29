//! drivebridge: command-line control for the sync service.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "drivebridge", version, about = "Two-way Google Drive sync for Linux")]
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
}

fn main() {
    let name = match Cli::parse().command {
        Command::Setup => "setup",
        Command::Sync => "sync",
        Command::Status => "status",
        Command::Pause => "pause",
        Command::Resume => "resume",
        Command::Restore => "restore",
    };
    println!("drivebridge {name}: not implemented yet");
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
