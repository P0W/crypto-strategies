//! Circuit Breaker pattern implementation for fault tolerance
//!
//! The circuit breaker prevents cascading failures by temporarily
//! stopping requests to a failing service.
//!
//! States:
//! - Closed: Normal operation, requests pass through
//! - Open: Service is failing, requests are rejected
//! - HalfOpen: Testing if service has recovered

use std::time::Duration;
use tokio::time::Instant;

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CircuitState {
    /// Normal operation - requests pass through
    #[default]
    Closed,
    /// Service is failing - requests are rejected immediately
    Open,
    /// Testing if service has recovered - limited requests allowed
    HalfOpen,
}

/// Configuration for the circuit breaker
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before opening the circuit
    pub failure_threshold: u32,
    /// Number of consecutive successes in HalfOpen state before closing
    pub success_threshold: u32,
    /// Duration to stay in Open state before transitioning to HalfOpen
    pub timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 2,
            timeout: Duration::from_secs(60),
        }
    }
}

impl CircuitBreakerConfig {
    /// Create a new configuration with custom failure threshold
    pub fn with_failure_threshold(mut self, threshold: u32) -> Self {
        self.failure_threshold = threshold;
        self
    }

    /// Create a new configuration with custom success threshold
    pub fn with_success_threshold(mut self, threshold: u32) -> Self {
        self.success_threshold = threshold;
        self
    }

    /// Create a new configuration with custom timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Circuit breaker for managing service failures
///
/// # Example
///
/// ```
/// use std::time::Duration;
/// use crypto_strategies::common::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
///
/// let config = CircuitBreakerConfig::default()
///     .with_failure_threshold(3)
///     .with_timeout(Duration::from_secs(30));
///
/// let mut cb = CircuitBreaker::new(config);
///
/// // Circuit starts closed
/// assert!(cb.can_attempt());
///
/// // Record failures
/// cb.record_failure();
/// cb.record_failure();
/// cb.record_failure();
///
/// // Circuit is now open
/// assert_eq!(cb.state(), CircuitState::Open);
/// ```
#[derive(Debug)]
pub struct CircuitBreaker {
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    config: CircuitBreakerConfig,
    last_failure_time: Option<Instant>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given configuration
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            config,
            last_failure_time: None,
        }
    }

    /// Create a circuit breaker with default configuration
    pub fn with_defaults() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    /// Get the current state of the circuit breaker
    pub fn state(&self) -> CircuitState {
        self.state
    }

    /// Check if a request attempt is allowed
    ///
    /// Returns `true` if the circuit is Closed or HalfOpen (and timeout has elapsed),
    /// `false` if the circuit is Open and timeout hasn't elapsed.
    pub fn can_attempt(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if timeout has elapsed
                if let Some(last_failure) = self.last_failure_time {
                    if last_failure.elapsed() >= self.config.timeout {
                        tracing::info!("Circuit breaker transitioning to HalfOpen state");
                        self.state = CircuitState::HalfOpen;
                        self.failure_count = 0;
                        self.success_count = 0;
                        true
                    } else {
                        false
                    }
                } else {
                    // No failure time recorded, shouldn't happen but allow attempt
                    true
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Record a successful operation
    ///
    /// In Closed state: resets failure count
    /// In HalfOpen state: increments success count, may close circuit
    pub fn record_success(&mut self) {
        match self.state {
            CircuitState::Closed => {
                self.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                self.success_count += 1;
                if self.success_count >= self.config.success_threshold {
                    tracing::info!("Circuit breaker closed after successful recovery");
                    self.state = CircuitState::Closed;
                    self.failure_count = 0;
                    self.success_count = 0;
                }
            }
            CircuitState::Open => {
                // Shouldn't happen, but ignore
            }
        }
    }

    /// Record a failed operation
    ///
    /// In Closed state: increments failure count, may open circuit
    /// In HalfOpen state: immediately reopens circuit
    pub fn record_failure(&mut self) {
        self.last_failure_time = Some(Instant::now());

        match self.state {
            CircuitState::Closed => {
                self.failure_count += 1;
                if self.failure_count >= self.config.failure_threshold {
                    tracing::warn!(
                        "Circuit breaker opened after {} failures",
                        self.failure_count
                    );
                    self.state = CircuitState::Open;
                }
            }
            CircuitState::HalfOpen => {
                tracing::warn!("Circuit breaker re-opened due to failure in HalfOpen state");
                self.state = CircuitState::Open;
                self.failure_count = 0;
                self.success_count = 0;
            }
            CircuitState::Open => {
                // Already open, just update last failure time
            }
        }
    }

    /// Reset the circuit breaker to its initial closed state
    pub fn reset(&mut self) {
        self.state = CircuitState::Closed;
        self.failure_count = 0;
        self.success_count = 0;
        self.last_failure_time = None;
    }

    /// Get current failure count
    pub fn failure_count(&self) -> u32 {
        self.failure_count
    }

    /// Get current success count (only meaningful in HalfOpen state)
    pub fn success_count(&self) -> u32 {
        self.success_count
    }

    /// Check if the circuit is open
    pub fn is_open(&self) -> bool {
        self.state == CircuitState::Open
    }

    /// Check if the circuit is closed
    pub fn is_closed(&self) -> bool {
        self.state == CircuitState::Closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_is_closed() {
        let cb = CircuitBreaker::with_defaults();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.is_closed());
        assert!(!cb.is_open());
    }

    #[test]
    fn test_closed_allows_attempts() {
        let mut cb = CircuitBreaker::with_defaults();
        assert!(cb.can_attempt());
    }

    #[test]
    fn test_failure_threshold_opens_circuit() {
        let config = CircuitBreakerConfig::default().with_failure_threshold(3);
        let mut cb = CircuitBreaker::new(config);

        // Record failures below threshold
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.failure_count(), 1);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.failure_count(), 2);

        // Third failure should open circuit
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(cb.is_open());
    }

    #[test]
    fn test_open_circuit_rejects_attempts() {
        let config = CircuitBreakerConfig::default()
            .with_failure_threshold(1)
            .with_timeout(Duration::from_secs(60));
        let mut cb = CircuitBreaker::new(config);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Immediately after opening, attempts should be rejected
        assert!(!cb.can_attempt());
    }

    #[test]
    fn test_success_resets_failure_count_in_closed_state() {
        let config = CircuitBreakerConfig::default().with_failure_threshold(3);
        let mut cb = CircuitBreaker::new(config);

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.failure_count(), 2);

        cb.record_success();
        assert_eq!(cb.failure_count(), 0);
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_half_open_to_closed_on_success() {
        let config = CircuitBreakerConfig::default()
            .with_failure_threshold(1)
            .with_success_threshold(2)
            .with_timeout(Duration::from_millis(1));
        let mut cb = CircuitBreaker::new(config);

        // Open the circuit
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for timeout
        std::thread::sleep(Duration::from_millis(5));

        // Check attempt - should transition to HalfOpen
        assert!(cb.can_attempt());
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // One success - still HalfOpen
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        assert_eq!(cb.success_count(), 1);

        // Second success - should close
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.is_closed());
    }

    #[test]
    fn test_half_open_to_open_on_failure() {
        let config = CircuitBreakerConfig::default()
            .with_failure_threshold(1)
            .with_timeout(Duration::from_millis(1));
        let mut cb = CircuitBreaker::new(config);

        // Open the circuit
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for timeout
        std::thread::sleep(Duration::from_millis(5));

        // Transition to HalfOpen
        assert!(cb.can_attempt());
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // Failure in HalfOpen should reopen
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_reset() {
        let config = CircuitBreakerConfig::default().with_failure_threshold(1);
        let mut cb = CircuitBreaker::new(config);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        cb.reset();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.failure_count(), 0);
        assert!(cb.can_attempt());
    }

    #[test]
    fn test_config_builder() {
        let config = CircuitBreakerConfig::default()
            .with_failure_threshold(10)
            .with_success_threshold(5)
            .with_timeout(Duration::from_secs(120));

        assert_eq!(config.failure_threshold, 10);
        assert_eq!(config.success_threshold, 5);
        assert_eq!(config.timeout, Duration::from_secs(120));
    }

    #[test]
    fn test_default_config() {
        let config = CircuitBreakerConfig::default();
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.success_threshold, 2);
        assert_eq!(config.timeout, Duration::from_secs(60));
    }
}
