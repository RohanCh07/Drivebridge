# Contributing

Thanks for helping. Drivebridge handles people's files, so the bar is: no change may make data
loss more likely, and every safety-relevant change comes with a test.

## Getting started

```sh
sudo apt install pkg-config libdbus-1-dev libgtk-4-dev libadwaita-1-dev dbus
curl https://rclone.org/install.sh | sudo bash    # rclone 1.66+
cargo build
dbus-run-session -- cargo test -- --include-ignored
```

The ignored tests start a real rclone and a D-Bus session; they run in CI too.

## Where things go

- A rule about what to sync, delete or keep belongs in `crates/core`, as a pure function with
  unit tests. Don't add I/O there.
- A new way to lose data is a scenario: add it to `crates/engine/tests/scenarios.rs` (and to the
  table in the README) before or with the fix.
- Never test against your real Google account. Use a throwaway account for anything that needs
  real Drive.

## Before opening a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
dbus-run-session -- cargo test --workspace -- --include-ignored
```

Keep commits focused, and describe in the pull request what changes for someone using the app.
By contributing you agree that your work is licensed under GPL-3.0-or-later.
