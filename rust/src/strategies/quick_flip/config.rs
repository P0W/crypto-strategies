//! Quick Flip (Pattern Scalp) Configuration
//!
//! Rolling range breakout strategy adapted for crypto 24/7 markets

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickFlipConfig {
    /// ATR period for volatility measurement (default: 96 = ~8 hours on 5m chart)
    pub atr_period: usize,

    /// Minimum range as percentage of ATR (default: 0.25 = 25%)
    pub min_range_pct: f64,

    /// Number of bars for range window (default: 6 = 30 minutes on 5m)
    pub opening_bars: usize,

    /// Cooldown bars between trades (default: 6)
    pub cooldown_bars: usize,

    /// Use conservative target (50% of range) vs full range (default: false = full range)
    pub conservative_target: bool,
}

impl Default for QuickFlipConfig {
    fn default() -> Self {
        Self {
            atr_period: 96,           // ~8 hours on 5m chart
            min_range_pct: 0.25,
            opening_bars: 6,          // 30 minutes on 5m chart
            cooldown_bars: 6,
            conservative_target: false,
        }
    }
}
