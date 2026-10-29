//! Everything the daemon shows on the desktop: D-Bus signals for the app, the tray icon, and
//! notifications. Each part is optional, and failures are logged, never fatal.

use notify_rust::Notification;

use crate::bus::Bus;
use crate::report::{Event, Status};
use crate::tray::TrayHandle;

/// How many file names a conflict notification lists before "and N more".
const NAMED: usize = 3;

pub struct Desktop {
    pub bus: Option<Bus>,
    pub tray: Option<TrayHandle>,
}

impl Desktop {
    pub fn status_changed(&self, status: &Status) {
        if let Some(bus) = &self.bus {
            if let Err(e) = bus.status_changed(status) {
                eprintln!("could not signal the desktop: {e}");
            }
        }
        if let Some(tray) = &self.tray {
            let status = status.clone();
            tray.update(move |t| t.status = status);
        }
    }

    pub fn event(&self, event: &Event) {
        if let Some(bus) = &self.bus {
            if let Err(e) = bus.event(event) {
                eprintln!("could not signal the desktop: {e}");
            }
        }
        // Notifications need a desktop session too.
        if self.bus.is_some() {
            notify(event);
        }
    }
}

fn notify(event: &Event) {
    let (summary, body, icon) = notification(event);
    let shown = Notification::new()
        .appname("Drivebridge")
        .summary(&summary)
        .body(&body)
        .icon(icon)
        .show();
    if let Err(e) = shown {
        eprintln!("could not show a notification: {e}");
    }
}

/// The summary, body and icon name for an event.
fn notification(event: &Event) -> (String, String, &'static str) {
    match event {
        Event::Blocked(message) => (
            "Sync stopped for safety".to_string(),
            format!("{message} Open Drivebridge to review it."),
            "dialog-warning",
        ),
        Event::Conflicts(paths) => {
            let summary = match paths.len() {
                1 => format!("{} was changed on both sides", paths[0]),
                n => format!("{n} files were changed on both sides"),
            };
            let mut body = "Both copies were kept: the newer one under the original name, \
                            the other beside it as name.conflict1."
                .to_string();
            if paths.len() > 1 {
                let named: Vec<&str> = paths.iter().take(NAMED).map(String::as_str).collect();
                body.push_str(&format!(" Files: {}", named.join(", ")));
                if paths.len() > NAMED {
                    body.push_str(&format!(" and {} more", paths.len() - NAMED));
                }
                body.push('.');
            }
            (summary, body, "dialog-information")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_conflict_names_the_file() {
        let event = Event::Conflicts(vec!["notes.txt".into()]);
        let (summary, body, _) = notification(&event);
        assert_eq!(summary, "notes.txt was changed on both sides");
        assert!(body.contains("Both copies were kept"));
    }

    #[test]
    fn many_conflicts_name_a_few() {
        let paths = (1..=5).map(|i| format!("f{i}.txt")).collect();
        let (summary, body, _) = notification(&Event::Conflicts(paths));
        assert_eq!(summary, "5 files were changed on both sides");
        assert!(body.ends_with("Files: f1.txt, f2.txt, f3.txt and 2 more."), "{body}");
    }

    #[test]
    fn a_blocked_sync_asks_for_review() {
        let event = Event::Blocked("Sync blocked: plan deletes 500 files.".into());
        let (summary, body, icon) = notification(&event);
        assert_eq!(summary, "Sync stopped for safety");
        assert!(body.starts_with("Sync blocked: plan deletes 500 files."));
        assert_eq!(icon, "dialog-warning");
    }
}
