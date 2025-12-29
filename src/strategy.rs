//! Trading strategy framework
//!
//! Defines the Strategy trait and implements the Volatility Regime strategy.

use crate::{Candle, Position, Signal, Symbol, VolatilityRegime};
use crate::config::StrategyConfig;
use crate::indicators::{atr, ema, adx};

/// Trading strategy trait
pub trait Strategy: Send + Sync {
    /// Generate trading signal for the given candle data
    fn generate_signal(
        &self,
        symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal;

    /// Calculate stop loss price for entry
    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64;

    /// Calculate take profit price for entry
    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64;

    /// Update trailing stop if applicable
    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64>;
}

/// Volatility Regime Adaptive Strategy
pub struct VolatilityRegimeStrategy {
    config: StrategyConfig,
}

impl VolatilityRegimeStrategy {
    pub fn new(config: StrategyConfig) -> Self {
        VolatilityRegimeStrategy { config }
    }

    /// Classify volatility regime
    fn classify_regime(&self, candles: &[Candle]) -> Option<VolatilityRegime> {
        if candles.len() < self.config.volatility_lookback {
            return None;
        }

        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        
        // Get current ATR - properly handle nested Option
        let current_atr = atr_values.last().and_then(|&x| x)?;
        
        // Calculate ATR percentile over lookback period
        let lookback_start = candles.len().saturating_sub(self.config.volatility_lookback);
        let lookback_atrs: Vec<f64> = atr_values[lookback_start..]
            .iter()
            .filter_map(|&x| x)
            .collect();

        if lookback_atrs.is_empty() {
            return None;
        }

        let mut sorted_atrs = lookback_atrs.clone();
        // Use unwrap_or for safety with NaN handling
        sorted_atrs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        
        let median_atr = sorted_atrs[sorted_atrs.len() / 2];
        
        if median_atr == 0.0 {
            return Some(VolatilityRegime::Normal);
        }

        let atr_ratio = current_atr / median_atr;

        if atr_ratio >= self.config.extreme_threshold {
            Some(VolatilityRegime::Extreme)
        } else if atr_ratio >= self.config.expansion_threshold {
            Some(VolatilityRegime::Expansion)
        } else if atr_ratio <= self.config.compression_threshold {
            Some(VolatilityRegime::Compression)
        } else {
            Some(VolatilityRegime::Normal)
        }
    }

    /// Check if trend is confirmed
    fn is_trend_confirmed(&self, candles: &[Candle]) -> bool {
        if candles.len() < self.config.ema_slow {
            return false;
        }

        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();

        let ema_fast = ema(&close, self.config.ema_fast);
        let ema_slow = ema(&close, self.config.ema_slow);
        let adx_values = adx(&high, &low, &close, self.config.adx_period);

        let fast = ema_fast.last().and_then(|&x| x).unwrap_or(0.0);
        let slow = ema_slow.last().and_then(|&x| x).unwrap_or(0.0);
        let adx_val = adx_values.last().and_then(|&x| x).unwrap_or(0.0);

        fast > slow && adx_val > self.config.adx_threshold
    }

    /// Check for breakout
    fn is_breakout(&self, candles: &[Candle]) -> bool {
        if candles.len() < 2 {
            return false;
        }

        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_values.last().and_then(|&x| x).unwrap_or(0.0);

        // Find recent high
        let lookback = 10.min(candles.len());
        let recent_high = candles[candles.len() - lookback..]
            .iter()
            .map(|c| c.high)
            .fold(f64::MIN, f64::max);

        let breakout_level = recent_high - self.config.breakout_atr_multiple * current_atr;
        let current_close = candles.last().unwrap().close;
        let prev_close = candles[candles.len() - 2].close;

        current_close > breakout_level && prev_close <= breakout_level
    }
}

impl Strategy for VolatilityRegimeStrategy {
    fn generate_signal(
        &self,
        _symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal {
        // Don't generate signals if insufficient data
        if candles.len() < self.config.ema_slow {
            return Signal::Flat;
        }

        // If we have a position, check exit conditions
        if let Some(pos) = position {
            let current_price = candles.last().unwrap().close;
            
            // Exit on regime shift to extreme
            if let Some(regime) = self.classify_regime(candles) {
                if regime == VolatilityRegime::Extreme {
                    return Signal::Flat;
                }
            }

            // Exit if trend breaks (only if profitable)
            if pos.unrealized_pnl(current_price) >= 0.0 {
                let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
                let ema_slow = ema(&close, self.config.ema_slow);
                if let Some(slow_ema) = ema_slow.last().and_then(|&x| x) {
                    if current_price < slow_ema {
                        return Signal::Flat;
                    }
                }
            }

            return Signal::Long; // Hold position
        }

        // Entry logic
        let regime = match self.classify_regime(candles) {
            Some(r) => r,
            None => return Signal::Flat,
        };

        // Only trade in compression or normal regimes
        if regime != VolatilityRegime::Compression && regime != VolatilityRegime::Normal {
            return Signal::Flat;
        }

        // Check trend confirmation
        if !self.is_trend_confirmed(candles) {
            return Signal::Flat;
        }

        // Check for breakout
        if self.is_breakout(candles) {
            Signal::Long
        } else {
            Signal::Flat
        }
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_values
            .last()
            .and_then(|&x| x)
            .unwrap_or(entry_price * 0.05);

        entry_price - self.config.stop_atr_multiple * current_atr
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_values
            .last()
            .and_then(|&x| x)
            .unwrap_or(entry_price * 0.05);

        entry_price + self.config.target_atr_multiple * current_atr
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        let unrealized_pnl = position.unrealized_pnl(current_price);
        let target_pnl = (position.target_price - position.entry_price) * position.quantity;

        // Activate trailing stop at specified percentage of target
        if unrealized_pnl < target_pnl * self.config.trailing_activation {
            return None;
        }

        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_values
            .last()
            .and_then(|&x| x)
            .unwrap_or(current_price * 0.05);

        let new_stop = current_price - self.config.trailing_atr_multiple * current_atr;

        // Only update if it's higher than current stop
        if let Some(current_stop) = position.trailing_stop {
            if new_stop > current_stop {
                Some(new_stop)
            } else {
                Some(current_stop)
            }
        } else if new_stop > position.stop_price {
            Some(new_stop)
        } else {
            None
        }
    }
}
