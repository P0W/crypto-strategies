//! Regime-Aware Grid Trading Strategy
//!
//! A production-ready grid trading strategy that adapts to market conditions
//! through regime classification. Designed for crypto markets with emphasis on
//! capital preservation during unfavorable conditions.

mod config;
mod strategy;

pub use config::RegimeGridConfig;
pub use strategy::RegimeGridStrategy;

use crate::{Config, Strategy};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Market regime classification for grid trading
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketRegime {
    /// ADX < 20, price near 50 EMA - IDEAL for grid trading
    Sideways,
    /// Price > 200 EMA, RSI 50-70 - Modified grid (reduced exposure)
    Bullish,
    /// Price < 200 EMA, RSI < 40 - NO trading (capital preservation)
    Bearish,
    /// Single candle > 5% or ATR spike - NO trading (volatility protection)
    HighVolatility,
}

/// Create strategy from config (called by registry)
pub fn create(config: &Config) -> Result<Box<dyn Strategy>> {
    let strategy_config: RegimeGridConfig =
        serde_json::from_value(config.strategy.clone())
            .map_err(|e| anyhow::anyhow!("Failed to parse regime_grid config: {}", e))?;
    Ok(Box::new(RegimeGridStrategy::new(strategy_config)))
}
