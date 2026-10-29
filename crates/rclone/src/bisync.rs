//! Runs `rclone bisync`, which carries out a two-way sync once our own checks pass.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Error, Result};

/// Options for one `rclone bisync` run. Needs rclone 1.66 or newer for `--backup-dir1`.
#[derive(Debug, Clone, Default)]
pub struct Bisync {
    /// The local folder.
    pub path1: String,
    /// The Drive folder, e.g. `drivebridge:Drivebridge`.
    pub path2: String,
    /// Where bisync keeps its own listings between runs.
    pub workdir: PathBuf,
    /// Local files that bisync deletes or overwrites are moved here instead.
    pub backup_dir1: Option<PathBuf>,
    /// First run, or recovery: rebuild bisync's listings without deleting anything.
    pub resync: bool,
    /// Skip bisync's own max-delete check (ours already ran).
    pub force: bool,
    /// rclone exclude patterns.
    pub excludes: Vec<String>,
}

impl Bisync {
    pub fn args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![
            "bisync".into(),
            (&self.path1).into(),
            (&self.path2).into(),
            "--workdir".into(),
            self.workdir.clone().into(),
            "--create-empty-src-dirs".into(),
            "--compare".into(),
            "size,modtime".into(),
            "--conflict-resolve".into(),
            "none".into(),
            "--conflict-loser".into(),
            "num".into(),
            "--resilient".into(),
            "--recover".into(),
            "--verbose".into(),
        ];
        if let Some(dir) = &self.backup_dir1 {
            args.push("--backup-dir1".into());
            args.push(dir.clone().into());
        }
        if self.resync {
            // On a first sync, a file that differs on both sides keeps the newer copy;
            // the older local copy goes to backup_dir1.
            args.push("--resync".into());
            args.push("--resync-mode".into());
            args.push("newer".into());
        }
        if self.force {
            args.push("--force".into());
        }
        for pattern in &self.excludes {
            args.push("--exclude".into());
            args.push(pattern.into());
        }
        args
    }

    /// Runs bisync with `env` (credentials) and the user's terminal as output.
    pub fn run(&self, rclone: &Path, env: &[(String, String)]) -> Result<()> {
        let status = Command::new(rclone)
            .args(self.args())
            .envs(env.iter().map(|(k, v)| (k, v)))
            .status()
            .map_err(Error::Spawn)?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Failed {
                command: "bisync".into(),
                status: status.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_pair(args: &[OsString], flag: &str, value: &str) -> bool {
        args.windows(2).any(|w| w[0] == flag && w[1] == value)
    }

    #[test]
    fn builds_arguments() {
        let run = Bisync {
            path1: "/home/me/Drive".into(),
            path2: "drivebridge:Drivebridge".into(),
            workdir: "/tmp/work".into(),
            backup_dir1: Some("/tmp/trash/1".into()),
            resync: true,
            force: false,
            excludes: vec![".git/**".into()],
        };
        let args = run.args();
        assert_eq!(args[0], "bisync");
        assert_eq!(args[1], "/home/me/Drive");
        assert_eq!(args[2], "drivebridge:Drivebridge");
        assert!(has_pair(&args, "--workdir", "/tmp/work"));
        assert!(has_pair(&args, "--backup-dir1", "/tmp/trash/1"));
        assert!(has_pair(&args, "--exclude", ".git/**"));
        assert!(args.iter().any(|a| a == "--resync"));
        assert!(!args.iter().any(|a| a == "--force"));
    }
}
