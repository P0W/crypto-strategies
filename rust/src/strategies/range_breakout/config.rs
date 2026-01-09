//! Range Breakout Configuration - Minimal parameters for speed

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeBreakoutConfig {
    /// Lookback period for high/low range (default: 30)
    pub lookback: usize,

    /// ATR period (default: 14)
    pub atr_period: usize,

    /// Stop loss ATR multiple (default: 1.5)
    pub stop_atr: f64,

    /// Take profit ATR multiple (default: 4.0)
    pub target_atr: f64,

    /// Minimum bars between trades (default: 3)
    pub cooldown: usize,

    /// Allow short positions (default: false for crypto spot)
    #[serde(default)]
    pub allow_shorts: bool,

    /// EMA period for trend filter (default: 50, 0 = disabled)
    #[serde(default = "default_trend_ema")]
    pub trend_ema: usize,

    /// ADX period for trend strength filter (default: 14)
    #[serde(default = "default_adx_period")]
    pub adx_period: usize,

    /// Minimum ADX for valid breakout (default: 20, 0 = disabled)
    #[serde(default = "default_min_adx")]
    pub min_adx: f64,

    /// Enable trailing stop (default: true)
    #[serde(default = "default_use_trailing")]
    pub use_trailing: bool,

    /// Trailing stop ATR multiple (default: 2.0)
    #[serde(default = "default_trailing_atr")]
    pub trailing_atr: f64,
}

fn default_trend_ema() -> usize {
    50
}
fn default_adx_period() -> usize {
    14
}
fn default_min_adx() -> f64 {
    20.0
}
fn default_use_trailing() -> bool {
    true
}
fn default_trailing_atr() -> f64 {
    2.0
}

impl Default for RangeBreakoutConfig {
    fn default() -> Self {
        Self {
            lookback: 30,
            atr_period: 14,
            stop_atr: 1.5,
            target_atr: 4.0,
            cooldown: 3,
            allow_shorts: false,
            trend_ema: 50,
            adx_period: 14,
            min_adx: 20.0,
            use_trailing: true,
            trailing_atr: 2.0,
        }
    }
}
