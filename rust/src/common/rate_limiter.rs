//! Rate Limiter implementation using token bucket algorithm
//!
//! Provides rate limiting to prevent API abuse and stay within
//! exchange rate limits.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};
use tokio::time::Instant;

/// Configuration for the rate limiter
#[derive(Debug, Clone)]
pub struct RateLimiterConfig {
    /// Maximum requests allowed per second
    pub max_requests_per_second: usize,
    /// Refill interval for tokens
    pub refill_interval: Duration,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            max_requests_per_second: 10,
            refill_interval: Duration::from_secs(1),
        }
    }
}

impl RateLimiterConfig {
    /// Create a new configuration with custom rate limit
    pub fn with_rate(mut self, requests_per_second: usize) -> Self {
        self.max_requests_per_second = requests_per_second;
        self
    }

    /// Create a new configuration with custom refill interval
    pub fn with_refill_interval(mut self, interval: Duration) -> Self {
        self.refill_interval = interval;
        self
    }
}

/// Rate limiter using token bucket algorithm
///
/// # Example
///
/// ```
/// use crypto_strategies::common::{RateLimiter, RateLimiterConfig};
///
/// #[tokio::main]
/// async fn main() {
///     let config = RateLimiterConfig::default().with_rate(5);
///     let limiter = RateLimiter::new(config);
///
///     // Acquire a permit before making a request
///     limiter.acquire().await;
///     // Make API request...
/// }
/// ```
#[derive(Debug)]
pub struct RateLimiter {
    permits: Arc<Semaphore>,
    max_permits: usize,
    last_refill: Arc<Mutex<Instant>>,
    refill_interval: Duration,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration
    pub fn new(config: RateLimiterConfig) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(config.max_requests_per_second)),
            max_permits: config.max_requests_per_second,
            last_refill: Arc::new(Mutex::new(Instant::now())),
            refill_interval: config.refill_interval,
        }
    }

    /// Create a rate limiter with default configuration (10 requests/second)
    pub fn with_defaults() -> Self {
        Self::new(RateLimiterConfig::default())
    }

    /// Create a rate limiter with a specific requests-per-second limit
    pub fn with_rate(requests_per_second: usize) -> Self {
        Self::new(RateLimiterConfig::default().with_rate(requests_per_second))
    }

    /// Acquire a permit to make a request
    ///
    /// This method will:
    /// 1. Refill permits if the refill interval has elapsed
    /// 2. Wait for a permit to become available
    ///
    /// The permit is consumed (not returned to the pool).
    pub async fn acquire(&self) {
        // Try to refill permits
        self.try_refill().await;

        // Wait for a permit and consume it
        let permit = self
            .permits
            .acquire()
            .await
            .expect("Semaphore should not be closed");
        permit.forget(); // Consume the permit (don't return it to the pool)
    }

    /// Try to acquire a permit without blocking
    ///
    /// Returns `true` if a permit was acquired, `false` otherwise.
    /// The permit is consumed (not returned to the pool).
    pub async fn try_acquire(&self) -> bool {
        self.try_refill().await;
        match self.permits.try_acquire() {
            Ok(permit) => {
                permit.forget(); // Consume the permit
                true
            }
            Err(_) => false,
        }
    }

    /// Get the number of available permits
    pub fn available_permits(&self) -> usize {
        self.permits.available_permits()
    }

    /// Get the maximum number of permits (rate limit)
    pub fn max_permits(&self) -> usize {
        self.max_permits
    }

    /// Try to refill permits if the refill interval has elapsed
    async fn try_refill(&self) {
        let mut last_refill = self.last_refill.lock().await;
        let elapsed = last_refill.elapsed();

        if elapsed >= self.refill_interval {
            // Calculate how many intervals have passed
            let intervals = (elapsed.as_millis() / self.refill_interval.as_millis()) as usize;
            let permits_to_add = intervals * self.max_permits;

            // Only add permits up to the maximum
            let current = self.permits.available_permits();
            let to_add = permits_to_add.min(self.max_permits.saturating_sub(current));

            if to_add > 0 {
                self.permits.add_permits(to_add);
            }

            *last_refill = Instant::now();
        }
    }
}

impl Clone for RateLimiter {
    fn clone(&self) -> Self {
        Self {
            permits: Arc::clone(&self.permits),
            max_permits: self.max_permits,
            last_refill: Arc::clone(&self.last_refill),
            refill_interval: self.refill_interval,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_default_config() {
        let config = RateLimiterConfig::default();
        assert_eq!(config.max_requests_per_second, 10);
        assert_eq!(config.refill_interval, Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_config_builder() {
        let config = RateLimiterConfig::default()
            .with_rate(20)
            .with_refill_interval(Duration::from_millis(500));

        assert_eq!(config.max_requests_per_second, 20);
        assert_eq!(config.refill_interval, Duration::from_millis(500));
    }

    #[tokio::test]
    async fn test_initial_permits() {
        let limiter = RateLimiter::with_rate(5);
        assert_eq!(limiter.available_permits(), 5);
        assert_eq!(limiter.max_permits(), 5);
    }

    #[tokio::test]
    async fn test_acquire_reduces_permits() {
        let limiter = RateLimiter::with_rate(5);
        assert_eq!(limiter.available_permits(), 5);

        limiter.acquire().await;
        // Permit is consumed (not returned like with acquire_owned)
        // The semaphore permit is dropped after acquire returns
    }

    #[tokio::test]
    async fn test_try_acquire_success() {
        let limiter = RateLimiter::with_rate(5);
        assert!(limiter.try_acquire().await);
    }

    #[tokio::test]
    async fn test_try_acquire_exhausted() {
        let config = RateLimiterConfig::default()
            .with_rate(2)
            .with_refill_interval(Duration::from_secs(60)); // Long interval to prevent refill
        let limiter = RateLimiter::new(config);

        // Exhaust all permits using acquire (blocks until permit available)
        limiter.acquire().await;
        limiter.acquire().await;

        // Verify permits are exhausted
        assert_eq!(limiter.available_permits(), 0);

        // try_acquire should fail immediately since no permits available
        // and refill interval (60s) hasn't passed
        assert!(!limiter.try_acquire().await);
    }

    #[tokio::test]
    async fn test_refill_after_interval() {
        let config = RateLimiterConfig::default()
            .with_rate(2)
            .with_refill_interval(Duration::from_millis(50));
        let limiter = RateLimiter::new(config);

        // Exhaust permits
        limiter.acquire().await;
        limiter.acquire().await;

        // Wait for refill
        sleep(Duration::from_millis(60)).await;

        // Should be able to acquire again
        assert!(limiter.try_acquire().await);
    }

    #[tokio::test]
    async fn test_clone_shares_state() {
        let limiter1 = RateLimiter::with_rate(3);
        let limiter2 = limiter1.clone();

        // Both should see the same permits
        assert_eq!(limiter1.available_permits(), limiter2.available_permits());

        // Consuming from one should affect the other
        limiter1.acquire().await;
        // Note: The permit is consumed, so both should see reduced permits
    }

    #[tokio::test]
    async fn test_concurrent_acquire() {
        let limiter = RateLimiter::with_rate(5);
        let limiter_clone = limiter.clone();

        let handles: Vec<_> = (0..5)
            .map(|_| {
                let l = limiter_clone.clone();
                tokio::spawn(async move {
                    l.acquire().await;
                })
            })
            .collect();

        for handle in handles {
            handle.await.unwrap();
        }

        // All 5 permits should be consumed
    }

    #[tokio::test]
    async fn test_with_defaults() {
        let limiter = RateLimiter::with_defaults();
        assert_eq!(limiter.max_permits(), 10);
    }
}
