# Drivebridge

Open-source two-way Google Drive sync for Linux: a Rust service with bundled rclone.

**Status:** pre-alpha. Nothing syncs yet.

## Build

You need Rust 1.89+ (via [rustup](https://rustup.rs)), `pkg-config`, `libdbus-1-dev` (for the
system keyring), `libgtk-4-dev` and `libadwaita-1-dev` (for the desktop app), and rclone 1.66 or
newer (`curl https://rclone.org/install.sh | sudo bash`).

```sh
cargo build
cargo test                        # unit and scenario tests
cargo test -- --include-ignored   # also starts a real rclone (needs rclone on PATH)
```

## Use it

You need your own Google OAuth client: create a project in Google Cloud Console, enable the
Drive API, and create an OAuth client of type **Desktop app**. Download its JSON file.

```sh
drivebridge setup --client-secret ~/Downloads/client_secret_XXXX.json
drivebridge sync --dry-run   # preview
drivebridge sync
drivebridge status
drivebridge restore          # list files removed or overwritten locally
drivebridge restore notes/todo.txt
```

By default `~/GoogleDrive` syncs with a Drive folder named `Drivebridge`; change them with
`--local` and `--drive-folder`. Your Google token is kept in the system keyring, never in a file.

Safety rules:

- Every sync is planned first. If it would delete more than 50 files or 10% of your files, it
  stops and changes nothing; use `--force` once you've checked the plan.
- If Drive can't be listed, the sync stops. An unreachable Drive is never read as "everything
  was deleted".
- Local files that a sync deletes or overwrites go to a trash folder for 30 days
  (`drivebridge restore`). Files deleted on Drive go to Drive's own trash.

Google Docs, Sheets and Slides appear locally as `.desktop` links that open them in the browser.
They are never uploaded back as copies.

## Desktop app

`drivebridge-gui` walks you through setup on first run (your OAuth client file, the two folders,
then Google sign-in), and afterwards shows what the background service is doing, with Sync now,
Pause/Resume and a "Recently removed" list to restore files from.

```sh
cargo install --path crates/gui --root ~/.local
cp packaging/io.github.Drivebridge.App.desktop ~/.local/share/applications/
```

## Run it in the background

`drivebridged` syncs at startup, a few seconds after local changes settle, and every five
minutes to pick up changes made on Drive. It honours `drivebridge pause`.

```sh
cargo install --path crates/daemon --root ~/.local
mkdir -p ~/.config/systemd/user
cp packaging/drivebridged.service ~/.config/systemd/user/
systemctl --user enable --now drivebridged
journalctl --user -u drivebridged -f    # watch what it does
```

Restart it after running `drivebridge setup` again.

It also shows a tray icon (with Sync now, Pause/Resume and Open Drivebridge) on desktops that
support one, and a notification when a sync is stopped for safety or a file changed on both
sides. Set `DRIVEBRIDGE_NO_TRAY=1` to hide the icon.

## Try the rclone spike

Set up a Drive remote once with `rclone config` (call it `gdrive`), then:

```sh
cargo run -p drivebridge-cli -- ls gdrive:
cargo run -p drivebridge-cli -- ls gdrive:Documents
```

`drivebridge` starts a private `rclone rcd` on localhost with random credentials, lists the
folder through rclone's remote-control API, and shuts it down.

Set `DRIVEBRIDGE_RCLONE=/path/to/rclone` to use a specific rclone binary.

## Layout

| Path | What it is |
| --- | --- |
| `crates/core` | Sync plan types and safety rules (mass-delete guard) |
| `crates/store` | Local state database |
| `crates/rclone` | rclone wrapper |
| `crates/engine` | One sync run: list, plan, check, bisync, record; scenario tests |
| `crates/watcher` | Filesystem watcher |
| `crates/daemon` | `drivebridged` background service |
| `crates/cli` | `drivebridge` command-line tool |
| `crates/gui` | `drivebridge-gui` desktop app (GTK4 + libadwaita) |
| `packaging/` | systemd user unit and distro packaging |

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
