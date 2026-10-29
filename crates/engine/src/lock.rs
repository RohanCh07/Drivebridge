//! One sync at a time per machine: the CLI and the daemon share a lock file.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

/// Held while a sync runs; released when dropped, or by the OS if the process dies.
pub struct SyncLock {
    _file: File,
}

/// Takes the lock without waiting. `None` means another sync holds it.
pub fn try_acquire(path: &Path) -> io::Result<Option<SyncLock>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(SyncLock { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_holder_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync.lock");
        let first = try_acquire(&path).unwrap();
        assert!(first.is_some());
        assert!(try_acquire(&path).unwrap().is_none());
        drop(first);
        assert!(try_acquire(&path).unwrap().is_some());
    }
}
