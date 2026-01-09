//! Quick Flip (Pattern Scalp) Strategy
//!
//! Time-of-day range breakout with candlestick pattern confirmation.

mod config;
mod strategy;

pub use config::QuickFlipConfig;
pub use strategy::QuickFlipStrategy;

use crate::{Config, Strategy};
use anyhow::Result;

/// Create strategy from config (called by registry)
pub fn create(config: &Config) -> Result<Box<dyn Strategy>> {
    let strategy_config: QuickFlipConfig = serde_json::from_value(config.strategy.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse quick_flip config: {}", e))?;
    Ok(Box::new(QuickFlipStrategy::new(strategy_config)))
}
