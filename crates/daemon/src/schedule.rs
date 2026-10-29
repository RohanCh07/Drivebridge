//! When to sync: soon after local changes settle, regularly to catch changes made on Drive,
//! and with growing gaps after failures.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct Schedule {
    /// Wait this long after the last local change, so a burst of writes syncs once.
    pub quiet: Duration,
    /// But never hold a local change back longer than this.
    pub max_delay: Duration,
    /// Check Drive this often.
    pub poll: Duration,
    /// First retry after a failure; doubles each time, up to `retry_max`.
    pub retry_min: Duration,
    pub retry_max: Duration,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            quiet: Duration::from_secs(3),
            max_delay: Duration::from_secs(30),
            poll: Duration::from_secs(5 * 60),
            retry_min: Duration::from_secs(30),
            retry_max: Duration::from_secs(30 * 60),
        }
    }
}

pub struct Scheduler {
    schedule: Schedule,
    next_poll: Instant,
    first_change: Option<Instant>,
    last_change: Option<Instant>,
    failures: u32,
}

impl Scheduler {
    /// The first sync is due straight away.
    pub fn new(schedule: Schedule, now: Instant) -> Self {
        Self {
            schedule,
            next_poll: now,
            first_change: None,
            last_change: None,
            failures: 0,
        }
    }

    pub fn local_change(&mut self, now: Instant) {
        if self.first_change.is_none() {
            self.first_change = Some(now);
        }
        self.last_change = Some(now);
    }

    pub fn next_wake(&self) -> Instant {
        match (self.first_change, self.last_change) {
            (Some(first), Some(last)) => {
                let settled = last + self.schedule.quiet;
                let deadline = first + self.schedule.max_delay;
                settled.min(deadline).min(self.next_poll)
            }
            _ => self.next_poll,
        }
    }

    /// Someone asked for a sync (the desktop app, or resume after a pause).
    pub fn sync_now(&mut self, now: Instant) {
        self.next_poll = now;
    }

    pub fn due(&self, now: Instant) -> bool {
        now >= self.next_wake()
    }

    /// Records a sync attempt. `ok` is false when it should be retried with backoff.
    pub fn finished(&mut self, now: Instant, ok: bool) {
        self.first_change = None;
        self.last_change = None;
        if ok {
            self.failures = 0;
            self.next_poll = now + self.schedule.poll;
        } else {
            self.failures += 1;
            self.next_poll = now + self.backoff();
        }
    }

    fn backoff(&self) -> Duration {
        let doublings = self.failures.saturating_sub(1).min(16);
        let delay = self.schedule.retry_min.saturating_mul(1u32 << doublings);
        delay.min(self.schedule.retry_max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn synced_at(t0: Instant) -> Scheduler {
        let mut s = Scheduler::new(Schedule::default(), t0);
        s.finished(t0, true);
        s
    }

    #[test]
    fn first_sync_is_immediate() {
        let t0 = Instant::now();
        assert!(Scheduler::new(Schedule::default(), t0).due(t0));
    }

    #[test]
    fn idle_waits_for_the_poll() {
        let t0 = Instant::now();
        let s = synced_at(t0);
        assert!(!s.due(t0 + secs(299)));
        assert!(s.due(t0 + secs(300)));
    }

    #[test]
    fn local_change_syncs_once_things_settle() {
        let t0 = Instant::now();
        let mut s = synced_at(t0);
        s.local_change(t0 + secs(10));
        assert!(!s.due(t0 + secs(12)));
        assert!(s.due(t0 + secs(13)));
    }

    #[test]
    fn constant_changes_still_sync_within_max_delay() {
        let t0 = Instant::now();
        let mut s = synced_at(t0);
        for second in (1..=29).step_by(2) {
            s.local_change(t0 + secs(second));
        }
        assert!(!s.due(t0 + secs(30)));
        assert!(s.due(t0 + secs(31)));
    }

    #[test]
    fn sync_now_is_due_immediately() {
        let t0 = Instant::now();
        let mut s = synced_at(t0);
        s.sync_now(t0 + secs(5));
        assert!(s.due(t0 + secs(5)));
    }

    #[test]
    fn failures_back_off_up_to_the_cap() {
        let t0 = Instant::now();
        let mut s = Scheduler::new(Schedule::default(), t0);
        s.finished(t0, false);
        assert_eq!(s.next_wake(), t0 + secs(30));
        s.finished(t0, false);
        assert_eq!(s.next_wake(), t0 + secs(60));
        for _ in 0..20 {
            s.finished(t0, false);
        }
        assert_eq!(s.next_wake(), t0 + secs(30 * 60));
        s.finished(t0, true);
        assert_eq!(s.next_wake(), t0 + secs(300));
    }
}
