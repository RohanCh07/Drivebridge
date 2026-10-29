//! Starts a real `rclone rcd`. Needs rclone on PATH: `cargo test -- --include-ignored`.

use std::fs;
use std::path::Path;

use drivebridge_rclone::Rcd;

#[test]
#[ignore = "needs rclone on PATH"]
fn lists_a_local_folder_through_rcd() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("hello.txt"), "hi").unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();

    let rcd = Rcd::start(Path::new("rclone"), &[]).unwrap();
    assert!(rcd.client().version().unwrap().starts_with('v'));

    let mut entries: Vec<_> = rcd
        .client()
        .list(dir.path().to_str().unwrap())
        .unwrap()
        .into_iter()
        .map(|item| (item.name, item.is_dir))
        .collect();
    entries.sort();
    assert_eq!(
        entries,
        vec![("hello.txt".to_string(), false), ("sub".to_string(), true)]
    );
}
