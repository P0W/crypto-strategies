//! Mean Reversion Scalper Strategy Configuration
//!
//! Strategy-specific configuration optimized for short timeframe trading (5m, 15m, 1h).
//! Parameters are tuned for crypto markets' high volatility and 24/7 nature.

use serde::{Deserialize, Serialize};

/// Mean Reversion Scalper Strategy Configuration
///
/// This strategy exploits mean reversion after extreme price moves, validated by:
/// 1. Bollinger Band deviation (price at extremes)
/// 2. RSI confirmation (oversold/overbought)
/// 3. Volume spike (institutional interest)
/// 4. Trend filter (don't fight the trend)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeanReversionConfig {
    // === Bollinger Band Parameters ===
    /// Bollinger Band period (default: 20)
    pub bb_period: usize,
    /// Bollinger Band standard deviation multiplier (default: 2.0)
    pub bb_std: f64,

    // === RSI Parameters ===
    /// RSI calculation period (default: 14)
    pub rsi_period: usize,
    /// RSI oversold threshold for long entry (default: 30)
    pub rsi_oversold: f64,
    /// RSI overbought threshold for short entry or exit (default: 70)
    pub rsi_overbought: f64,

    // === Volume Parameters ===
    /// Volume moving average period (default: 20)
    pub volume_period: usize,
    /// Volume spike multiplier - volume must exceed MA * this value (default: 1.5)
    pub volume_spike_threshold: f64,
    /// Require volume spike for entry (default: true)
    pub require_volume_spike: bool,

    // === Trend Filter Parameters ===
    /// EMA period for trend filter (default: 50)
    pub trend_ema_period: usize,
    /// Use trend filter - only trade in trend direction (default: true)
    pub use_trend_filter: bool,

    // === Risk Management Parameters ===
    /// ATR period for stop/target calculation (default: 14)
    pub atr_period: usize,
    /// Stop loss ATR multiple (default: 1.5)
    pub stop_atr_multiple: f64,
    /// Take profit mode: "bb_middle" uses middle band, "atr" uses ATR multiple
    pub take_profit_mode: String,
    /// Take profit ATR multiple (only used if take_profit_mode is "atr") (default: 2.0)
    pub target_atr_multiple: f64,
    /// Trailing stop activation - profit in ATR terms to activate trailing (default: 0.5)
    pub trailing_activation: f64,
    /// Trailing stop ATR multiple (default: 1.0)
    pub trailing_atr_multiple: f64,

    // === Entry Refinement Parameters ===
    /// Minimum distance from BB for entry (% of band width) (default: 0.0)
    /// 0 = touch band, 0.1 = 10% beyond band
    pub bb_penetration: f64,
    /// Maximum consecutive losses before pausing (default: 3)
    pub max_consecutive_losses: usize,
    /// Cooldown bars after max consecutive losses (default: 5)
    pub cooldown_bars: usize,

    // === Short Selling Parameters ===
    /// Enable short selling (default: false for spot trading)
    pub allow_short: bool,
}

impl Default for MeanReversionConfig {
    fn default() -> Self {
        MeanReversionConfig {
            // Bollinger Bands - standard settings work well for crypto
            bb_period: 20,
            bb_std: 2.0,

            // RSI - standard period, slightly relaxed thresholds for crypto volatility
            rsi_period: 14,
            rsi_oversold: 30.0,
            rsi_overbought: 70.0,

            // Volume - confirms institutional interest
            volume_period: 20,
            volume_spike_threshold: 1.5,
            require_volume_spike: true,

            // Trend filter - 50 EMA is classic institutional level
            trend_ema_period: 50,
            use_trend_filter: true,

            // Risk management - tight stops for scalping
            atr_period: 14,
            stop_atr_multiple: 1.5,
            take_profit_mode: "bb_middle".to_string(),
            target_atr_multiple: 2.0,
            trailing_activation: 0.5,
            trailing_atr_multiple: 1.0,

            // Entry refinement
            bb_penetration: 0.0,
            max_consecutive_losses: 3,
            cooldown_bars: 5,

            // Spot trading only by default
            allow_short: false,
        }
    }
}
