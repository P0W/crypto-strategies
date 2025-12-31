//! Mean Reversion Scalper Strategy
//!
//! Professional-grade mean reversion strategy for short timeframes.

mod config;
mod strategy;

pub use config::MeanReversionConfig;
pub use strategy::MeanReversionStrategy;

use crate::{Config, Strategy};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Market state for mean reversion trading
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketState {
    Oversold,
    Overbought,
    Neutral,
    Extreme,
}

/// Volume condition for trade validation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VolumeState {
    Spike,
    Normal,
    Low,
}

/// Create strategy from config (called by registry)
pub fn create(config: &Config) -> Result<Box<dyn Strategy>> {
    let strategy_config: MeanReversionConfig = serde_json::from_value(config.strategy.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse mean_reversion config: {}", e))?;
    Ok(Box::new(MeanReversionStrategy::new(strategy_config)))
}
