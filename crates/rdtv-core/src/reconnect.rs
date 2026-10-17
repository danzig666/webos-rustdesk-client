//! Error classification and the unexpected-disconnect retry policy.
//!
//! Owner decision: after an *unexpected* loss, retry for up to 30 s with bounded backoff,
//! a visible countdown and Cancel; then show Retry. Intentional disconnects, cancellation
//! and the leave-app disconnect never retry. Authentication, key and permission failures
//! get their own state instead of blind retries.

use std::time::{Duration, Instant};

pub const RECONNECT_WINDOW: Duration = Duration::from_secs(30);
const BACKOFF: [u64; 5] = [1, 2, 4, 6, 8];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// DNS failure, ID server unreachable, timeout talking to the server.
    Server,
    /// Peer ID unknown to the server.
    IdNotFound,
    /// Peer known but offline.
    PeerOffline,
    /// Server key mismatch / licence problems.
    Key,
    /// Wrong or missing password, 2FA failure.
    Auth,
    /// Host refused the session or a permission.
    Refused,
    /// Could not decode or present the negotiated video.
    Codec,
    /// TV platform failure (decoder load, audio device, ...).
    Platform,
    /// Transport dropped after it was established.
    ConnectionLost,
    /// Anything else.
    Other,
}

impl FailureKind {
    /// Whether the automatic reconnect loop may retry this failure.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            FailureKind::Server | FailureKind::ConnectionLost | FailureKind::PeerOffline
        )
    }

    pub fn title(&self) -> &'static str {
        match self {
            FailureKind::Server => "Cannot reach the server",
            FailureKind::IdNotFound => "Unknown computer ID",
            FailureKind::PeerOffline => "Computer is offline",
            FailureKind::Key => "Server key problem",
            FailureKind::Auth => "Sign-in failed",
            FailureKind::Refused => "Connection refused by the computer",
            FailureKind::Codec => "Video could not be decoded",
            FailureKind::Platform => "TV playback problem",
            FailureKind::ConnectionLost => "Connection lost",
            FailureKind::Other => "Connection failed",
        }
    }
}

/// Classify an error string produced by the transport/login code (which keeps upstream's
/// wording) into a user-facing failure kind.
pub fn classify(err: &str) -> FailureKind {
    let e = err.to_ascii_lowercase();
    if e.contains("id does not exist") {
        FailureKind::IdNotFound
    } else if e.contains("offline") {
        FailureKind::PeerOffline
    } else if e.contains("key mismatch") || e.contains("key overuse") || e.contains("licen") {
        FailureKind::Key
    } else if e.contains("password") || e.contains("2fa") || e.contains("verification code") {
        FailureKind::Auth
    } else if e.contains("refused")
        || e.contains("not allowed")
        || e.contains("denied")
        || e.contains("rejected")
        || e.contains("no permission")
    {
        FailureKind::Refused
    } else if e.contains("decod") || e.contains("codec") {
        FailureKind::Codec
    } else if e.contains("ndl") || e.contains("audio device") || e.contains("platform") {
        FailureKind::Platform
    } else if e.contains("reset by the peer")
        || e.contains("connection reset")
        || e.contains("broken pipe")
        || e.contains("connection lost")
        || e.contains("eof")
    {
        FailureKind::ConnectionLost
    } else if e.contains("failed to connect")
        || e.contains("timeout")
        || e.contains("timed out")
        || e.contains("dns")
        || e.contains("resolve")
        || e.contains("lookup")
        || e.contains("network is unreachable")
    {
        FailureKind::Server
    } else {
        FailureKind::Other
    }
}

/// Bounded-backoff schedule for one unexpected loss.
#[derive(Debug, Clone)]
pub struct ReconnectPlan {
    started: Instant,
    attempt: usize,
}

impl ReconnectPlan {
    pub fn start(now: Instant) -> Self {
        ReconnectPlan {
            started: now,
            attempt: 0,
        }
    }

    /// When the next attempt should start, or `None` when the 30 s window is used up.
    pub fn next_attempt_at(&mut self, now: Instant) -> Option<Instant> {
        let delay = Duration::from_secs(BACKOFF[self.attempt.min(BACKOFF.len() - 1)]);
        let at = now.max(self.started) + delay;
        if at.duration_since(self.started) > RECONNECT_WINDOW {
            return None;
        }
        self.attempt += 1;
        Some(at)
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        RECONNECT_WINDOW.saturating_sub(now.saturating_duration_since(self.started))
    }

    pub fn attempts(&self) -> usize {
        self.attempt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification() {
        assert_eq!(classify("ID does not exist"), FailureKind::IdNotFound);
        assert_eq!(classify("Remote desktop is offline"), FailureKind::PeerOffline);
        assert_eq!(classify("Wrong Password"), FailureKind::Auth);
        assert_eq!(classify("Key mismatch"), FailureKind::Key);
        assert_eq!(classify("Reset by the peer"), FailureKind::ConnectionLost);
        assert_eq!(
            classify("Failed to connect to ID server: dns error"),
            FailureKind::Server
        );
        assert!(!FailureKind::Auth.retryable());
        assert!(FailureKind::ConnectionLost.retryable());
    }

    #[test]
    fn plan_is_bounded_to_window() {
        let t0 = Instant::now();
        let mut p = ReconnectPlan::start(t0);
        let mut now = t0;
        let mut n = 0;
        while let Some(at) = p.next_attempt_at(now) {
            assert!(at.duration_since(t0) <= RECONNECT_WINDOW);
            now = at + Duration::from_millis(500); // attempt itself takes time
            n += 1;
            assert!(n < 20);
        }
        assert!(n >= 4, "expected several attempts within the window, got {n}");
        assert_eq!(p.remaining(t0 + Duration::from_secs(40)), Duration::ZERO);
    }
}
