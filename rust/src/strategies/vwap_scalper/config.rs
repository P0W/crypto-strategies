//! VWAP Scalper Strategy Configuration
//!
//! Simple price action strategy around VWAP for short timeframes (5m, 15m).

use serde::{Deserialize, Serialize};

/// VWAP Scalper Strategy Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VwapScalperConfig {
    // === ATR Parameters ===
    /// ATR period for stops/targets (default: 14)
    pub atr_period: usize,
    /// Stop loss ATR multiple (default: 1.5)
    pub stop_atr_multiple: f64,
    /// Take profit ATR multiple (default: 2.0)
    pub target_atr_multiple: f64,

    // === Trailing Stop ===
    /// Trailing stop activation in ATR profit (default: 1.0)
    pub trailing_activation: f64,
    /// Trailing stop ATR multiple (default: 1.0)
    pub trailing_atr_multiple: f64,

    // === Entry Filters ===
    /// Max distance from VWAP to enter (percentage, default: 2.0)
    pub max_distance_pct: f64,
    /// Volume MA period (default: 20)
    pub volume_period: usize,
    /// Volume spike threshold (default: 1.0 = no filter)
    pub volume_threshold: f64,
    /// Require volume confirmation (default: false)
    pub require_volume: bool,

    // === Position ===
    /// Allow short selling (default: false)
    pub allow_short: bool,

    // === Scalping ===
    /// Max bars to hold position (default: 50)
    pub max_hold_bars: usize,
    /// Cooldown bars after trade (default: 2)
    pub cooldown_bars: usize,
}

impl Default for VwapScalperConfig {
    fn default() -> Self {
        VwapScalperConfig {
            // ATR settings
            atr_period: 14,
            stop_atr_multiple: 1.5,
            target_atr_multiple: 2.0,

            // Trailing stop
            trailing_activation: 1.0,
            trailing_atr_multiple: 1.0,

            // Entry filters - relaxed by default
            max_distance_pct: 2.0,
            volume_period: 20,
            volume_threshold: 1.0,
            require_volume: false,

            // Long only by default
            allow_short: false,

            // Scalping
            max_hold_bars: 50,
            cooldown_bars: 2,
        }
    }
}
