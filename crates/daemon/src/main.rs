//! drivebridged: the background sync service.

fn main() {
    let policy = drivebridge_core::SafetyPolicy::default();
    println!(
        "drivebridged {} (pre-alpha): not implemented yet (delete guard: {} files / {:.0}%)",
        env!("CARGO_PKG_VERSION"),
        policy.max_delete_count,
        policy.max_delete_ratio * 100.0
    );
}
