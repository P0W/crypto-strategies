//! Regime Grid Strategy Configuration
//!
//! Strategy-specific configuration for the Dynamic Regime-Aware Grid Trading Strategy.

use serde::{Deserialize, Serialize};

/// Regime Grid Strategy Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeGridConfig {
    // Grid Parameters
    /// Maximum number of grid levels (default: 10)
    pub max_grids: usize,
    /// Spacing between grid levels as percentage (default: 0.01 = 1%)
    pub grid_spacing_pct: f64,
    /// Sell target above buy price as percentage (default: 0.04 = 4%)
    pub sell_target_pct: f64,
    /// Cancel orders too far from price as percentage (default: 0.12 = 12%)
    pub cancel_threshold_pct: f64,

    // Regime Detection (4H timeframe)
    /// ADX period (default: 14)
    pub adx_period: usize,
    /// ADX threshold for sideways detection (default: 20.0)
    pub adx_sideways_threshold: f64,
    /// Short EMA period for regime detection (default: 50)
    pub ema_short_period: usize,
    /// Long EMA period for regime detection (default: 200)
    pub ema_long_period: usize,
    /// EMA band percentage for sideways detection (default: 0.10 = ±10%)
    pub ema_band_pct: f64,
    /// RSI period (default: 14)
    pub rsi_period: usize,
    /// RSI minimum for bull market (default: 50.0)
    pub rsi_bull_min: f64,
    /// RSI maximum for bull market (default: 70.0)
    pub rsi_bull_max: f64,
    /// RSI threshold for bear market (default: 40.0)
    pub rsi_bear_threshold: f64,
    /// High volatility single candle threshold (default: 0.05 = 5%)
    pub high_volatility_candle_pct: f64,

    // Bull Market Adjustments
    /// Maximum grids in bull market (default: 5)
    pub bull_max_grids: usize,
    /// Grid spacing in bull market (default: 0.01 = 1%)
    pub bull_grid_spacing_pct: f64,
    /// Sell target in bull market (default: 0.025 = 2.5%)
    pub bull_sell_target_pct: f64,

    // Risk Management (MANDATORY)
    /// Maximum capital allocated to grid (default: 0.40 = 40%)
    pub max_capital_usage_pct: f64,
    /// Maximum drawdown before stop (default: 0.20 = 20%)
    pub max_drawdown_pct: f64,
    /// ATR/Price ratio for volatility kill switch (default: 0.15 = 15% for daily, use lower for intraday)
    pub volatility_kill_threshold: f64,
    /// Hours to pause after volatility kill switch (default: 12)
    pub volatility_pause_hours: u64,
    /// ATR period for 1H volatility kill switch (default: 14)
    pub atr_period_1h: usize,

    // Stop Loss & Position Management
    /// Stop loss ATR multiple (default: 2.0)
    pub stop_atr_multiple: f64,
    /// Trailing stop activation percentage (default: 0.03 = 3%)
    pub trailing_activation_pct: f64,
    /// Trailing stop ATR multiple (default: 1.0)
    pub trailing_atr_multiple: f64,
}

impl Default for RegimeGridConfig {
    fn default() -> Self {
        RegimeGridConfig {
            // Grid Parameters
            max_grids: 10,
            grid_spacing_pct: 0.01,
            sell_target_pct: 0.04,
            cancel_threshold_pct: 0.12,

            // Regime Detection
            adx_period: 14,
            adx_sideways_threshold: 20.0,
            ema_short_period: 50,
            ema_long_period: 200,
            ema_band_pct: 0.10,
            rsi_period: 14,
            rsi_bull_min: 50.0,
            rsi_bull_max: 70.0,
            rsi_bear_threshold: 40.0,
            high_volatility_candle_pct: 0.05,

            // Bull Market Adjustments
            bull_max_grids: 5,
            bull_grid_spacing_pct: 0.01,
            bull_sell_target_pct: 0.025,

            // Risk Management
            max_capital_usage_pct: 0.40,
            max_drawdown_pct: 0.20,
            volatility_kill_threshold: 0.15, // 15% for daily timeframe
            volatility_pause_hours: 12,
            atr_period_1h: 14,

            // Stop Loss & Position Management
            stop_atr_multiple: 2.0,
            trailing_activation_pct: 0.03,
            trailing_atr_multiple: 1.0,
        }
    }
}
