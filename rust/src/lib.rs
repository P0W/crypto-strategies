//! Trading Strategies System
//!
//! An automated trading system for cryptocurrency and equity markets,
//! featuring volatility-based strategies, comprehensive backtesting, and
//! parameter optimization.
//!
//! # Exchange API Libraries
//!
//! This crate includes standalone API clients for:
//! - **Binance** (default): Public market data, no API key required
//! - **CoinDCX**: Indian crypto exchange with full trading support
//! - **Zerodha Kite**: Indian equity/F&O exchange (NSE, BSE)
//!
//! All clients include circuit breaker, rate limiting, and retry logic.
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
//! ## CoinDCX Example (Crypto Trading)
//! ```no_run
//! use crypto_strategies::coindcx::CoinDCXClient;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let client = CoinDCXClient::new("api_key", "api_secret");
//!     let ticker = client.get_ticker("BTCINR").await?;
//!     println!("Price: {}", ticker.last_price);
//!     Ok(())
//! }
//! ```
//!
//! ## Zerodha Example (Equity Trading)
//! ```no_run
//! use crypto_strategies::zerodha::ZerodhaClient;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let client = ZerodhaClient::new("api_key", "api_secret")
//!         .with_access_token("access_token".to_string());
//!     let quote = client.get_quote("NSE:RELIANCE").await?;
//!     println!("LTP: {}", quote.last_price);
//!     Ok(())
//! }
//! ```

pub mod backtest;
pub mod binance;
pub mod coindcx;
pub mod common;
pub mod config;
pub mod data;
pub mod grid;
pub mod indicators;
pub mod monthly_pnl;
pub mod multi_timeframe;
pub mod optimizer;
pub mod risk;
pub mod state_manager;
pub mod strategies;
pub mod types;
pub mod zerodha;

pub use config::Config;
pub use monthly_pnl::MonthlyPnLMatrix;
pub use multi_timeframe::{
    MultiSymbolMultiTimeframeData, MultiTimeframeCandles, MultiTimeframeData,
};
pub use strategies::Strategy;
pub use types::*;

// Re-export exchange clients for convenience
pub use binance::BinanceClient;
pub use coindcx::CoinDCXClient;
pub use zerodha::ZerodhaClient;
