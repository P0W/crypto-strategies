//! Quick Flip (Pattern Scalp) Configuration
//!
//! Range breakout strategy with momentum confirmation

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickFlipConfig {
    /// ATR period for volatility measurement (default: 14)
    pub atr_period: usize,

    /// Number of bars for range lookback (default: 10)
    pub range_bars: usize,

    /// Stop loss ATR multiplier (default: 1.5)
    #[serde(default = "default_stop_atr")]
    pub stop_atr: f64,

    /// Take profit ATR multiplier (default: 3.0)
    #[serde(default = "default_target_atr")]
    pub target_atr: f64,

    /// Minimum range as percentage of ATR (default: 0.5 = 50%)
    #[serde(default = "default_min_range_pct")]
    pub min_range_pct: f64,

    /// Cooldown bars between trades (default: 3)
    #[serde(default = "default_cooldown")]
    pub cooldown: usize,

    /// Enable reversal trades at range edges (default: false)
    #[serde(default)]
    pub enable_reversals: bool,

    /// Minimum body ratio for strong candle (default: 0.6)
    #[serde(default = "default_body_ratio")]
    pub body_ratio: f64,

    /// Allow short trades (default: false - long only)
    #[serde(default)]
    pub allow_shorts: bool,
}

fn default_stop_atr() -> f64 { 1.5 }
fn default_target_atr() -> f64 { 3.0 }
fn default_min_range_pct() -> f64 { 0.5 }
fn default_cooldown() -> usize { 3 }
fn default_body_ratio() -> f64 { 0.6 }

impl Default for QuickFlipConfig {
    fn default() -> Self {
        Self {
            atr_period: 14,
            range_bars: 10,
            stop_atr: 1.5,
            target_atr: 3.0,
            min_range_pct: 0.5,
            cooldown: 3,
            enable_reversals: false,
            body_ratio: 0.6,
            allow_shorts: false,
        }
    }
}
