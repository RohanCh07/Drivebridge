//! The local trash: one folder per sync run, holding files that run deleted or overwrote.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{Duration, NaiveDateTime};

/// Trash folder names, e.g. `2026-10-29_101500`. Sorting them sorts by time.
pub const STAMP_FORMAT: &str = "%Y-%m-%d_%H%M%S";

/// A file in the trash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashedFile {
    /// The trash folder (sync run) it's in.
    pub stamp: String,
    /// Its path relative to the sync folder.
    pub path: PathBuf,
}

impl TrashedFile {
    pub fn location(&self, trash: &Path) -> PathBuf {
        trash.join(&self.stamp).join(&self.path)
    }
}

/// Everything in the trash, newest run first.
pub fn list(trash: &Path) -> io::Result<Vec<TrashedFile>> {
    let mut files = Vec::new();
    for stamp in stamps(trash)?.into_iter().rev() {
        let root = trash.join(&stamp);
        for path in files_under(&root)? {
            files.push(TrashedFile {
                stamp: stamp.clone(),
                path,
            });
        }
    }
    Ok(files)
}

/// Permanently removes trash folders older than `days`. Returns how many were removed.
pub fn prune(trash: &Path, now: NaiveDateTime, days: i64) -> io::Result<usize> {
    let cutoff = now - Duration::days(days);
    let mut removed = 0;
    for stamp in stamps(trash)? {
        // Leave folders we didn't name alone.
        let Ok(at) = NaiveDateTime::parse_from_str(&stamp, STAMP_FORMAT) else {
            continue;
        };
        if at < cutoff {
            fs::remove_dir_all(trash.join(&stamp))?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// Trash folder names, oldest first.
fn stamps(trash: &Path) -> io::Result<Vec<String>> {
    if !trash.exists() {
        return Ok(Vec::new());
    }
    let mut stamps = Vec::new();
    for entry in fs::read_dir(trash)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            stamps.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    stamps.sort();
    Ok(stamps)
}

/// Files under `root`, as paths relative to it, sorted.
fn files_under(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                pending.push(path);
            } else if let Ok(relative) = path.strip_prefix(root) {
                files.push(relative.to_path_buf());
            }
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(trash: &Path, stamp: &str, path: &str) {
        let file = trash.join(stamp).join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, "x").unwrap();
    }

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, STAMP_FORMAT).unwrap()
    }

    #[test]
    fn lists_newest_run_first() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "2026-10-01_090000", "old.txt");
        put(dir.path(), "2026-10-20_090000", "docs/new.txt");

        let files = list(dir.path()).unwrap();
        let paths: Vec<_> = files.iter().map(|f| f.path.clone()).collect();
        assert_eq!(paths, vec![PathBuf::from("docs/new.txt"), "old.txt".into()]);
        assert_eq!(files[0].stamp, "2026-10-20_090000");
    }

    #[test]
    fn missing_trash_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list(&dir.path().join("nope")).unwrap().is_empty());
    }

    #[test]
    fn prune_removes_only_old_runs() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "2026-09-01_090000", "a.txt");
        put(dir.path(), "2026-10-20_090000", "b.txt");
        put(dir.path(), "keep-me", "c.txt");

        let removed = prune(dir.path(), at("2026-10-29_090000"), 30).unwrap();
        assert_eq!(removed, 1);
        assert!(!dir.path().join("2026-09-01_090000").exists());
        assert!(dir.path().join("2026-10-20_090000").exists());
        assert!(dir.path().join("keep-me").exists());
    }
}
