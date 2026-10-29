# Architecture

Drivebridge is one Cargo workspace. The rule that shapes it: **`core` decides what to do; the
other crates only carry it out.** Every safety decision is in code that can be unit tested
without a network, a desktop or a Google account.

## One sync run

`drivebridge_engine::sync` is the only way a sync happens, whether the CLI, the daemon or the
app asks for it.

1. **Lock.** Take `sync.lock` (an flock in the data folder). If another sync holds it, return
   `Busy` without waiting. Holding it also means any bisync lock file left in the work folder
   is from a killed run, so it is removed.
2. **List** the local folder and the Drive folder through a private `rclone rcd`, started on
   localhost with random credentials passed in the environment. The Drive remote itself is
   defined in memory from the keyring, so the token is never written to an rclone config file.
   If the Drive listing fails, the run stops with `Offline`.
3. **Plan.** `core::plan(local, remote, base, ignore)` compares each file with the *base*: the
   state both sides agreed on after the last run, kept in SQLite. Changed on one side means copy
   it across; changed on both sides means a conflict; gone on one side and unchanged on the
   other means delete. When in doubt it keeps data: an edit beats a delete.
4. **Check.** `SafetyPolicy::check_plan` rejects plans that delete more than 50 files or more
   than 10% of the base. A rejected plan returns `Blocked` and changes nothing. `--force`
   overrides it for one run.
5. **Carry out** with `rclone bisync`, with `--backup-dir1` pointing at a new trash folder so
   local deletions and overwrites are moved, not erased, and conflicts resolved as "newer keeps
   the name, the other is renamed beside it".
6. **Record** the files that now match on both sides as the new base, and the actions in the
   history table.

## Why planning is a pure function

`core::plan` takes three snapshots and returns a list of actions. It does no I/O, so:

- every rule has a direct unit test (`crates/core/src/planner.rs`);
- the data-loss scenarios can be checked against the planner in microseconds
  (`crates/core/tests/scenarios.rs`) before they are run end to end with rclone
  (`crates/engine/tests/scenarios.rs`);
- the safety check sees the whole plan before anything happens, which is what makes a
  mass-delete guard possible at all.

## The background service

`drivebridged` runs as a systemd user service (in a Flatpak, the app starts it and asks the
Background portal for autostart). Its main loop waits on one channel for local changes (from
inotify) and requests from D-Bus, and a scheduler decides when to run: three seconds after local
changes settle but at most thirty after the first, every five minutes to catch changes made on
Drive, and with backoff from thirty seconds to thirty minutes after failures. A blocked plan
waits for the user instead of retrying.

It publishes its state on the session bus as `io.github.Drivebridge` (methods `Status`,
`SyncNow`, `Pause`, `Resume`, `RecentActivity`; signals `StatusChanged`, `Blocked`,
`Conflicts`), shows a tray icon where the desktop has one, and sends a notification the first
time a sync is blocked and whenever files conflict.

## Google Docs

Google Docs have no file to sync. bisync runs with `--drive-skip-gdocs`; after each run the
engine lists the folder again with Docs exported as freedesktop links and copies new links down.
Links are recorded with the exact content rclone wrote, excluded from uploads, and removed only
if the document is gone and the link is unchanged. A file of the user's own with the same name
is never overwritten.
