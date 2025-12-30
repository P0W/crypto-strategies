//! Crypto Trading Strategies
//!
//! An automated trading system for cryptocurrency markets,
//! featuring volatility-based strategies, comprehensive backtesting, and
//! parameter optimization.
//!
//! # CoinDCX API Library
//!
//! This crate includes a standalone, production-grade CoinDCX API client
//! with circuit breaker, rate limiting, and retry logic.
//!
//! ```no_run
//! use crypto_strategies::coindcx::{CoinDCXClient, OrderRequest, OrderSide};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let client = CoinDCXClient::new("api_key", "api_secret");
//!     let ticker = client.get_ticker("BTCINR").await?;
//!     println!("Price: {}", ticker.last_price);
//!     Ok(())
//! }
//! ```

pub mod backtest;
pub mod coindcx;
pub mod config;
pub mod data;
pub mod indicators;
pub mod optimizer;
pub mod risk;
pub mod state_manager;
pub mod strategies;
pub mod types;

pub use config::Config;
pub use strategies::Strategy;
pub use types::*;

// Re-export CoinDCX client for convenience
pub use coindcx::{ClientConfig, CoinDCXClient};
