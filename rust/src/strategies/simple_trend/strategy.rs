//! Simple Trend Following Strategy Implementation
//!
//! Minimalist trend-following with only 2 entry conditions:
//! 1. Price above EMA (trend direction)
//! 2. ATR expanding (optional volatility confirmation)

use crate::indicators::{atr, ema};
use crate::strategies::Strategy;
use crate::{Candle, Position, Signal, Symbol};

use super::config::SimpleTrendConfig;

/// Simple Trend Following Strategy
pub struct SimpleTrendStrategy {
    config: SimpleTrendConfig,
}

impl SimpleTrendStrategy {
    pub fn new(config: SimpleTrendConfig) -> Self {
        Self { config }
    }

    /// Check if price is above EMA (bullish trend)
    fn is_bullish_trend(&self, candles: &[Candle]) -> bool {
        if candles.len() < self.config.ema_period {
            return false;
        }

        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let ema_values = ema(&close, self.config.ema_period);

        let current_close = candles.last().map(|c| c.close).unwrap_or(0.0);
        let current_ema = ema_values.last().and_then(|&x| x).unwrap_or(0.0);

        current_close > current_ema
    }

    /// Check if ATR is expanding (volatility increasing)
    fn is_atr_expanding(&self, candles: &[Candle]) -> bool {
        if !self.config.require_expansion {
            return true; // Skip check if disabled
        }

        if candles.len() < self.config.atr_period + self.config.atr_lookback {
            return true; // Not enough data, allow entry
        }

        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        let len = atr_values.len();

        if len < self.config.atr_lookback + 1 {
            return true;
        }

        let current_atr = atr_values[len - 1].unwrap_or(0.0);
        let prev_atr = atr_values[len - 1 - self.config.atr_lookback].unwrap_or(0.0);

        if prev_atr <= 0.0 {
            return true;
        }

        // ATR is expanding if current > threshold × previous
        current_atr >= self.config.expansion_threshold * prev_atr
    }

    /// Get current ATR value
    fn get_current_atr(&self, candles: &[Candle]) -> f64 {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        atr_values.last().and_then(|&x| x).unwrap_or(0.0)
    }
}

impl Strategy for SimpleTrendStrategy {
    fn generate_signal(
        &self,
        _symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal {
        // Minimum data requirement
        let min_bars = self.config.ema_period.max(self.config.atr_period) + 5;
        if candles.len() < min_bars {
            return Signal::Flat;
        }

        // If in position, hold
        if position.is_some() {
            // Exit if trend reverses (price below EMA)
            if !self.is_bullish_trend(candles) {
                return Signal::Flat;
            }
            return Signal::Long;
        }

        // Entry conditions (simplified!)
        // 1. Price above EMA
        // 2. ATR expanding (optional)
        if self.is_bullish_trend(candles) && self.is_atr_expanding(candles) {
            return Signal::Long;
        }

        Signal::Flat
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let current_atr = self.get_current_atr(candles);
        let atr_value = if current_atr > 0.0 {
            current_atr
        } else {
            entry_price * 0.02 // Fallback: 2%
        };
        entry_price - self.config.stop_atr_multiple * atr_value
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let current_atr = self.get_current_atr(candles);
        let atr_value = if current_atr > 0.0 {
            current_atr
        } else {
            entry_price * 0.02 // Fallback: 2%
        };
        entry_price + self.config.target_atr_multiple * atr_value
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        let current_atr = self.get_current_atr(candles);
        let atr_value = if current_atr > 0.0 {
            current_atr
        } else {
            current_price * 0.02
        };

        // Calculate profit in ATR terms
        let profit_atr = if atr_value > 0.0 {
            (current_price - position.entry_price) / atr_value
        } else {
            0.0
        };

        let current_stop = position.trailing_stop.unwrap_or(position.stop_price);

        // Activate trailing stop if profit threshold met
        if profit_atr >= self.config.trailing_activation {
            let new_stop = current_price - self.config.trailing_atr_multiple * atr_value;
            if new_stop > current_stop {
                Some(new_stop)
            } else {
                Some(current_stop)
            }
        } else if position.trailing_stop.is_some() {
            Some(current_stop)
        } else {
            None
        }
    }

    fn get_regime_score(&self, _candles: &[Candle]) -> f64 {
        1.0 // Simple strategy, no regime-based sizing
    }

    fn init(&mut self) {
        tracing::info!("Simple Trend Following strategy initialized");
    }
}
