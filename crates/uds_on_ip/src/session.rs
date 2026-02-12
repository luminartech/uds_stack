//! UDS session management over DoIP.

use std::time::Duration;

/// Configuration for UDS session behavior.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Interval for sending tester present messages to keep the session alive.
    /// Default is 2 seconds.
    pub tester_present_interval: Duration,

    /// Timeout for waiting for a response to a diagnostic request.
    /// Default is 5 seconds (P2 server max).
    pub response_timeout: Duration,

    /// Extended timeout for responses after receiving a "response pending" NRC.
    /// Default is 25 seconds (P2* server max).
    pub response_pending_timeout: Duration,

    /// Whether to automatically send tester present messages.
    /// Default is true.
    pub auto_tester_present: bool,

    /// Whether to automatically reconnect on connection failures.
    /// Default is true.
    pub auto_reconnect: bool,

    /// Total time to spend attempting reconnection.
    /// Default is 30 seconds. Each reconnection attempt may take up to ~6 seconds
    /// (due to simple_doip's internal 5-second wait for in-flight messages).
    /// Must be long enough to cover sensor reboot times (typically 5-10 seconds)
    /// plus successful reconnection overhead.
    pub reconnect_timeout: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            tester_present_interval: Duration::from_secs(2),
            response_timeout: Duration::from_secs(5),
            response_pending_timeout: Duration::from_secs(25),
            auto_tester_present: true,
            auto_reconnect: true,
            reconnect_timeout: Duration::from_secs(30),
        }
    }
}

impl SessionConfig {
    /// Create a new session configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the tester present interval.
    pub fn with_tester_present_interval(mut self, interval: Duration) -> Self {
        self.tester_present_interval = interval;
        self
    }

    /// Set the response timeout.
    pub fn with_response_timeout(mut self, timeout: Duration) -> Self {
        self.response_timeout = timeout;
        self
    }

    /// Set the response pending timeout.
    pub fn with_response_pending_timeout(mut self, timeout: Duration) -> Self {
        self.response_pending_timeout = timeout;
        self
    }

    /// Enable or disable automatic tester present messages.
    pub fn with_auto_tester_present(mut self, enabled: bool) -> Self {
        self.auto_tester_present = enabled;
        self
    }

    /// Enable or disable automatic reconnection on connection failures.
    pub fn with_auto_reconnect(mut self, enabled: bool) -> Self {
        self.auto_reconnect = enabled;
        self
    }

    /// Set the total time to spend attempting reconnection.
    /// Each reconnection attempt may take up to ~6 seconds.
    pub fn with_reconnect_timeout(mut self, timeout: Duration) -> Self {
        self.reconnect_timeout = timeout;
        self
    }
}
