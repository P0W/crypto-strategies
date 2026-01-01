//! Quick Flip (Pattern Scalp) Configuration
//!
//! Minimal parameters for range breakout with pattern confirmation
//! Adapted for 24/7 crypto markets

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickFlipConfig {
    /// Range lookback period in bars (default: 3 = 15 min on 5m chart)
    pub range_lookback: usize,

    /// ATR period for volatility measurement (default: 14)
    pub atr_period: usize,

    /// Minimum range as percentage of ATR (default: 0.25 = 25%)
    pub min_range_pct: f64,

    /// Cooldown bars between trades (default: 5)
    pub cooldown_bars: usize,

    /// Use conservative target (50% of range) vs full range (default: false = full range)
    pub conservative_target: bool,
}

impl Default for QuickFlipConfig {
    fn default() -> Self {
        Self {
            range_lookback: 3,
            atr_period: 14,
            min_range_pct: 0.25,
            cooldown_bars: 5,
            conservative_target: false,
        }
    }
}
