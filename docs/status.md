# Status

Last updated 2026-11-11. Everything below is written and passes its tests on a Linux build
machine (Rust 1.97, rclone 1.71.1): **101 tests**, including **13 end-to-end data-loss
scenarios** run with a real `rclone bisync` against a stand-in Drive folder, and the D-Bus test
under `dbus-run-session`. Nothing has run against a real Google Drive yet.

## Done

### Phase 0: Foundations

- Cargo workspace (`core`, `store`, `rclone`, `engine`, `watcher`, `daemon`, `cli`, `gui`),
  GPL-3.0-or-later, CI with `cargo fmt`, `clippy -D warnings` and tests
- A private `rclone rcd` on localhost with random credentials, driven through its rc API
  (`drivebridge ls`)

### Phase 1: CLI prototype

- `drivebridge setup`: your own Google OAuth client, browser sign-in, token in the system
  keyring (never in a file)
- `drivebridge sync`, `status`, `pause`, `resume`, `restore`
- SQLite store for settings, the last agreed state and history
- 30-day local trash for files a sync deletes or overwrites

### Phase 2: Safety layer and background service

- Every sync is planned first by a pure function, then checked: more than 50 deletes or more
  than 10% of files blocks the sync; `--dry-run` previews; `--force` overrides once
- Conflicts keep both copies (the newer keeps the name, the other becomes `name.conflict1.ext`)
- An unreachable Drive is offline, never "everything was deleted"
- One sync at a time (a shared lock), and recovery from killed runs and lost sync state
- Ignore rules (`.git`, `node_modules`, editor temp files)
- Google Docs, Sheets and Slides as `.desktop` links, never uploaded back
- `drivebridged`: a systemd user service with inotify, debouncing, a five-minute poll for Drive
  changes and backoff after failures

### Phase 3: Desktop app

- D-Bus interface `io.github.Drivebridge` (status, sync now, pause, resume, recent activity, and
  signals for status changes, blocked syncs and conflicts)
- `drivebridge-gui` (GTK4 + libadwaita): setup wizard, status window, "Recently removed" with
  restore
- Tray icon and desktop notifications

### Phase 4: Packaging and docs

- `.deb` with all three programs, the user unit, launcher, AppStream metainfo and icon; built
  and installed cleanly; release workflow builds it on version tags
- Flatpak manifest (GNOME 46, rclone 1.71.1 bundled with verified checksums)
- README, [architecture](architecture.md), [Google setup guide](google-setup.md),
  [CONTRIBUTING](../CONTRIBUTING.md)

## Still open

- [ ] Commit `Cargo.lock` (created by the first `cargo build`)
- [ ] Test against a real Google Drive with a throwaway account (setup, sync, conflict, a
      blocked mass delete, restore, Google Docs links)
- [ ] Build the Flatpak on a real machine: generate `packaging/flatpak/cargo-sources.json`
      from `Cargo.lock`, then `flatpak-builder` (see [packaging/flatpak](../packaging/flatpak/README.md))
- [ ] Screenshots in [docs/screenshots](screenshots/README.md)
- [ ] Two-minute demo video: install, sign in, sync, make a conflict, delete 200 files and watch
      it block, restore a file
- [ ] First tagged release (`v0.1.0`) so the release workflow publishes the `.deb`
