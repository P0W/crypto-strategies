//! Crypto Trading Strategies
//!
//! An automated trading system for cryptocurrency markets,
//! featuring volatility-based strategies, comprehensive backtesting, and
//! parameter optimization.

pub mod backtest;
pub mod config;
pub mod data;
pub mod exchange;
pub mod indicators;
pub mod optimizer;
pub mod risk;
pub mod state_manager;
pub mod strategies;
pub mod types;

pub use config::Config;
pub use exchange::{Balance, OrderRequest, OrderResponse, RobustCoinDCXClient, Ticker};
pub use strategies::Strategy;
pub use types::*;
