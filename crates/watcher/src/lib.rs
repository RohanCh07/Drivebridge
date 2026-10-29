//! Watches the local sync folder (inotify on Linux) and reports changes worth syncing.

use std::path::Path;
use std::sync::mpsc::{self, Receiver};

use drivebridge_core::IgnoreRules;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

pub use notify::Error;

/// Keeps watching while alive.
pub struct LocalWatcher {
    _watcher: RecommendedWatcher,
}

/// Watches `root` recursively. Each relevant change sends `()` on the receiver; the caller
/// debounces them.
pub fn watch(root: &Path, ignore: IgnoreRules) -> Result<(LocalWatcher, Receiver<()>), Error> {
    let (tx, rx) = mpsc::channel();
    let watched = root.to_path_buf();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
        if let Ok(event) = result {
            if is_relevant(&watched, &event, &ignore) {
                // The receiver only goes away when the daemon is shutting down.
                let _ = tx.send(());
            }
        }
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    Ok((LocalWatcher { _watcher: watcher }, rx))
}

/// True for changes to files we sync: not plain reads, and not only ignored paths.
pub fn is_relevant(root: &Path, event: &Event, ignore: &IgnoreRules) -> bool {
    if event.need_rescan() {
        return true;
    }
    if event.kind.is_access() {
        return false;
    }
    event.paths.iter().any(|path| {
        path.strip_prefix(root)
            .is_ok_and(|relative| !ignore.is_ignored(relative))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;

    use notify::event::{AccessKind, CreateKind, EventKind};

    const ROOT: &str = "/home/me/GoogleDrive";

    fn relevant(kind: EventKind, path: &str) -> bool {
        let event = Event::new(kind).add_path(PathBuf::from(path));
        is_relevant(Path::new(ROOT), &event, &IgnoreRules::new())
    }

    #[test]
    fn new_files_count() {
        let kind = EventKind::Create(CreateKind::File);
        assert!(relevant(kind, "/home/me/GoogleDrive/docs/a.txt"));
    }

    #[test]
    fn reads_do_not_count() {
        let kind = EventKind::Access(AccessKind::Read);
        assert!(!relevant(kind, "/home/me/GoogleDrive/a.txt"));
    }

    #[test]
    fn ignored_and_outside_paths_do_not_count() {
        let kind = EventKind::Create(CreateKind::File);
        assert!(!relevant(kind, "/home/me/GoogleDrive/.git/index"));
        assert!(!relevant(kind, "/home/me/GoogleDrive/notes.txt.swp"));
        assert!(!relevant(kind, "/home/me/Downloads/a.txt"));
    }

    #[test]
    fn reports_a_real_change() {
        let dir = tempfile::tempdir().unwrap();
        let (_watcher, changes) = watch(dir.path(), IgnoreRules::new()).unwrap();
        fs::write(dir.path().join("a.txt"), "a").unwrap();
        assert!(changes.recv_timeout(Duration::from_secs(5)).is_ok());
    }
}
