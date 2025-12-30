//! CoinDCX Exchange API Library
//!
//! A production-grade Rust library for interacting with the CoinDCX cryptocurrency exchange.
//!
//! # Features
//!
//! - **Full API Coverage**: Public and authenticated endpoints
//! - **Retry with Exponential Backoff**: Automatic retries on transient failures
//! - **Rate Limiting**: Token bucket algorithm to stay within API limits
//! - **Circuit Breaker**: Fault tolerance pattern to prevent cascading failures
//! - **Type-Safe**: Strongly typed request/response models
//!
//! # Quick Start
//!
//! ```no_run
//! use crypto_strategies::coindcx::{CoinDCXClient, types::OrderRequest, types::OrderSide};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Create client with API credentials
//!     let client = CoinDCXClient::new("your_api_key", "your_api_secret");
//!
//!     // Get ticker information
//!     let ticker = client.get_ticker("BTCINR").await?;
//!     println!("BTC/INR: {}", ticker.last_price);
//!
//!     // Get account balances
//!     let balances = client.get_balances().await?;
//!     for balance in balances {
//!         if balance.balance > 0.0 {
//!             println!("{}: {}", balance.currency, balance.balance);
//!         }
//!     }
//!
//!     // Place a limit order
//!     let order = OrderRequest::limit(OrderSide::Buy, "BTCINR", 0.001, 5000000.0);
//!     let response = client.place_order(&order).await?;
//!     println!("Order placed: {:?}", response);
//!
//!     Ok(())
//! }
//! ```
//!
//! # Configuration
//!
//! ```no_run
//! use std::time::Duration;
//! use crypto_strategies::coindcx::{CoinDCXClient, ClientConfig};
//!
//! let config = ClientConfig::default()
//!     .with_max_retries(5)
//!     .with_timeout(Duration::from_secs(60))
//!     .with_rate_limit(20)
//!     .with_circuit_breaker_threshold(10);
//!
//! let client = CoinDCXClient::with_config("api_key", "api_secret", config);
//! ```
//!
//! # Environment Variables
//!
//! You can also create a client using environment variables:
//!
//! ```no_run
//! use crypto_strategies::coindcx::CoinDCXClient;
//!
//! // Expects COINDCX_API_KEY and COINDCX_API_SECRET
//! let client = CoinDCXClient::from_env().expect("Missing credentials");
//! ```
//!
//! # Modules
//!
//! - [`auth`]: Authentication utilities for HMAC-SHA256 signing
//! - [`circuit_breaker`]: Circuit breaker pattern implementation
//! - [`rate_limiter`]: Rate limiting with token bucket algorithm
//! - [`types`]: Request and response type definitions
//! - [`client`]: Main API client implementation

pub mod auth;
pub mod circuit_breaker;
pub mod client;
pub mod rate_limiter;
pub mod types;

// Re-export main types for convenience
pub use auth::Credentials;
pub use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
pub use client::{ClientConfig, CoinDCXClient, API_BASE_URL, PUBLIC_BASE_URL};
pub use rate_limiter::{RateLimiter, RateLimiterConfig};

// Re-export commonly used types
pub use types::{
    Balance, Candle, MarketDetails, OrderBook, OrderRequest, OrderResponse, OrderSide, OrderStatus,
    OrderType, Ticker, Trade, UserInfo,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Verify all public exports are accessible
        let _ = CircuitBreakerConfig::default();
        let _ = RateLimiterConfig::default();
        let _ = ClientConfig::default();
    }

    #[test]
    fn test_types_accessible() {
        let _ = OrderSide::Buy;
        let _ = OrderType::MarketOrder;
    }
}
