//! Crypto Trading Strategies
//!
//! An automated trading system for cryptocurrency markets,
//! featuring volatility-based strategies, comprehensive backtesting, and
//! parameter optimization.

pub mod config;
pub mod data;
pub mod exchange;
pub mod strategies;
pub mod risk;
pub mod backtest;
pub mod optimizer;
pub mod indicators;
pub mod types;

pub use config::Config;
pub use types::*;
pub use strategies::Strategy;
