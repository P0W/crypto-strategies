//! Volatility Regime Strategy Module
//!
//! Contains all components for the Volatility Regime Adaptive Strategy.

use serde::{Deserialize, Serialize};

pub mod config;
pub mod grid_params;
pub mod strategy;
pub mod utils;

pub use config::VolatilityRegimeConfig;
pub use grid_params::GridParams;
pub use strategy::VolatilityRegimeStrategy;
pub use utils::{config_to_params, create_strategy_from_config};

/// Market volatility regime
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VolatilityRegime {
    Compression, // Low volatility, potential breakout
    Normal,      // Average volatility
    Expansion,   // High volatility, trending
    Extreme,     // Very high volatility, risk-off
}
