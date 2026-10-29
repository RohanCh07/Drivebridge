//! Three-way sync planning: compare local, remote and last-synced state, decide what to do.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::ignore::IgnoreRules;
use crate::SyncAction;

/// What we know about one file at a point in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    pub size: u64,
    /// Modification time in seconds since the Unix epoch.
    pub mtime: i64,
    /// Content hash, when both sides can provide the same kind (e.g. MD5 on Drive).
    pub hash: Option<String>,
}

impl FileMeta {
    /// True when both describe the same content. Hashes win; otherwise size and mtime must match.
    pub fn same_content(&self, other: &FileMeta) -> bool {
        match (&self.hash, &other.hash) {
            (Some(a), Some(b)) => a == b,
            _ => self.size == other.size && self.mtime == other.mtime,
        }
    }
}

/// Files under the sync root, keyed by path relative to it.
pub type Snapshot = BTreeMap<std::path::PathBuf, FileMeta>;

/// Builds a sync plan from the current local and remote trees and the state after the last sync.
///
/// When in doubt the planner keeps data: an edit beats a delete, and two different edits
/// become a conflict (both copies kept). Run the result through [`crate::check_plan`] before
/// carrying it out.
pub fn plan(
    local: &Snapshot,
    remote: &Snapshot,
    base: &Snapshot,
    ignore: &IgnoreRules,
) -> Vec<SyncAction> {
    let paths: BTreeSet<_> = local
        .keys()
        .chain(remote.keys())
        .chain(base.keys())
        .collect();
    paths
        .into_iter()
        .filter(|p| !ignore.is_ignored(p))
        .filter_map(|p| decide(p, local.get(p), remote.get(p), base.get(p)))
        .collect()
}

fn decide(
    path: &Path,
    local: Option<&FileMeta>,
    remote: Option<&FileMeta>,
    base: Option<&FileMeta>,
) -> Option<SyncAction> {
    let p = path.to_path_buf();
    let changed = |now: &FileMeta| !matches!(base, Some(b) if now.same_content(b));
    let action = match (local, remote, base) {
        // Gone on both sides; the store just forgets it.
        (None, None, _) => return None,
        (Some(l), Some(r), _) if l.same_content(r) => SyncAction::Skip(p),
        (Some(l), Some(r), Some(_)) => match (changed(l), changed(r)) {
            (true, false) => SyncAction::Upload(p),
            (false, true) => SyncAction::Download(p),
            _ => SyncAction::Conflict(p),
        },
        // Created on both sides with different content.
        (Some(_), Some(_), None) => SyncAction::Conflict(p),
        (Some(_), None, None) => SyncAction::Upload(p),
        (None, Some(_), None) => SyncAction::Download(p),
        // Deleted on Drive. A local edit since the last sync wins over the delete.
        (Some(l), None, Some(_)) => {
            if changed(l) {
                SyncAction::Upload(p)
            } else {
                SyncAction::DeleteLocal(p)
            }
        }
        // Deleted locally. An edit on Drive since the last sync wins over the delete.
        (None, Some(r), Some(_)) => {
            if changed(r) {
                SyncAction::Download(p)
            } else {
                SyncAction::DeleteRemote(p)
            }
        }
    };
    Some(action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn meta(size: u64, mtime: i64) -> FileMeta {
        FileMeta {
            size,
            mtime,
            hash: None,
        }
    }

    fn snap(entries: &[(&str, FileMeta)]) -> Snapshot {
        entries
            .iter()
            .map(|(p, m)| (PathBuf::from(p), m.clone()))
            .collect()
    }

    fn one(
        local: Option<FileMeta>,
        remote: Option<FileMeta>,
        base: Option<FileMeta>,
    ) -> Vec<SyncAction> {
        let s = |m: Option<FileMeta>| m.map(|m| snap(&[("f", m)])).unwrap_or_default();
        plan(&s(local), &s(remote), &s(base), &IgnoreRules::new())
    }

    const V1: FileMeta = FileMeta {
        size: 10,
        mtime: 100,
        hash: None,
    };
    const V2: FileMeta = FileMeta {
        size: 12,
        mtime: 200,
        hash: None,
    };
    const V3: FileMeta = FileMeta {
        size: 14,
        mtime: 300,
        hash: None,
    };

    fn f(action: fn(PathBuf) -> SyncAction) -> Vec<SyncAction> {
        vec![action(PathBuf::from("f"))]
    }

    #[test]
    fn unchanged_is_skipped() {
        assert_eq!(one(Some(V1), Some(V1), Some(V1)), f(SyncAction::Skip));
    }

    #[test]
    fn new_local_file_is_uploaded() {
        assert_eq!(one(Some(V1), None, None), f(SyncAction::Upload));
    }

    #[test]
    fn new_remote_file_is_downloaded() {
        assert_eq!(one(None, Some(V1), None), f(SyncAction::Download));
    }

    #[test]
    fn local_edit_is_uploaded() {
        assert_eq!(one(Some(V2), Some(V1), Some(V1)), f(SyncAction::Upload));
    }

    #[test]
    fn remote_edit_is_downloaded() {
        assert_eq!(one(Some(V1), Some(V2), Some(V1)), f(SyncAction::Download));
    }

    #[test]
    fn different_edits_on_both_sides_conflict() {
        assert_eq!(one(Some(V2), Some(V3), Some(V1)), f(SyncAction::Conflict));
    }

    #[test]
    fn identical_edits_on_both_sides_are_skipped() {
        assert_eq!(one(Some(V2), Some(V2), Some(V1)), f(SyncAction::Skip));
    }

    #[test]
    fn different_new_files_conflict() {
        assert_eq!(one(Some(V1), Some(V2), None), f(SyncAction::Conflict));
    }

    #[test]
    fn remote_delete_of_unchanged_file_deletes_locally() {
        assert_eq!(one(Some(V1), None, Some(V1)), f(SyncAction::DeleteLocal));
    }

    #[test]
    fn local_edit_beats_remote_delete() {
        assert_eq!(one(Some(V2), None, Some(V1)), f(SyncAction::Upload));
    }

    #[test]
    fn local_delete_of_unchanged_file_deletes_remotely() {
        assert_eq!(one(None, Some(V1), Some(V1)), f(SyncAction::DeleteRemote));
    }

    #[test]
    fn remote_edit_beats_local_delete() {
        assert_eq!(one(None, Some(V2), Some(V1)), f(SyncAction::Download));
    }

    #[test]
    fn deleted_on_both_sides_needs_nothing() {
        assert_eq!(one(None, None, Some(V1)), vec![]);
    }

    #[test]
    fn hashes_override_mtime() {
        let a = FileMeta {
            size: 10,
            mtime: 100,
            hash: Some("abc".into()),
        };
        let touched = FileMeta {
            mtime: 999,
            ..a.clone()
        };
        assert!(a.same_content(&touched));
        // Without a hash on both sides, a changed mtime means changed content.
        assert!(!a.same_content(&meta(10, 999)));
    }

    #[test]
    fn ignored_paths_are_left_out() {
        let local = snap(&[(".git/HEAD", meta(1, 1)), ("a.txt", meta(1, 1))]);
        let actions = plan(&local, &Snapshot::new(), &Snapshot::new(), &IgnoreRules::new());
        assert_eq!(actions, vec![SyncAction::Upload("a.txt".into())]);
    }
}
