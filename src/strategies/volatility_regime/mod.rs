//! Volatility Regime Strategy
//!
//! Exploits volatility clustering via regime classification.

mod config;
mod strategy;

pub use config::VolatilityRegimeConfig;
pub use strategy::VolatilityRegimeStrategy;

use crate::{Config, Strategy};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Market volatility regime
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VolatilityRegime {
    Compression,
    Normal,
    Expansion,
    Extreme,
}

/// Create strategy from config (called by registry)
pub fn create(config: &Config) -> Result<Box<dyn Strategy>> {
    let strategy_config: VolatilityRegimeConfig =
        serde_json::from_value(config.strategy.clone())
            .map_err(|e| anyhow::anyhow!("Failed to parse volatility_regime config: {}", e))?;
    Ok(Box::new(VolatilityRegimeStrategy::new(strategy_config)))
}
