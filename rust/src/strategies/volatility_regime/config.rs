//! Volatility Regime Strategy Configuration
//!
//! Strategy-specific configuration for the Volatility Regime Adaptive Strategy.

use serde::{Deserialize, Serialize};

/// Volatility Regime Strategy Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolatilityRegimeConfig {
    /// ATR period for volatility calculation
    pub atr_period: usize,
    /// Lookback period for volatility regime classification
    pub volatility_lookback: usize,
    /// Compression regime threshold (ATR percentile)
    pub compression_threshold: f64,
    /// Expansion regime threshold (ATR percentile)
    pub expansion_threshold: f64,
    /// Extreme regime threshold (ATR percentile)
    pub extreme_threshold: f64,
    /// Fast EMA period
    pub ema_fast: usize,
    /// Slow EMA period
    pub ema_slow: usize,
    /// ADX period
    pub adx_period: usize,
    /// ADX threshold for trend strength
    pub adx_threshold: f64,
    /// Breakout ATR multiple
    pub breakout_atr_multiple: f64,
    /// Stop loss ATR multiple
    pub stop_atr_multiple: f64,
    /// Take profit ATR multiple
    pub target_atr_multiple: f64,
    /// Trailing stop activation (% of target)
    pub trailing_activation: f64,
    /// Trailing stop ATR multiple
    pub trailing_atr_multiple: f64,
}

impl Default for VolatilityRegimeConfig {
    fn default() -> Self {
        VolatilityRegimeConfig {
            atr_period: 14,
            volatility_lookback: 20,
            compression_threshold: 0.6,
            expansion_threshold: 1.5,
            extreme_threshold: 2.5,
            ema_fast: 8,
            ema_slow: 21,
            adx_period: 14,
            adx_threshold: 30.0,
            breakout_atr_multiple: 1.5,
            stop_atr_multiple: 2.5,
            target_atr_multiple: 5.0,
            trailing_activation: 0.5,
            trailing_atr_multiple: 1.5,
        }
    }
}
