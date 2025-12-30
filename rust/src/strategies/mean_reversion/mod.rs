//! Mean Reversion Scalper Strategy Module
//!
//! A professional-grade mean reversion strategy commonly used by prop desks
//! and quant traders on short timeframes (5m, 15m, 1h).
//!
//! Core concept: Crypto markets exhibit strong mean reversion on short timeframes.
//! When price deviates significantly from its mean (Bollinger Band extreme), with
//! RSI confirmation and volume validation, there's a high probability of reversion.

use serde::{Deserialize, Serialize};

pub mod config;
pub mod grid_params;
pub mod strategy;
pub mod utils;

pub use config::MeanReversionConfig;
pub use grid_params::GridParams;
pub use strategy::MeanReversionStrategy;
pub use utils::{config_to_params, create_strategy_from_config, format_params};

/// Market state for mean reversion trading
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketState {
    /// Price near lower band + RSI oversold - potential long
    Oversold,
    /// Price near upper band + RSI overbought - potential short (or exit)
    Overbought,
    /// Price near middle band - neutral zone
    Neutral,
    /// Extreme deviation - too risky, no trade
    Extreme,
}

/// Volume condition for trade validation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VolumeState {
    /// Volume spike detected (above threshold)
    Spike,
    /// Normal volume
    Normal,
    /// Low volume - avoid trading
    Low,
}
