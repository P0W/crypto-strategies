//! Grid Trading Strategy
//!
//! A market-neutral strategy that profits from price oscillations within a range.
//! Best suited for ranging/sideways markets with high volatility.

mod config;
mod strategy;

pub use config::GridTradingConfig;
pub use strategy::GridTradingStrategy;

use crate::{Config, Strategy};
use anyhow::Result;

/// Create strategy from config (called by registry)
pub fn create(config: &Config) -> Result<Box<dyn Strategy>> {
    let strategy_config: GridTradingConfig = serde_json::from_value(config.strategy.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse grid_trading config: {}", e))?;
    Ok(Box::new(GridTradingStrategy::new(strategy_config)))
}
