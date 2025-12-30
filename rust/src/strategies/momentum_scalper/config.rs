//! Momentum Scalper Strategy Configuration
//!
//! Optimized for short timeframe trading (5m, 15m, 1h).
//! Fast signals, tight stops, quick profits.

use serde::{Deserialize, Serialize};

/// Momentum Scalper Strategy Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MomentumScalperConfig {
    // === EMA Crossover Parameters ===
    /// Fast EMA period (default: 9)
    pub ema_fast: usize,
    /// Slow EMA period (default: 21)
    pub ema_slow: usize,
    /// Trend EMA period for bias (default: 50)
    pub ema_trend: usize,

    // === Momentum Confirmation ===
    /// Use MACD for momentum confirmation (default: true)
    pub use_macd: bool,
    /// MACD fast period (default: 12)
    pub macd_fast: usize,
    /// MACD slow period (default: 26)
    pub macd_slow: usize,
    /// MACD signal period (default: 9)
    pub macd_signal: usize,

    // === Volume Filter ===
    /// Volume MA period (default: 20)
    pub volume_period: usize,
    /// Volume spike threshold (default: 1.2)
    pub volume_threshold: f64,
    /// Require volume confirmation (default: false for short TF)
    pub require_volume: bool,

    // === Risk Management ===
    /// ATR period for stops (default: 14)
    pub atr_period: usize,
    /// Stop loss ATR multiple (default: 1.0 - tight for scalping)
    pub stop_atr_multiple: f64,
    /// Take profit ATR multiple (default: 1.5)
    pub target_atr_multiple: f64,
    /// Trailing stop activation in ATR (default: 0.5)
    pub trailing_activation: f64,
    /// Trailing stop ATR multiple (default: 0.75)
    pub trailing_atr_multiple: f64,

    // === Entry Filters ===
    /// Minimum ADX for trend strength (default: 20 - lower for short TF)
    pub adx_threshold: f64,
    /// ADX period (default: 14)
    pub adx_period: usize,
    /// Only trade in trend direction (default: true)
    pub trade_with_trend: bool,

    // === Scalping Specific ===
    /// Max bars to hold position (default: 20)
    pub max_hold_bars: usize,
    /// Exit on EMA cross back (default: true)
    pub exit_on_cross: bool,
    /// Cooldown bars after trade (default: 2)
    pub cooldown_bars: usize,

    // === Position ===
    /// Allow short selling (default: false)
    pub allow_short: bool,
}

impl Default for MomentumScalperConfig {
    fn default() -> Self {
        MomentumScalperConfig {
            // Fast EMAs for quick signals
            ema_fast: 9,
            ema_slow: 21,
            ema_trend: 50,

            // MACD for momentum
            use_macd: true,
            macd_fast: 12,
            macd_slow: 26,
            macd_signal: 9,

            // Volume - relaxed for short TF
            volume_period: 20,
            volume_threshold: 1.2,
            require_volume: false,

            // Tight risk management
            atr_period: 14,
            stop_atr_multiple: 1.0,
            target_atr_multiple: 1.5,
            trailing_activation: 0.5,
            trailing_atr_multiple: 0.75,

            // Entry filters
            adx_threshold: 20.0,
            adx_period: 14,
            trade_with_trend: true,

            // Scalping
            max_hold_bars: 20,
            exit_on_cross: true,
            cooldown_bars: 2,

            // Long only
            allow_short: false,
        }
    }
}
