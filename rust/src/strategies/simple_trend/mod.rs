//! Simple Trend Following Strategy
//!
//! A minimalist trend-following strategy designed to generate more trades
//! by using only essential filters.
//!
//! ## Philosophy
//! - Fewer filters = More opportunities
//! - Simple rules that work across different market conditions
//! - Let winners run, cut losers quickly
//!
//! ## Entry Logic (Long)
//! 1. Price closes above EMA (trend direction)
//! 2. ATR expanding (volatility confirmation)
//!
//! That's it. No ADX, no breakout confirmation, no regime classification.
//!
//! ## Exit Logic
//! 1. Stop loss: entry - 2×ATR
//! 2. Take profit: entry + 4×ATR (2:1 reward-risk)
//! 3. Trailing stop after 50% profit reached

mod config;
mod strategy;
mod grid_params;

pub use config::SimpleTrendConfig;
pub use strategy::SimpleTrendStrategy;
pub use grid_params::GridParams;

use crate::Config;
use anyhow::Result;
use std::collections::HashMap;

/// Create strategy from config
pub fn create_strategy_from_config(config: &Config) -> Result<SimpleTrendStrategy> {
    let strategy_config: SimpleTrendConfig = serde_json::from_value(config.strategy.clone())?;
    Ok(SimpleTrendStrategy::new(strategy_config))
}

/// Convert config to params for reporting
pub fn config_to_params(config: &SimpleTrendConfig) -> HashMap<String, f64> {
    let mut params = HashMap::new();
    params.insert("ema_period".to_string(), config.ema_period as f64);
    params.insert("atr_period".to_string(), config.atr_period as f64);
    params.insert("stop_atr".to_string(), config.stop_atr_multiple);
    params.insert("target_atr".to_string(), config.target_atr_multiple);
    params.insert("trailing_activation".to_string(), config.trailing_activation);
    params.insert("require_expansion".to_string(), if config.require_expansion { 1.0 } else { 0.0 });
    params
}

/// Format params for display
pub fn format_params(params: &HashMap<String, f64>) -> String {
    let ema = params.get("ema_period").map(|v| *v as usize).unwrap_or(0);
    let stop = params.get("stop_atr").unwrap_or(&0.0);
    let target = params.get("target_atr").unwrap_or(&0.0);
    let req_exp = params.get("require_expansion").map(|v| *v > 0.5).unwrap_or(false);
    format!("EMA:{} Stop:{:.1} Tgt:{:.1} Exp:{}", ema, stop, target, if req_exp { "Y" } else { "N" })
}
