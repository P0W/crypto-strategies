//! Micro Scalper Strategy Module
//!
//! High-frequency scalping strategy optimized for 5-minute crypto charts.
//! Uses RSI + EMA crossover for entries with ATR-based risk management.

pub mod config;
pub mod strategy;

pub use config::MicroScalperConfig;
pub use strategy::MicroScalperStrategy;

use crate::strategies::Strategy;
use crate::Config;
use anyhow::Result;

/// Create a new Micro Scalper strategy from config
pub fn create(config: &Config) -> Result<Box<dyn Strategy>> {
    let strategy_config: MicroScalperConfig = serde_json::from_value(config.strategy.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse micro_scalper config: {}", e))?;
    Ok(Box::new(MicroScalperStrategy::new(strategy_config)))
}
