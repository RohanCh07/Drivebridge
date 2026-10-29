//! Data-loss scenarios from the roadmap, end to end: a real `rclone bisync` between two local
//! folders, one standing in for Google Drive.
//!
//! Needs rclone 1.66+ on PATH or in `DRIVEBRIDGE_RCLONE`: `cargo test -- --include-ignored`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::Local;
use drivebridge_core::{IgnoreRules, SafetyPolicy, SyncAction};
use drivebridge_engine::{sync, trash, Error, Options, Outcome, SyncConfig};
use drivebridge_store::Store;
use tempfile::TempDir;

/// A local folder, a fake Drive folder, and the state between runs.
struct World {
    dir: TempDir,
    local: PathBuf,
    drive: PathBuf,
    config: SyncConfig,
    store: Store,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("local");
        let drive = dir.path().join("drive");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&drive).unwrap();
        let rclone = std::env::var_os("DRIVEBRIDGE_RCLONE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("rclone"));
        let config = SyncConfig {
            rclone,
            env: Vec::new(),
            local_root: local.to_string_lossy().into_owned(),
            remote_root: drive.to_string_lossy().into_owned(),
            workdir: dir.path().join("bisync"),
            trash: dir.path().join("trash"),
            ignore: IgnoreRules::new(),
            policy: SafetyPolicy::default(),
        };
        let store = Store::open_in_memory().unwrap();
        World {
            dir,
            local,
            drive,
            config,
            store,
        }
    }

    fn sync(&mut self) -> Outcome {
        self.try_sync(Options::default()).unwrap()
    }

    fn try_sync(&mut self, options: Options) -> Result<Outcome, Error> {
        sync(&self.config, &mut self.store, options, Local::now())
    }

    /// `count` files on the laptop, synced to Drive.
    fn synced_files(count: usize) -> Self {
        let mut world = World::new();
        for i in 0..count {
            write(&world.local, &format!("f{i}.txt"), &format!("file {i}"), 60);
        }
        assert!(matches!(world.sync(), Outcome::Synced { .. }));
        assert_eq!(tree(&world.drive).len(), count);
        world
    }
}

/// Writes `content` with a modification time `minutes_ago`, so each edit is clearly newer
/// than the last to a size-and-mtime comparison.
fn write(root: &Path, path: &str, content: &str, minutes_ago: u64) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, content).unwrap();
    let mtime = SystemTime::now() - Duration::from_secs(minutes_ago * 60);
    let handle = fs::File::options().write(true).open(&file).unwrap();
    handle.set_modified(mtime).unwrap();
}

fn read(root: &Path, path: &str) -> Option<String> {
    fs::read_to_string(root.join(path)).ok()
}

/// Every file under `root` as (relative path, content), sorted.
fn tree(root: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(root).unwrap();
                let content = fs::read_to_string(&path).unwrap_or_default();
                files.push((relative.to_string_lossy().into_owned(), content));
            }
        }
    }
    files.sort();
    files
}

fn contents(root: &Path) -> Vec<String> {
    tree(root).into_iter().map(|(_, content)| content).collect()
}

#[test]
#[ignore = "needs rclone 1.66+"]
fn first_sync_merges_both_sides() {
    let mut world = World::new();
    write(&world.local, "laptop.txt", "from laptop", 60);
    write(&world.drive, "docs/drive.txt", "from drive", 60);

    assert!(matches!(world.sync(), Outcome::Synced { .. }));
    let drive_copy = read(&world.drive, "laptop.txt");
    assert_eq!(drive_copy.as_deref(), Some("from laptop"));
    let local_copy = read(&world.local, "docs/drive.txt");
    assert_eq!(local_copy.as_deref(), Some("from drive"));

    assert!(matches!(world.sync(), Outcome::UpToDate));
}

#[test]
#[ignore = "needs rclone 1.66+"]
fn same_file_edited_on_both_sides_keeps_both() {
    let mut world = World::new();
    write(&world.local, "notes.txt", "original", 60);
    world.sync();

    write(&world.local, "notes.txt", "edited on laptop", 0);
    write(&world.drive, "notes.txt", "edited on Drive!!", 1);
    let Outcome::Synced { actions, .. } = world.sync() else {
        panic!("expected a sync");
    };
    assert_eq!(actions, vec![SyncAction::Conflict("notes.txt".into())]);

    for side in [&world.local, &world.drive] {
        let found = contents(side);
        assert!(found.contains(&"edited on laptop".to_string()), "{found:?}");
        assert!(found.contains(&"edited on Drive!!".to_string()), "{found:?}");
    }
}

#[test]
#[ignore = "needs rclone 1.66+"]
fn local_folder_wiped_while_stopped_is_blocked() {
    let mut world = World::synced_files(60);
    fs::remove_dir_all(&world.local).unwrap();
    fs::create_dir_all(&world.local).unwrap();

    let outcome = world.sync();
    assert!(matches!(outcome, Outcome::Blocked { .. }), "{outcome:?}");
    assert_eq!(tree(&world.drive).len(), 60);
    // Still blocked next time: nothing was recorded as agreed.
    assert!(matches!(world.sync(), Outcome::Blocked { .. }));
}

#[test]
#[ignore = "needs rclone 1.66+"]
fn unreachable_drive_changes_nothing() {
    let mut world = World::synced_files(5);
    let gone = world.dir.path().join("unplugged");
    world.config.remote_root = gone.to_string_lossy().into_owned();

    let result = world.try_sync(Options::default());
    assert!(matches!(result, Err(Error::Offline(_))), "{result:?}");
    assert_eq!(tree(&world.local).len(), 5);
}

#[test]
#[ignore = "needs rclone 1.66+"]
fn file_deleted_on_drive_goes_to_local_trash() {
    let mut world = World::synced_files(10);
    fs::remove_file(world.drive.join("f3.txt")).unwrap();

    assert!(matches!(world.sync(), Outcome::Synced { .. }));
    assert_eq!(read(&world.local, "f3.txt"), None);

    let trashed = trash::list(&world.config.trash).unwrap();
    let file = trashed
        .iter()
        .find(|f| f.path == Path::new("f3.txt"))
        .expect("f3.txt should be in the trash");
    let saved = fs::read_to_string(file.location(&world.config.trash)).unwrap();
    assert_eq!(saved, "file 3");
}

#[test]
#[ignore = "needs rclone 1.66+"]
fn local_delete_removes_file_from_drive() {
    let mut world = World::synced_files(10);
    fs::remove_file(world.local.join("f3.txt")).unwrap();

    assert!(matches!(world.sync(), Outcome::Synced { .. }));
    assert_eq!(read(&world.drive, "f3.txt"), None);
    assert_eq!(tree(&world.drive).len(), 9);
}

#[test]
#[ignore = "needs rclone 1.66+"]
fn edit_on_drive_survives_local_delete() {
    let mut world = World::synced_files(10);
    fs::remove_file(world.local.join("f3.txt")).unwrap();
    write(&world.drive, "f3.txt", "edited on Drive", 0);

    assert!(matches!(world.sync(), Outcome::Synced { .. }));
    let restored = read(&world.local, "f3.txt");
    assert_eq!(restored.as_deref(), Some("edited on Drive"));
}

#[test]
#[ignore = "needs rclone 1.66+"]
fn ignored_files_stay_local() {
    let mut world = World::new();
    write(&world.local, ".git/HEAD", "ref: main", 60);
    write(&world.local, "node_modules/x/index.js", "x", 60);
    write(&world.local, ".draft.txt.swp", "swap", 60);
    write(&world.local, "draft.txt", "real", 60);

    world.sync();
    let drive = tree(&world.drive);
    assert_eq!(drive, vec![("draft.txt".to_string(), "real".to_string())]);
}

#[test]
#[ignore = "needs rclone 1.66+"]
fn dry_run_changes_nothing() {
    let mut world = World::new();
    write(&world.local, "a.txt", "a", 60);
    let options = Options {
        dry_run: true,
        force: false,
    };

    let outcome = world.try_sync(options).unwrap();
    let Outcome::Preview { actions, violation } = outcome else {
        panic!("expected a preview, got {outcome:?}");
    };
    assert_eq!(actions, vec![SyncAction::Upload("a.txt".into())]);
    assert!(violation.is_none());
    assert!(tree(&world.drive).is_empty());
    assert!(world.store.base().unwrap().is_empty());
}
