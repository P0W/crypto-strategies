//! Volatility Regime Adaptive Strategy
//!
//! Strategy implementation for trading based on volatility regime classification.
//!
//! Performance optimized: Indicators are calculated once per generate_signal call
//! and reused across all helper methods to avoid O(N²) complexity.

use crate::indicators::{adx, atr, ema};
use crate::oms::{OrderRequest, StrategyContext};
use crate::strategies::Strategy;
use crate::{Candle, Position};

use super::config::VolatilityRegimeConfig;
use super::VolatilityRegime;

/// Pre-calculated indicators to avoid redundant computation
struct Indicators {
    atr_values: Vec<Option<f64>>,
    current_atr: Option<f64>,
    current_ema_fast: Option<f64>,
    current_ema_slow: Option<f64>,
    current_adx: Option<f64>,
}

impl Indicators {
    /// Calculate all indicators once from candle data
    fn new(candles: &[Candle], config: &VolatilityRegimeConfig) -> Self {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, config.atr_period);
        let ema_fast = ema(&close, config.ema_fast);
        let ema_slow = ema(&close, config.ema_slow);
        let adx_values = adx(&high, &low, &close, config.adx_period);

        Self {
            current_atr: atr_values.last().and_then(|&x| x),
            current_ema_fast: ema_fast.last().and_then(|&x| x),
            current_ema_slow: ema_slow.last().and_then(|&x| x),
            current_adx: adx_values.last().and_then(|&x| x),
            atr_values,
        }
    }

    /// Calculate ATR only (for stop/target/trailing methods)
    fn atr_only(candles: &[Candle], atr_period: usize) -> Option<f64> {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        atr(&high, &low, &close, atr_period).last().and_then(|&x| x)
    }
}

/// Volatility Regime Adaptive Strategy
pub struct VolatilityRegimeStrategy {
    config: VolatilityRegimeConfig,
}

impl VolatilityRegimeStrategy {
    pub fn new(config: VolatilityRegimeConfig) -> Self {
        VolatilityRegimeStrategy { config }
    }

    /// Classify volatility regime
    fn classify_regime(&self, candles: &[Candle], ind: &Indicators) -> Option<VolatilityRegime> {
        if candles.len() < self.config.volatility_lookback {
            return None;
        }

        let current_atr = ind.current_atr?;

        let lookback_start = candles
            .len()
            .saturating_sub(self.config.volatility_lookback);
        let lookback_atrs: Vec<f64> = ind.atr_values[lookback_start..]
            .iter()
            .filter_map(|&x| x)
            .collect();

        if lookback_atrs.is_empty() {
            return None;
        }

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
    fn is_trend_confirmed(&self, ind: &Indicators) -> bool {
        let fast = ind.current_ema_fast.unwrap_or(0.0);
        let slow = ind.current_ema_slow.unwrap_or(0.0);
        let adx_val = ind.current_adx.unwrap_or(0.0);
        fast > slow && adx_val > self.config.adx_threshold
    }

    /// Check for breakout
    fn is_breakout(&self, candles: &[Candle], ind: &Indicators) -> bool {
        if candles.len() < self.config.volatility_lookback + 1 {
            return false;
        }

        let current_atr = ind.current_atr.unwrap_or(0.0);
        let n = candles.len();
        let lookback = self.config.volatility_lookback;
        let start = n.saturating_sub(lookback + 1);
        let end = n - 1;

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
    fn name(&self) -> &'static str {
        "volatility_regime"
    }

    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        let mut orders = Vec::new();

        let min_warmup = (self.config.atr_period + 2 * self.config.adx_period)
            .max(self.config.atr_period + self.config.volatility_lookback)
            .max(self.config.ema_slow);

        if ctx.candles.len() < min_warmup {
            return orders;
        }

        // Calculate all indicators ONCE
        let ind = Indicators::new(ctx.candles, &self.config);
        let current_price = ctx.candles.last().unwrap().close;

        // Position exit logic
        if let Some(pos) = ctx.current_position {
            if let Some(VolatilityRegime::Extreme) = self.classify_regime(ctx.candles, &ind) {
                // Close position in extreme regime
                orders.push(OrderRequest::market_sell(ctx.symbol.clone(), pos.quantity));
                return orders;
            }

            if pos.unrealized_pnl >= 0.0 {
                if let Some(slow_ema) = ind.current_ema_slow {
                    if current_price < slow_ema {
                        // Exit profitable position when price crosses below slow EMA
                        orders.push(OrderRequest::market_sell(ctx.symbol.clone(), pos.quantity));
                        return orders;
                    }
                }
            }

            // Hold existing position
            return orders;
        }

        // Entry logic - only if no position
        let regime = match self.classify_regime(ctx.candles, &ind) {
            Some(r) => r,
            None => return orders,
        };

        if regime != VolatilityRegime::Compression && regime != VolatilityRegime::Normal {
            return orders;
        }

        if !self.is_trend_confirmed(&ind) {
            return orders;
        }

        if self.is_breakout(ctx.candles, &ind) {
            // Generate market buy order
            // Quantity will be determined by risk manager in backtest engine
            // For now, use a placeholder - backtest will override this
            orders.push(OrderRequest::market_buy(ctx.symbol.clone(), 1.0));
        }

        orders
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let current_atr =
            Indicators::atr_only(candles, self.config.atr_period).unwrap_or(entry_price * 0.05);
        entry_price - self.config.stop_atr_multiple * current_atr
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let current_atr =
            Indicators::atr_only(candles, self.config.atr_period).unwrap_or(entry_price * 0.05);
        entry_price + self.config.target_atr_multiple * current_atr
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        let current_atr =
            Indicators::atr_only(candles, self.config.atr_period).unwrap_or(current_price * 0.05);

        let profit_atr = if current_atr > 0.0 {
            (current_price - position.average_entry_price) / current_atr
        } else {
            0.0
        };

        // For now, no trailing stop until position has trailing_stop field or we track it in backtest
        // This is a simplified implementation
        if profit_atr >= self.config.trailing_activation {
            let new_stop = current_price - self.config.trailing_atr_multiple * current_atr;
            Some(new_stop)
        } else {
            None
        }
    }

    fn get_regime_score(&self, candles: &[Candle]) -> f64 {
        let ind = Indicators::new(candles, &self.config);
        match self.classify_regime(candles, &ind) {
            Some(VolatilityRegime::Compression) => 1.5,
            Some(VolatilityRegime::Normal) => 1.0,
            Some(VolatilityRegime::Expansion) => 0.8,
            Some(VolatilityRegime::Extreme) => 0.5,
            None => 1.0,
        }
    }
}
