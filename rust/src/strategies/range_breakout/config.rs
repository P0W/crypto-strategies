//! Range Breakout Configuration - Minimal parameters for speed

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeBreakoutConfig {
    /// Lookback period for high/low range (default: 20)
    pub lookback: usize,

    /// ATR period (default: 14)
    pub atr_period: usize,

    /// Stop loss ATR multiple (default: 1.0)
    pub stop_atr: f64,

    /// Take profit ATR multiple (default: 2.0)
    pub target_atr: f64,

    /// Minimum bars between trades (default: 1)
    pub cooldown: usize,
}

impl Default for RangeBreakoutConfig {
    fn default() -> Self {
        Self {
            lookback: 20,
            atr_period: 14,
            stop_atr: 1.0,
            target_atr: 2.0,
            cooldown: 1,
        }
    }
}
