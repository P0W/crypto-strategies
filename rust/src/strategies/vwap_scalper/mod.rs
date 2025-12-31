//! VWAP Scalper Strategy
//!
//! Simple price action strategy around VWAP for short timeframes.

mod config;
mod strategy;

pub use config::VwapScalperConfig;
pub use strategy::VwapScalperStrategy;

use crate::{Config, Strategy};
use anyhow::Result;

/// Create strategy from config (called by registry)
pub fn create(config: &Config) -> Result<Box<dyn Strategy>> {
    let strategy_config: VwapScalperConfig = serde_json::from_value(config.strategy.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse vwap_scalper config: {}", e))?;
    Ok(Box::new(VwapScalperStrategy::new(strategy_config)))
}
