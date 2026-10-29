# Drivebridge

Open-source two-way Google Drive sync for Linux: a Rust service with bundled rclone.

**Status:** pre-alpha. Nothing syncs yet.

## Build

You need Rust (stable, via [rustup](https://rustup.rs)), plus `pkg-config` and `libsecret-1-dev`.

```sh
cargo build
cargo test                        # unit and scenario tests
cargo test -- --include-ignored   # also starts a real rclone (needs rclone on PATH)
```

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
| `crates/watcher` | Filesystem watcher |
| `crates/daemon` | `drivebridged` background service |
| `crates/cli` | `drivebridge` command-line tool |
| `packaging/` | systemd user unit and distro packaging |
| `tests/scenarios` | End-to-end sync scenarios |

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
