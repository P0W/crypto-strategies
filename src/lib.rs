//! Crypto Trading Strategies
//!
//! A production-grade automated trading system for cryptocurrency markets,
//! featuring volatility-based strategies, comprehensive backtesting, and
//! parameter optimization.

pub mod config;
pub mod data;
pub mod exchange;
pub mod strategy;
pub mod risk;
pub mod backtest;
pub mod optimize;
pub mod indicators;
pub mod types;

pub use config::Config;
pub use types::*;
