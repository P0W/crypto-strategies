//! Volatility Regime Adaptive Strategy
//!
//! Strategy implementation for trading based on volatility regime classification.

use crate::indicators::{adx, atr, ema};
use crate::strategies::Strategy;
use crate::{Candle, Position, Signal, Symbol};

use super::config::VolatilityRegimeConfig;
use super::VolatilityRegime;

/// Volatility Regime Adaptive Strategy
pub struct VolatilityRegimeStrategy {
    config: VolatilityRegimeConfig,
}

impl VolatilityRegimeStrategy {
    pub fn new(config: VolatilityRegimeConfig) -> Self {
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
        let lookback_start = candles
            .len()
            .saturating_sub(self.config.volatility_lookback);
        let lookback_atrs: Vec<f64> = atr_values[lookback_start..]
            .iter()
            .filter_map(|&x| x)
            .collect();

        if lookback_atrs.is_empty() {
            return None;
        }

        // Calculate mean ATR (same as Python implementation)
        let atr_mean: f64 = lookback_atrs.iter().sum::<f64>() / lookback_atrs.len() as f64;

        if atr_mean == 0.0 {
            return Some(VolatilityRegime::Normal);
        }

        let atr_ratio = current_atr / atr_mean;

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
        if candles.len() < self.config.volatility_lookback + 1 {
            return false;
        }

        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_values.last().and_then(|&x| x).unwrap_or(0.0);

        // Find recent high using volatility_lookback (same as Python)
        // Python's ind["highest"][-1] is the Highest indicator value at the PREVIOUS bar
        // That value represents the highest high over the `lookback` bars ending at the previous bar
        // So for bar T, we want max(high[T-lookback], high[T-lookback+1], ..., high[T-1])
        let n = candles.len();
        let lookback = self.config.volatility_lookback;
        let start = n.saturating_sub(lookback + 1); // n - lookback - 1
        let end = n - 1; // n - 1 (exclusive of current bar)
        let recent_high = candles[start..end]
            .iter()
            .map(|c| c.high)
            .fold(f64::MIN, f64::max);

        let breakout_level = recent_high - self.config.breakout_atr_multiple * current_atr;
        let current_close = candles.last().unwrap().close;
        let prev_close = candles[n - 2].close;

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
        // Calculate minimum warmup period for all indicators
        // ADX needs: atr_period + 2*adx_period for proper smoothing
        // Volatility regime needs: atr_period + volatility_lookback
        // EMA needs: ema_slow period
        let min_warmup = (self.config.atr_period + 2 * self.config.adx_period)
            .max(self.config.atr_period + self.config.volatility_lookback)
            .max(self.config.ema_slow);

        // Don't generate signals if insufficient data for all indicators
        if candles.len() < min_warmup {
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
        let breakout = self.is_breakout(candles);

        if breakout {
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
        // Production behavior: Trailing stop activates AND updates immediately
        // This provides immediate downside protection once profit threshold is reached
        // HFTs and professional systems don't wait until next bar to protect profits

        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_values
            .last()
            .and_then(|&x| x)
            .unwrap_or(current_price * 0.05);

        // Calculate profit in ATR terms
        let profit_atr = if current_atr > 0.0 {
            (current_price - position.entry_price) / current_atr
        } else {
            0.0
        };

        // Get current stop level (trailing stop if active, else initial stop)
        let current_stop = position.trailing_stop.unwrap_or(position.stop_price);

        // Check if profit threshold is met for trailing activation
        if profit_atr >= self.config.trailing_activation {
            // Calculate new trailing stop level
            let new_stop = current_price - self.config.trailing_atr_multiple * current_atr;

            // Only update if new stop is higher (ratchet up only)
            if new_stop > current_stop {
                Some(new_stop)
            } else {
                Some(current_stop) // Keep existing stop
            }
        } else if position.trailing_stop.is_some() {
            // Trailing was active but profit dropped - keep existing stop
            Some(current_stop)
        } else {
            // Not activated yet
            None
        }
    }

    /// Get regime score for position sizing
    /// Matches Python implementation:
    /// - Compression: 1.5 (higher conviction for breakouts)
    /// - Normal: 1.0 (standard sizing)
    /// - Expansion: 0.8 (slightly reduced)
    /// - Extreme: 0.5 (minimal exposure)
    fn get_regime_score(&self, candles: &[Candle]) -> f64 {
        match self.classify_regime(candles) {
            Some(VolatilityRegime::Compression) => 1.5,
            Some(VolatilityRegime::Normal) => 1.0,
            Some(VolatilityRegime::Expansion) => 0.8,
            Some(VolatilityRegime::Extreme) => 0.5,
            None => 1.0,
        }
    }
}
