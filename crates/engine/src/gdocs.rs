//! Google Docs, Sheets and Slides as `.desktop` link files in the local folder.
//!
//! bisync never sees them: rclone runs with `--drive-skip-gdocs`. After each sync we list the
//! Drive folder again with Google Docs shown as freedesktop links, copy new ones down, and
//! remove links whose document is gone. Our links are excluded from uploads. We never touch a
//! file we did not write: an existing file blocks a link, and a link the user edited stays.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use drivebridge_rclone::{ListItem, RcClient};
use drivebridge_store::Store;

use crate::{Error, Result};

const SETTING: &str = "gdoc_links";
/// rclone's MIME type for a Google Doc exported as a freedesktop link.
const LINK_MIME: &str = "application/x-link-desktop";

/// Our links: path relative to the sync folder, and the content rclone wrote.
pub type Links = BTreeMap<PathBuf, String>;

/// The same Drive folder as `remote_root`, with Google Docs listed as `.desktop` links.
/// `None` for a plain local folder (as in tests), which has no Google Docs.
pub fn view(remote_root: &str) -> Option<String> {
    let (remote, path) = remote_root.split_once(':')?;
    if remote.is_empty() || remote.contains('/') {
        return None;
    }
    Some(format!(
        "{remote},skip_gdocs=false,export_formats=desktop:{path}"
    ))
}

/// The links we created, as recorded in the store.
pub fn tracked(store: &Store) -> Result<Links> {
    let saved = store.setting(SETTING)?;
    let links: BTreeMap<String, String> = saved
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    Ok(links
        .into_iter()
        .map(|(path, content)| (PathBuf::from(path), content))
        .collect())
}

fn save(store: &Store, links: &Links) -> Result<()> {
    let links: BTreeMap<String, &String> = links
        .iter()
        .map(|(path, content)| (path.to_string_lossy().into_owned(), content))
        .collect();
    let json = serde_json::to_string(&links).expect("a map of strings always serializes");
    store.set_setting(SETTING, &json)?;
    Ok(())
}

/// Paths of Google Docs in a listing of [`view`].
pub fn on_drive(items: &[ListItem]) -> BTreeSet<PathBuf> {
    items
        .iter()
        .filter(|item| !item.is_dir && item.mime_type == LINK_MIME)
        .map(|item| PathBuf::from(&item.path))
        .collect()
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LinkChanges {
    /// Links to copy down from Drive.
    pub create: Vec<PathBuf>,
    /// Our unchanged links whose document is gone.
    pub remove: Vec<PathBuf>,
    /// Docs whose link path is taken by a file we did not write.
    pub blocked: Vec<PathBuf>,
    /// Links that stay tracked as they are.
    pub keep: Links,
}

/// Decides what to change. `local` reads a local file, `None` if it does not exist.
pub fn changes(
    on_drive: &BTreeSet<PathBuf>,
    tracked: &Links,
    local: impl Fn(&Path) -> Option<String>,
) -> LinkChanges {
    let mut changes = LinkChanges::default();
    for path in on_drive {
        match (tracked.get(path), local(path)) {
            (_, None) => changes.create.push(path.clone()),
            // Ours, edited or not: keep it out of uploads, but don't overwrite it.
            (Some(recorded), Some(_)) => {
                changes.keep.insert(path.clone(), recorded.clone());
            }
            (None, Some(_)) => changes.blocked.push(path.clone()),
        }
    }
    for (path, recorded) in tracked {
        if on_drive.contains(path) {
            continue;
        }
        // Edited links become ordinary files; missing ones are just forgotten.
        if local(path).as_ref() == Some(recorded) {
            changes.remove.push(path.clone());
        }
    }
    changes
}

/// Brings the local links in line with the Google Docs in the Drive folder.
pub fn refresh(
    client: &RcClient,
    store: &Store,
    remote_root: &str,
    local_root: &str,
) -> Result<()> {
    let Some(view) = view(remote_root) else {
        return Ok(());
    };
    let root = Path::new(local_root);
    let read = |path: &Path| {
        let bytes = fs::read(root.join(path)).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    };
    let items = client.list_recursive_typed(&view)?;
    let LinkChanges {
        create,
        remove,
        blocked,
        mut keep,
    } = changes(&on_drive(&items), &tracked(store)?, read);

    for path in &blocked {
        eprintln!(
            "not linking Google Doc {}: a file with that name already exists",
            path.display()
        );
    }
    for path in create {
        let remote = path.to_string_lossy();
        client.copy_file(&view, &remote, local_root, &remote)?;
        let written = read(&path).ok_or_else(|| Error::Links(missing(&path)))?;
        keep.insert(path, written);
    }
    for path in &remove {
        match fs::remove_file(root.join(path)) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::Links(e)),
        }
    }
    save(store, &keep)
}

fn missing(path: &Path) -> io::Error {
    let message = format!("{} was not written", path.display());
    io::Error::new(io::ErrorKind::NotFound, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str = "[Desktop Entry]\nType=Link\nURL=https://docs.google.com/x\n";

    fn paths(names: &[&str]) -> BTreeSet<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    fn links(names: &[&str]) -> Links {
        names
            .iter()
            .map(|n| (PathBuf::from(n), LINK.to_string()))
            .collect()
    }

    /// A local folder holding `files`, each with the given content.
    fn local<'a>(files: &'a [(&'a str, &'a str)]) -> impl Fn(&Path) -> Option<String> + 'a {
        move |path| {
            files
                .iter()
                .find(|(name, _)| Path::new(name) == path)
                .map(|(_, content)| content.to_string())
        }
    }

    #[test]
    fn view_shows_docs_on_a_drive_remote_only() {
        assert_eq!(
            view("drivebridge:Drivebridge").as_deref(),
            Some("drivebridge,skip_gdocs=false,export_formats=desktop:Drivebridge")
        );
        assert_eq!(view("/tmp/fake-drive"), None);
    }

    #[test]
    fn only_link_exports_count_as_docs() {
        let item = |path: &str, mime: &str| ListItem {
            path: path.into(),
            name: path.into(),
            size: 100,
            mime_type: mime.into(),
            mod_time: String::new(),
            is_dir: false,
            id: None,
        };
        let items = [
            item("Plan.desktop", LINK_MIME),
            item("launcher.desktop", "application/x-desktop"),
        ];
        assert_eq!(on_drive(&items), paths(&["Plan.desktop"]));
    }

    #[test]
    fn new_docs_are_linked_and_unchanged_links_kept() {
        let tracked = links(&["Old.desktop"]);
        let folder = [("Old.desktop", LINK)];
        let found = paths(&["Old.desktop", "New.desktop"]);
        let c = changes(&found, &tracked, local(&folder));
        assert_eq!(c.create, vec![PathBuf::from("New.desktop")]);
        assert_eq!(c.keep, links(&["Old.desktop"]));
        assert!(c.remove.is_empty() && c.blocked.is_empty());
    }

    #[test]
    fn a_users_file_is_never_overwritten() {
        let folder = [("Plan.desktop", "my own launcher")];
        let c = changes(&paths(&["Plan.desktop"]), &Links::new(), local(&folder));
        assert_eq!(c.blocked, vec![PathBuf::from("Plan.desktop")]);
        assert!(c.create.is_empty() && c.keep.is_empty());
    }

    #[test]
    fn links_to_removed_docs_go_only_if_unchanged() {
        let tracked = links(&["Gone.desktop", "Edited.desktop", "Missing.desktop"]);
        let folder = [("Gone.desktop", LINK), ("Edited.desktop", "user notes")];
        let c = changes(&BTreeSet::new(), &tracked, local(&folder));
        assert_eq!(c.remove, vec![PathBuf::from("Gone.desktop")]);
        assert!(c.keep.is_empty());
    }

    #[test]
    fn a_deleted_link_comes_back() {
        let tracked = links(&["Plan.desktop"]);
        let c = changes(&paths(&["Plan.desktop"]), &tracked, local(&[]));
        assert_eq!(c.create, vec![PathBuf::from("Plan.desktop")]);
    }

    #[test]
    fn tracked_links_round_trip_through_the_store() {
        let store = Store::open_in_memory().unwrap();
        assert!(tracked(&store).unwrap().is_empty());
        let saved = links(&["Docs/Plan.desktop"]);
        save(&store, &saved).unwrap();
        assert_eq!(tracked(&store).unwrap(), saved);
    }
}
