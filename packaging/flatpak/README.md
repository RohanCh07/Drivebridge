# Flatpak

The manifest builds all three programs offline and bundles rclone 1.71.1 (checksums from
`https://downloads.rclone.org/v1.71.1/SHA256SUMS`).

1. Generate the vendored crate list from `Cargo.lock` (re-run whenever dependencies change):

   ```sh
   pip install aiohttp toml
   curl -LO https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
   python3 flatpak-cargo-generator.py ../../Cargo.lock -o cargo-sources.json
   ```

2. Build and install:

   ```sh
   flatpak install flathub org.gnome.Platform//46 org.gnome.Sdk//46 \
       org.freedesktop.Sdk.Extension.rust-stable//23.08
   flatpak-builder --user --install --force-clean build io.github.Drivebridge.App.yml
   flatpak run io.github.Drivebridge.App
   ```

Inside the sandbox there is no systemd user unit. The app starts `drivebridged` itself when it
isn't running, and asks the Background portal to start it at every login.
