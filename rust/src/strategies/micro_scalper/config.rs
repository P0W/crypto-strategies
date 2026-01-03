//! Micro Scalper Configuration - Optimized for 5m timeframe
//!
//! Fast RSI + EMA strategy designed for high-frequency crypto trading

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicroScalperConfig {
    /// RSI period (default: 7 for faster signals)
    pub rsi_period: usize,

    /// RSI oversold threshold for long entry (default: 30)
    pub rsi_oversold: f64,

    /// RSI overbought threshold for short entry (default: 70)
    pub rsi_overbought: f64,

    /// Fast EMA period (default: 9)
    pub ema_fast: usize,

    /// Slow EMA period (default: 21)
    pub ema_slow: usize,

    /// ATR period for stops (default: 14)
    pub atr_period: usize,

    /// Stop loss ATR multiple (default: 1.5)
    pub stop_atr: f64,

    /// Take profit ATR multiple (default: 2.0)
    pub target_atr: f64,

    /// Trailing stop ATR multiple (default: 1.0)
    pub trailing_atr: f64,

    /// Profit in ATR to activate trailing (default: 1.0)
    pub trailing_activation: f64,

    /// Cooldown bars between trades (default: 2)
    pub cooldown: usize,

    /// Allow short trades (default: true)
    pub allow_short: bool,

    /// Minimum bars to hold (default: 1)
    pub min_hold_bars: usize,

    /// Maximum bars to hold (default: 50)
    pub max_hold_bars: usize,
}

impl Default for MicroScalperConfig {
    fn default() -> Self {
        Self {
            rsi_period: 7,
            rsi_oversold: 30.0,
            rsi_overbought: 70.0,
            ema_fast: 9,
            ema_slow: 21,
            atr_period: 14,
            stop_atr: 1.5,
            target_atr: 2.0,
            trailing_atr: 1.0,
            trailing_activation: 1.0,
            cooldown: 2,
            allow_short: true,
            min_hold_bars: 1,
            max_hold_bars: 50,
        }
    }
}
