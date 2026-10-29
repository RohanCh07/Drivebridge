//! Data-loss scenarios from the roadmap, run against the pure planner and safety check.
//! The same scenarios run end to end against a fake remote in `tests/scenarios` later.

use std::path::PathBuf;

use drivebridge_core::{
    check_plan, plan, FileMeta, IgnoreRules, SafetyViolation, Snapshot, SyncAction,
};

fn file(size: u64, mtime: i64) -> FileMeta {
    FileMeta {
        size,
        mtime,
        hash: None,
    }
}

fn tree(n: usize) -> Snapshot {
    (0..n)
        .map(|i| (PathBuf::from(format!("dir/file{i}.txt")), file(100, 1_000)))
        .collect()
}

fn run(local: &Snapshot, remote: &Snapshot, base: &Snapshot) -> Vec<SyncAction> {
    plan(local, remote, base, &IgnoreRules::new())
}

#[test]
fn same_file_edited_on_both_sides_keeps_both() {
    let base = Snapshot::from([("notes.txt".into(), file(100, 1_000))]);
    let local = Snapshot::from([("notes.txt".into(), file(120, 2_000))]);
    let remote = Snapshot::from([("notes.txt".into(), file(130, 2_100))]);

    assert_eq!(
        run(&local, &remote, &base),
        vec![SyncAction::Conflict("notes.txt".into())]
    );
}

#[test]
fn local_folder_deleted_while_daemon_stopped_is_blocked() {
    let base = tree(500);
    let remote = tree(500);
    let local = Snapshot::new();

    let actions = run(&local, &remote, &base);
    assert_eq!(actions.len(), 500);
    assert!(actions
        .iter()
        .all(|a| matches!(a, SyncAction::DeleteRemote(_))));
    assert_eq!(
        check_plan(&actions, base.len()),
        Err(SafetyViolation::TooManyDeletes { count: 500, limit: 50 })
    );
}

#[test]
fn empty_drive_listing_never_wipes_local_files() {
    // What we'd see if Drive returned nothing because it was unreachable.
    let base = tree(20);
    let local = tree(20);
    let remote = Snapshot::new();

    let actions = run(&local, &remote, &base);
    assert!(matches!(
        check_plan(&actions, base.len()),
        Err(SafetyViolation::DeleteRatioExceeded { .. })
    ));
}

#[test]
fn file_deleted_on_drive_is_removed_locally_unless_edited() {
    let base = Snapshot::from([
        ("a.txt".into(), file(10, 100)),
        ("b.txt".into(), file(10, 100)),
    ]);
    let local = Snapshot::from([
        ("a.txt".into(), file(10, 100)),
        ("b.txt".into(), file(25, 500)),
    ]);
    let remote = Snapshot::new();

    assert_eq!(
        run(&local, &remote, &base),
        vec![
            SyncAction::DeleteLocal("a.txt".into()),
            SyncAction::Upload("b.txt".into()),
        ]
    );
}

#[test]
fn edit_on_drive_survives_local_delete() {
    let base = Snapshot::from([("report.pdf".into(), file(10, 100))]);
    let remote = Snapshot::from([("report.pdf".into(), file(40, 900))]);
    let local = Snapshot::new();

    assert_eq!(
        run(&local, &remote, &base),
        vec![SyncAction::Download("report.pdf".into())]
    );
}

#[test]
fn ignored_files_are_never_uploaded() {
    let local = Snapshot::from([
        (".git/HEAD".into(), file(1, 1)),
        ("app/node_modules/x/index.js".into(), file(1, 1)),
        ("doc.txt~".into(), file(1, 1)),
        ("doc.txt".into(), file(1, 1)),
    ]);

    assert_eq!(
        run(&local, &Snapshot::new(), &Snapshot::new()),
        vec![SyncAction::Upload("doc.txt".into())]
    );
}
