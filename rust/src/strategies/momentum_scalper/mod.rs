//! Momentum Scalper Strategy
//!
//! Professional momentum strategy for short timeframes.

mod config;
mod strategy;

pub use config::MomentumScalperConfig;
pub use strategy::MomentumScalperStrategy;

use crate::{Config, Strategy};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Momentum state classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MomentumState {
    StrongBullish,
    WeakBullish,
    Neutral,
    WeakBearish,
    StrongBearish,
}

/// Create strategy from config (called by registry)
pub fn create(config: &Config) -> Result<Box<dyn Strategy>> {
    let strategy_config: MomentumScalperConfig = serde_json::from_value(config.strategy.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse momentum_scalper config: {}", e))?;
    Ok(Box::new(MomentumScalperStrategy::new(strategy_config)))
}
