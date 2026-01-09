//! Range Breakout Strategy
//!
//! Entry on breakout of N-bar high/low range.

mod config;
mod strategy;

pub use config::RangeBreakoutConfig;
pub use strategy::RangeBreakoutStrategy;

use crate::{Config, Strategy};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakoutType {
    High,
    Low,
    None,
}

/// Create strategy from config (called by registry)
pub fn create(config: &Config) -> Result<Box<dyn Strategy>> {
    let strategy_config: RangeBreakoutConfig = serde_json::from_value(config.strategy.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse range_breakout config: {}", e))?;
    Ok(Box::new(RangeBreakoutStrategy::new(strategy_config)))
}
