//! Quick Flip (Pattern Scalp) Configuration
//!
//! Multi-timeframe range breakout strategy with trend confirmation

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickFlipConfig {
    /// ATR period for volatility measurement (14 period on 1d chart for multi-TF, or ~96 bars for single-TF)
    pub atr_period: usize,

    /// Minimum range as percentage of ATR (default: 0.25 = 25%)
    pub min_range_pct: f64,

    /// Number of bars for range window on 15m chart (default: 1 = original Quick Flip spec)
    pub opening_bars: usize,

    /// Cooldown bars between trades (default: 6 = 30 minutes on 5m)
    pub cooldown_bars: usize,

    /// Use conservative target (50% of range) vs full range (default: false = full range)
    pub conservative_target: bool,

    /// Enable trend filter (only trade in direction of 1h trend, default: true)
    pub use_trend_filter: bool,

    /// EMA period for trend determination (default: 20)
    pub trend_ema_period: usize,

    /// Enable volume confirmation (require above-average volume, default: true)
    pub use_volume_filter: bool,

    /// Volume lookback period for average calculation (default: 20)
    pub volume_lookback: usize,

    /// Minimum volume multiplier vs average (default: 1.2 = 20% above average)
    pub min_volume_multiplier: f64,
}

impl Default for QuickFlipConfig {
    fn default() -> Self {
        Self {
            atr_period: 14,              // 14 days on 1d chart (multi-TF) or 96 bars (single-TF)
            min_range_pct: 0.25,         // 25% of ATR
            opening_bars: 1,             // 1x 15m bar (original spec)
            cooldown_bars: 6,            // 30 minutes on 5m
            conservative_target: false,  // Use full range
            use_trend_filter: true,      // Enable trend filter
            trend_ema_period: 20,        // 20-period EMA
            use_volume_filter: true,     // Enable volume filter
            volume_lookback: 20,         // 20 bars for volume average
            min_volume_multiplier: 1.2,  // 20% above average
        }
    }
}
