//! Common utilities shared across exchange clients
//!
//! This module contains reusable components for all exchange integrations:
//! - Circuit breaker pattern for fault tolerance
//! - Rate limiter using token bucket algorithm
//! - Retry logic with exponential backoff

pub mod circuit_breaker;
pub mod rate_limiter;

pub use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
pub use rate_limiter::{RateLimiter, RateLimiterConfig};
