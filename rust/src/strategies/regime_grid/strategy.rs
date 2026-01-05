//! Regime-Aware Grid Trading Strategy Implementation
//!
//! This strategy implements a sophisticated grid trading system that adapts to
//! market conditions through regime classification. It focuses on capital preservation
//! by avoiding trading during unfavorable market conditions.
//!
//! Performance optimized: Indicators are calculated once per signal generation
//! and reused to avoid O(N²) complexity.

use crate::indicators::{adx, atr, ema, rsi};
use crate::strategies::Strategy;
use crate::{Candle, Position, Signal, Symbol};
use chrono::{DateTime, Utc};

use super::config::RegimeGridConfig;
use super::MarketRegime;

/// Pre-calculated indicators to avoid redundant computation
struct Indicators {
    _current_atr: Option<f64>,
    current_ema_short: Option<f64>,
    current_ema_long: Option<f64>,
    current_adx: Option<f64>,
    current_rsi: Option<f64>,
}

impl Indicators {
    /// Calculate all indicators once from candle data
    fn new(candles: &[Candle], config: &RegimeGridConfig) -> Self {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, config.adx_period); // Use same period as ADX
        let ema_short = ema(&close, config.ema_short_period);
        let ema_long = ema(&close, config.ema_long_period);
        let adx_values = adx(&high, &low, &close, config.adx_period);
        let rsi_values = rsi(&close, config.rsi_period);

        Self {
            _current_atr: atr_values.last().and_then(|&x| x),
            current_ema_short: ema_short.last().and_then(|&x| x),
            current_ema_long: ema_long.last().and_then(|&x| x),
            current_adx: adx_values.last().and_then(|&x| x),
            current_rsi: rsi_values.last().and_then(|&x| x),
        }
    }

    /// Calculate ATR only (for stop/target/trailing methods)
    fn atr_only(candles: &[Candle], atr_period: usize) -> Option<f64> {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        atr(&high, &low, &close, atr_period)
            .last()
            .and_then(|&x| x)
    }
}

/// Grid state tracking
#[derive(Debug, Clone)]
struct GridState {
    /// When volatility kill switch was activated (None if not active)
    paused_until: Option<DateTime<Utc>>,
}

impl Default for GridState {
    fn default() -> Self {
        GridState {
            paused_until: None,
        }
    }
}

/// Regime-Aware Grid Trading Strategy
pub struct RegimeGridStrategy {
    config: RegimeGridConfig,
    state: GridState,
}

impl RegimeGridStrategy {
    pub fn new(config: RegimeGridConfig) -> Self {
        RegimeGridStrategy {
            config,
            state: GridState::default(),
        }
    }

    /// Check if volatility kill switch is active
    fn is_volatility_paused(&self) -> bool {
        if let Some(paused_until) = self.state.paused_until {
            Utc::now() < paused_until
        } else {
            false
        }
    }

    /// Classify market regime based on indicators
    fn classify_regime(&self, candles: &[Candle], ind: &Indicators) -> Option<MarketRegime> {
        if candles.is_empty() {
            return None;
        }

        let current_candle = candles.last()?;
        let current_price = current_candle.close;

        // Check for high volatility single candle
        let candle_change = ((current_candle.close - current_candle.open) / current_candle.open).abs();
        if candle_change > self.config.high_volatility_candle_pct {
            return Some(MarketRegime::HighVolatility);
        }

        let adx = ind.current_adx?;
        let ema_short = ind.current_ema_short?;
        let ema_long = ind.current_ema_long?;
        let rsi = ind.current_rsi?;

        // Regime 1: Sideways (IDEAL)
        // ADX < threshold AND price within ±band% of short EMA
        if adx < self.config.adx_sideways_threshold {
            let distance_from_ema = (current_price - ema_short).abs() / ema_short;
            if distance_from_ema <= self.config.ema_band_pct {
                return Some(MarketRegime::Sideways);
            }
        }

        // Regime 3: Bear Market (NO TRADING)
        // Price < long EMA AND RSI < bear threshold
        if current_price < ema_long && rsi < self.config.rsi_bear_threshold {
            return Some(MarketRegime::Bearish);
        }

        // Regime 2: Bull Market (MODIFIED GRID)
        // Price > long EMA AND RSI between bull_min and bull_max
        if current_price > ema_long
            && rsi >= self.config.rsi_bull_min
            && rsi <= self.config.rsi_bull_max
        {
            return Some(MarketRegime::Bullish);
        }

        // Default to high volatility if no clear regime
        Some(MarketRegime::HighVolatility)
    }

    /// Generate signal for sideways regime (full grid)
    fn sideways_grid_signal(&self, _candles: &[Candle], position: Option<&Position>) -> Signal {
        // In sideways market, we want to buy on dips
        // For simplicity in backtest mode, generate Long signal when no position
        if position.is_none() {
            Signal::Long
        } else {
            // Hold existing position
            Signal::Flat
        }
    }

    /// Generate signal for bull regime (modified grid)
    fn bull_grid_signal(&self, _candles: &[Candle], position: Option<&Position>) -> Signal {
        // In bull market, be more selective - only buy on smaller dips
        // For simplicity in backtest mode, generate Long signal when no position
        if position.is_none() {
            Signal::Long
        } else {
            // Hold existing position
            Signal::Flat
        }
    }
}

impl Strategy for RegimeGridStrategy {
    fn name(&self) -> &'static str {
        "regime_grid"
    }

    fn generate_signal(
        &self,
        _symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal {
        // Need minimum data for indicators
        let min_period = self
            .config
            .ema_long_period
            .max(self.config.adx_period)
            .max(self.config.rsi_period);
        
        if candles.len() < min_period {
            return Signal::Flat;
        }

        // 1. Check volatility kill switch
        if self.is_volatility_paused() {
            return Signal::Flat;
        }

        // 2. Calculate all indicators once
        let ind = Indicators::new(candles, &self.config);

        // 3. Classify market regime
        let regime = match self.classify_regime(candles, &ind) {
            Some(r) => r,
            None => return Signal::Flat,
        };

        // 4. Apply regime-specific logic
        match regime {
            MarketRegime::Bearish | MarketRegime::HighVolatility => Signal::Flat,
            MarketRegime::Sideways => self.sideways_grid_signal(candles, position),
            MarketRegime::Bullish => self.bull_grid_signal(candles, position),
        }
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let atr = Indicators::atr_only(candles, self.config.adx_period)
            .unwrap_or(entry_price * 0.02); // Fallback to 2% if ATR not available
        
        entry_price - (atr * self.config.stop_atr_multiple)
    }

    fn calculate_take_profit(&self, _candles: &[Candle], entry_price: f64) -> f64 {
        // Use regime-specific sell target
        // Since we don't have access to current regime here, use default
        entry_price * (1.0 + self.config.sell_target_pct)
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        let unrealized_pnl_pct = (current_price - position.entry_price) / position.entry_price;
        
        // Activate trailing stop if profit exceeds threshold
        if unrealized_pnl_pct < self.config.trailing_activation_pct {
            return None;
        }

        let atr = Indicators::atr_only(candles, self.config.adx_period)
            .unwrap_or(current_price * 0.02);
        
        let trailing_stop = current_price - (atr * self.config.trailing_atr_multiple);
        
        // Only update if new stop is higher than current
        if let Some(current_stop) = position.trailing_stop {
            if trailing_stop > current_stop {
                Some(trailing_stop)
            } else {
                None
            }
        } else {
            Some(trailing_stop)
        }
    }

    fn get_regime_score(&self, candles: &[Candle]) -> f64 {
        let ind = Indicators::new(candles, &self.config);
        
        match self.classify_regime(candles, &ind) {
            Some(MarketRegime::Sideways) => 1.5,  // Ideal conditions
            Some(MarketRegime::Bullish) => 1.0,    // Modified grid
            _ => 0.0,                              // No trading
        }
    }
}
