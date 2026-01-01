//! Quick Flip (Pattern Scalp) Configuration
//!
//! Parameters for the original Quick Flip strategy adapted for crypto
//! Uses daily-like ATR and opening range concept on 5m timeframe

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickFlipConfig {
    /// ATR period for volatility measurement (default: 288 = ~1 day on 5m chart)
    /// On 5m chart: 288 bars = 24 hours
    pub atr_period: usize,

    /// Minimum range as percentage of ATR (default: 0.25 = 25%)
    pub min_range_pct: f64,

    /// Number of bars for opening range box (default: 3 = 15 minutes on 5m)
    pub opening_bars: usize,

    /// Validity window in bars after range establishment (default: 18 = 90 minutes on 5m)
    pub validity_window_bars: usize,

    /// Cooldown bars between trades (default: 12)
    pub cooldown_bars: usize,

    /// Use conservative target (50% of range) vs full range (default: false = full range)
    pub conservative_target: bool,

    /// Session start hour UTC (default: 0 for midnight, can be adjusted)
    pub session_start_hour: usize,
}

impl Default for QuickFlipConfig {
    fn default() -> Self {
        Self {
            atr_period: 288,          // ~24 hours on 5m chart
            min_range_pct: 0.25,
            opening_bars: 3,          // 15 minutes on 5m chart
            validity_window_bars: 18, // 90 minutes on 5m chart
            cooldown_bars: 12,
            conservative_target: false,
            session_start_hour: 0,    // Midnight UTC
        }
    }
}
