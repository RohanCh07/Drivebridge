//! What the daemon tells the desktop about each sync: a state, a message for people, and the
//! events worth a signal straight away.

use std::error::Error as _;

use drivebridge_core::SyncAction;
use drivebridge_engine::{Error, Outcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Starting,
    Idle,
    Syncing,
    Paused,
    Blocked,
    Offline,
    Failed,
}

impl State {
    /// The name sent over D-Bus.
    pub fn as_str(self) -> &'static str {
        match self {
            State::Starting => "starting",
            State::Idle => "idle",
            State::Syncing => "syncing",
            State::Paused => "paused",
            State::Blocked => "blocked",
            State::Offline => "offline",
            State::Failed => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    pub message: String,
}

impl Status {
    pub fn new(state: State, message: impl Into<String>) -> Self {
        Self {
            state,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A sync was stopped by the safety rules; nothing changed.
    Blocked(String),
    /// Files changed on both sides; both copies were kept.
    Conflicts(Vec<String>),
}

pub struct Report {
    /// False when the sync failed and should be retried with backoff.
    pub ok: bool,
    pub status: Status,
    pub events: Vec<Event>,
}

pub fn report(result: &drivebridge_engine::Result<Outcome>) -> Report {
    let ok = |status| Report {
        ok: true,
        status,
        events: Vec::new(),
    };
    match result {
        Ok(Outcome::Synced { actions, .. }) => {
            let changes = actions
                .iter()
                .filter(|a| !matches!(a, SyncAction::Skip(_)))
                .count();
            let conflicts: Vec<String> = actions
                .iter()
                .filter(|a| matches!(a, SyncAction::Conflict(_)))
                .map(|a| a.path().to_string_lossy().into_owned())
                .collect();
            let message = format!("Synced {changes} change(s)");
            let mut report = ok(Status::new(State::Idle, message));
            if !conflicts.is_empty() {
                report.events.push(Event::Conflicts(conflicts));
            }
            report
        }
        Ok(Outcome::UpToDate | Outcome::Preview { .. }) => {
            ok(Status::new(State::Idle, "Up to date"))
        }
        Ok(Outcome::Busy) => ok(Status::new(State::Idle, "Another sync is running")),
        Ok(Outcome::Blocked { violation, .. }) => {
            let message = format!("Sync blocked: {violation}. Nothing was changed.");
            let mut report = ok(Status::new(State::Blocked, message.clone()));
            report.events.push(Event::Blocked(message));
            report
        }
        Err(Error::Offline(_)) => Report {
            ok: false,
            status: Status::new(State::Offline, "Drive is unreachable; will retry"),
            events: Vec::new(),
        },
        Err(e) => Report {
            ok: false,
            status: Status::new(State::Failed, describe(e)),
            events: Vec::new(),
        },
    }
}

/// An error and its causes, like `{:#}` on an anyhow error.
pub fn describe(error: &Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use drivebridge_core::SafetyViolation;

    #[test]
    fn conflicts_are_reported_as_an_event() {
        let actions = vec![
            SyncAction::Upload("a.txt".into()),
            SyncAction::Conflict("notes.txt".into()),
            SyncAction::Skip("b.txt".into()),
        ];
        let result = Ok(Outcome::Synced {
            actions,
            overridden: None,
        });
        let r = report(&result);
        assert!(r.ok);
        assert_eq!(r.status, Status::new(State::Idle, "Synced 2 change(s)"));
        let expected = Event::Conflicts(vec!["notes.txt".into()]);
        assert_eq!(r.events, vec![expected]);
    }

    #[test]
    fn a_blocked_sync_waits_for_the_user() {
        let violation = SafetyViolation::TooManyDeletes {
            count: 500,
            limit: 50,
        };
        let result = Ok(Outcome::Blocked {
            violation,
            actions: Vec::new(),
        });
        let r = report(&result);
        assert!(r.ok, "blocked is not retried sooner");
        assert_eq!(r.status.state, State::Blocked);
        assert!(matches!(&r.events[..], [Event::Blocked(m)] if m.contains("500")));
    }

    #[test]
    fn offline_is_retried_with_backoff() {
        let cause = drivebridge_rclone::Error::Transport("connection refused".into());
        let r = report(&Err(Error::Offline(cause)));
        assert!(!r.ok);
        assert_eq!(r.status.state, State::Offline);
        assert!(r.events.is_empty());
    }

    #[test]
    fn errors_include_their_causes() {
        let cause = drivebridge_rclone::Error::Transport("connection refused".into());
        let text = describe(&Error::Offline(cause));
        assert!(text.ends_with("could not reach rclone: connection refused"), "{text}");
    }
}
