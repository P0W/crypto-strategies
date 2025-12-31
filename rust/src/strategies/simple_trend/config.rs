//! Simple Trend Configuration

use serde::{Deserialize, Serialize};

/// Configuration for Simple Trend Strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleTrendConfig {
    /// EMA period for trend direction (default: 20)
    #[serde(default = "default_ema_period")]
    pub ema_period: usize,

    /// ATR period for volatility measurement (default: 14)
    #[serde(default = "default_atr_period")]
    pub atr_period: usize,

    /// ATR lookback for expansion check (default: 5)
    #[serde(default = "default_atr_lookback")]
    pub atr_lookback: usize,

    /// Stop loss as ATR multiple (default: 2.0)
    #[serde(default = "default_stop_atr")]
    pub stop_atr_multiple: f64,

    /// Take profit as ATR multiple (default: 4.0)
    #[serde(default = "default_target_atr")]
    pub target_atr_multiple: f64,

    /// Trailing stop activation threshold in ATR (default: 0.5)
    #[serde(default = "default_trailing_activation")]
    pub trailing_activation: f64,

    /// Trailing stop ATR multiple (default: 1.5)
    #[serde(default = "default_trailing_atr")]
    pub trailing_atr_multiple: f64,

    /// Require ATR expansion for entry (default: true)
    #[serde(default = "default_require_expansion")]
    pub require_expansion: bool,

    /// ATR expansion threshold (current/previous ratio, default: 1.0)
    #[serde(default = "default_expansion_threshold")]
    pub expansion_threshold: f64,
}

fn default_ema_period() -> usize { 20 }
fn default_atr_period() -> usize { 14 }
fn default_atr_lookback() -> usize { 5 }
fn default_stop_atr() -> f64 { 2.0 }
fn default_target_atr() -> f64 { 4.0 }
fn default_trailing_activation() -> f64 { 0.5 }
fn default_trailing_atr() -> f64 { 1.5 }
fn default_require_expansion() -> bool { true }
fn default_expansion_threshold() -> f64 { 1.0 }

impl Default for SimpleTrendConfig {
    fn default() -> Self {
        Self {
            ema_period: default_ema_period(),
            atr_period: default_atr_period(),
            atr_lookback: default_atr_lookback(),
            stop_atr_multiple: default_stop_atr(),
            target_atr_multiple: default_target_atr(),
            trailing_activation: default_trailing_activation(),
            trailing_atr_multiple: default_trailing_atr(),
            require_expansion: default_require_expansion(),
            expansion_threshold: default_expansion_threshold(),
        }
    }
}
