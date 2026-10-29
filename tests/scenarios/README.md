# Scenario tests

The data-loss scenarios from the roadmap run in two places:

- `crates/core/tests/scenarios.rs`: against the pure planner and safety check. Fast, no rclone.
- `crates/engine/tests/scenarios.rs`: end to end, with a real `rclone bisync` between two local
  folders, one standing in for Google Drive. Needs rclone 1.66+:

  ```sh
  cargo test -p drivebridge-engine -- --include-ignored
  ```

Real-Drive end-to-end tests (`tests/e2e`, run by hand against a throwaway account) come later.
