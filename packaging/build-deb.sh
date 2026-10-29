#!/bin/sh
# Builds target/debian/drivebridge_<version>_<arch>.deb with all three programs.
# Needs cargo-deb: cargo install cargo-deb --locked
set -eu
cd "$(dirname "$0")/.."
cargo build --release --workspace
cargo deb -p drivebridge-gui --no-build
