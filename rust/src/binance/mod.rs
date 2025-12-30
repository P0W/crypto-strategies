//! Binance API client for downloading historical OHLCV data
//! No API key needed for public market data endpoints.

mod client;
mod types;

pub use client::BinanceClient;
pub use types::*;
