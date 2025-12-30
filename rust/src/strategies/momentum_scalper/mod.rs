//! Momentum Scalper Strategy Module
//!
//! A professional-grade momentum strategy designed for short timeframes (5m, 15m, 1h).
//! Used by prop desks and HFT firms for quick in/out trades.
//!
//! Core concept: Ride momentum breakouts with tight risk management.
//! Entry on EMA crossover + volume spike, exit on reversal or target hit.

use serde::{Deserialize, Serialize};

pub mod config;
pub mod grid_params;
pub mod strategy;
pub mod utils;

pub use config::MomentumScalperConfig;
pub use grid_params::GridParams;
pub use strategy::MomentumScalperStrategy;
pub use utils::{config_to_params, create_strategy_from_config, format_params};

/// Momentum state classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MomentumState {
    /// Strong bullish momentum
    StrongBullish,
    /// Weak bullish momentum
    WeakBullish,
    /// Neutral / no clear momentum
    Neutral,
    /// Weak bearish momentum
    WeakBearish,
    /// Strong bearish momentum
    StrongBearish,
}

/// Trend direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrendDirection {
    Up,
    Down,
    Sideways,
}
