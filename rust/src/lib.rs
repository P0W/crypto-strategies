//! Crypto Trading Strategies
//!
//! An automated trading system for cryptocurrency markets,
//! featuring volatility-based strategies, comprehensive backtesting, and
//! parameter optimization.
//!
//! # Exchange API Libraries
//!
//! This crate includes standalone API clients for:
//! - **Binance** (default): Public market data, no API key required
//! - **CoinDCX**: Full trading support with circuit breaker and rate limiting
//!
//! ## Binance Example (Market Data)
//! ```no_run
//! use crypto_strategies::binance::BinanceClient;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let client = BinanceClient::new();
//!     let klines = client.get_klines("BTCUSDT", "1h", None, None, Some(100)).await?;
//!     println!("Fetched {} klines", klines.len());
//!     Ok(())
//! }
//! ```
//!
//! ## CoinDCX Example (Trading)
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
pub mod binance;
pub mod coindcx;
pub mod config;
pub mod data;
pub mod grid;
pub mod indicators;
pub mod multi_timeframe;
pub mod optimizer;
pub mod risk;
pub mod state_manager;
pub mod strategies;
pub mod types;

pub use config::Config;
pub use multi_timeframe::{MultiSymbolMultiTimeframeData, MultiTimeframeCandles, MultiTimeframeData};
pub use strategies::Strategy;
pub use types::*;

// Re-export exchange clients for convenience
pub use binance::BinanceClient;
pub use coindcx::{ClientConfig, CoinDCXClient};
