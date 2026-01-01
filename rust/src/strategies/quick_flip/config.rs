//! Quick Flip (Pattern Scalp) Configuration
//!
//! Minimal parameters for a simple time-of-day range breakout strategy

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickFlipConfig {
    /// Daily ATR period for volatility filter (default: 14)
    pub daily_atr_period: usize,

    /// Minimum range as percentage of daily ATR (default: 0.25 = 25%)
    pub min_range_pct: f64,

    /// Session start time in minutes from midnight EST (9:30 AM EST = 570)
    pub session_start_minutes: usize,

    /// Validity window in minutes from session start (default: 90)
    pub validity_window_minutes: usize,

    /// Entry timeframe in minutes (1 or 5 minutes)
    pub entry_timeframe_minutes: usize,

    /// Use conservative target (50% of range) vs full range (default: false = full range)
    pub conservative_target: bool,
}

impl Default for QuickFlipConfig {
    fn default() -> Self {
        Self {
            daily_atr_period: 14,
            min_range_pct: 0.25,
            session_start_minutes: 570, // 9:30 AM EST
            validity_window_minutes: 90,
            entry_timeframe_minutes: 5,
            conservative_target: false,
        }
    }
}
