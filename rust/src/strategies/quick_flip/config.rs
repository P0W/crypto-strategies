//! Quick Flip (Pattern Scalp) Configuration
//!
//! Multi-timeframe range breakout strategy with pattern confirmation

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickFlipConfig {
    /// ATR period for volatility measurement (14 period on 1d chart for multi-TF)
    pub atr_period: usize,

    /// Minimum range as percentage of ATR (default: 0.25 = 25%)
    pub min_range_pct: f64,

    /// Number of bars for range window on 15m chart (default: 1 = original Quick Flip spec)
    pub opening_bars: usize,

    /// Cooldown bars between trades (default: 6 = 30 minutes on 5m)
    pub cooldown_bars: usize,

    /// Use conservative target (50% of range) vs full range (default: false = full range)
    pub conservative_target: bool,
}

impl Default for QuickFlipConfig {
    fn default() -> Self {
        Self {
            atr_period: 14,             // 14 days on 1d chart
            min_range_pct: 0.25,        // 25% of ATR
            opening_bars: 1,            // 1x 15m bar (original spec)
            cooldown_bars: 6,           // 30 minutes on 5m
            conservative_target: false, // Use full range
        }
    }
}
