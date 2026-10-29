//! Core sync types and safety rules for Drivebridge.
//!
//! Everything here is pure logic with no I/O, so every safety decision can be unit tested.

pub mod ignore;
pub mod planner;

pub use ignore::IgnoreRules;
pub use planner::{plan, FileMeta, Snapshot};

use std::fmt;
use std::path::{Path, PathBuf};

/// A single step in a sync plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    Upload(PathBuf),
    Download(PathBuf),
    DeleteLocal(PathBuf),
    DeleteRemote(PathBuf),
    Conflict(PathBuf),
    Skip(PathBuf),
}

impl SyncAction {
    pub fn path(&self) -> &Path {
        match self {
            SyncAction::Upload(p)
            | SyncAction::Download(p)
            | SyncAction::DeleteLocal(p)
            | SyncAction::DeleteRemote(p)
            | SyncAction::Conflict(p)
            | SyncAction::Skip(p) => p,
        }
    }

    /// A stable name for logs and the history table.
    pub fn kind(&self) -> &'static str {
        match self {
            SyncAction::Upload(_) => "upload",
            SyncAction::Download(_) => "download",
            SyncAction::DeleteLocal(_) => "delete-local",
            SyncAction::DeleteRemote(_) => "delete-remote",
            SyncAction::Conflict(_) => "conflict",
            SyncAction::Skip(_) => "skip",
        }
    }

    pub fn is_delete(&self) -> bool {
        matches!(
            self,
            SyncAction::DeleteLocal(_) | SyncAction::DeleteRemote(_)
        )
    }
}

/// Limits that stop a sync plan from deleting too much at once.
#[derive(Debug, Clone, PartialEq)]
pub struct SafetyPolicy {
    pub max_delete_count: usize,
    pub max_delete_ratio: f64,
}

impl Default for SafetyPolicy {
    fn default() -> Self {
        Self {
            max_delete_count: 50,
            max_delete_ratio: 0.10,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SafetyViolation {
    TooManyDeletes { count: usize, limit: usize },
    DeleteRatioExceeded { ratio: f64, limit: f64 },
}

impl fmt::Display for SafetyViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SafetyViolation::TooManyDeletes { count, limit } => {
                write!(f, "plan deletes {count} files, limit is {limit}")
            }
            SafetyViolation::DeleteRatioExceeded { ratio, limit } => write!(
                f,
                "plan deletes {:.1}% of files, limit is {:.1}%",
                ratio * 100.0,
                limit * 100.0
            ),
        }
    }
}

impl std::error::Error for SafetyViolation {}

impl SafetyPolicy {
    /// Rejects plans that would delete more than the configured count or ratio.
    pub fn check_plan(
        &self,
        actions: &[SyncAction],
        total_files: usize,
    ) -> Result<(), SafetyViolation> {
        let count = actions.iter().filter(|a| a.is_delete()).count();
        if count == 0 {
            return Ok(());
        }
        if count > self.max_delete_count {
            return Err(SafetyViolation::TooManyDeletes {
                count,
                limit: self.max_delete_count,
            });
        }
        // An empty tree with pending deletes means our state is wrong; treat it as 100%.
        let ratio = if total_files == 0 {
            1.0
        } else {
            count as f64 / total_files as f64
        };
        if ratio > self.max_delete_ratio {
            return Err(SafetyViolation::DeleteRatioExceeded {
                ratio,
                limit: self.max_delete_ratio,
            });
        }
        Ok(())
    }
}

/// Checks a plan against the default [`SafetyPolicy`].
pub fn check_plan(actions: &[SyncAction], total_files: usize) -> Result<(), SafetyViolation> {
    SafetyPolicy::default().check_plan(actions, total_files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deletes(n: usize) -> Vec<SyncAction> {
        (0..n)
            .map(|i| SyncAction::DeleteRemote(PathBuf::from(format!("f{i}"))))
            .collect()
    }

    #[test]
    fn allows_plan_without_deletes() {
        let plan = vec![
            SyncAction::Upload("a".into()),
            SyncAction::Download("b".into()),
            SyncAction::Skip("c".into()),
        ];
        assert_eq!(check_plan(&plan, 3), Ok(()));
    }

    #[test]
    fn allows_small_delete() {
        assert_eq!(check_plan(&deletes(5), 100), Ok(()));
    }

    #[test]
    fn blocks_delete_count_over_limit() {
        assert_eq!(
            check_plan(&deletes(51), 10_000),
            Err(SafetyViolation::TooManyDeletes {
                count: 51,
                limit: 50
            })
        );
    }

    #[test]
    fn allows_delete_count_at_limit() {
        assert_eq!(check_plan(&deletes(50), 10_000), Ok(()));
    }

    #[test]
    fn blocks_delete_ratio_over_limit() {
        assert!(matches!(
            check_plan(&deletes(11), 100),
            Err(SafetyViolation::DeleteRatioExceeded { .. })
        ));
    }

    #[test]
    fn allows_delete_ratio_at_limit() {
        assert_eq!(check_plan(&deletes(10), 100), Ok(()));
    }

    #[test]
    fn blocks_deletes_when_tree_is_empty() {
        assert!(check_plan(&deletes(1), 0).is_err());
    }

    #[test]
    fn counts_local_and_remote_deletes() {
        let mut plan = deletes(30);
        plan.extend((0..21).map(|i| SyncAction::DeleteLocal(format!("l{i}").into())));
        assert!(matches!(
            check_plan(&plan, 100_000),
            Err(SafetyViolation::TooManyDeletes { count: 51, .. })
        ));
    }
}
