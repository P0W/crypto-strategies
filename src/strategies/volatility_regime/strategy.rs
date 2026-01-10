//! Volatility Regime Adaptive Strategy
//!
//! Strategy implementation for trading based on volatility regime classification.
//!
//! Uses batch indicator calculation per signal, matching the proven main branch
//! implementation for consistent signal generation.

use crate::indicators::{adx, atr, ema};
use crate::oms::{OrderRequest, StrategyContext};
use crate::strategies::Strategy;
use crate::{Candle, Position, Side};

use super::config::VolatilityRegimeConfig;
use super::VolatilityRegime;

/// Pre-calculated indicators to avoid redundant computation within a single call
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
///
/// Uses batch indicator calculation matching main branch for proven results.
pub struct VolatilityRegimeStrategy {
    config: VolatilityRegimeConfig,
}

impl VolatilityRegimeStrategy {
    pub fn new(config: VolatilityRegimeConfig) -> Self {
        Self { config }
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
        let current_close = match candles.last() {
            Some(c) => c.close,
            None => return false,
        };
        let prev_close = candles[n - 2].close;

        current_close > breakout_level && prev_close <= breakout_level
    }
}

impl Strategy for VolatilityRegimeStrategy {
    fn name(&self) -> &'static str {
        "volatility_regime"
    }

    fn clone_boxed(&self) -> Box<dyn Strategy> {
        Box::new(VolatilityRegimeStrategy::new(self.config.clone()))
    }

    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        let candles = ctx.candles;
        let mut orders = Vec::new();

        // Minimum warmup check matching main branch
        let min_warmup = (self.config.atr_period + 2 * self.config.adx_period)
            .max(self.config.atr_period + self.config.volatility_lookback)
            .max(self.config.ema_slow);

        if candles.len() < min_warmup {
            return orders;
        }

        // Calculate all indicators ONCE
        let ind = Indicators::new(candles, &self.config);
        let current_price = match candles.last() {
            Some(c) => c.close,
            None => return orders,
        };

        // Position exit logic
        if let Some(pos) = ctx.current_position {
            if let Some(VolatilityRegime::Extreme) = self.classify_regime(candles, &ind) {
                orders.push(OrderRequest::market_sell(
                    ctx.symbol.clone(),
                    pos.quantity.to_f64(),
                ));
                return orders;
            }

            // Early exit: if profitable and price below slow EMA
            if pos.unrealized_pnl(current_price) >= 0.0 {
                if let Some(slow_ema) = ind.current_ema_slow {
                    if current_price < slow_ema {
                        orders.push(OrderRequest::market_sell(
                            ctx.symbol.clone(),
                            pos.quantity.to_f64(),
                        ));
                        return orders;
                    }
                }
            }

            return orders; // Hold position
        }

        // Entry logic
        let regime = match self.classify_regime(candles, &ind) {
            Some(r) => r,
            None => return orders,
        };

        if regime != VolatilityRegime::Compression && regime != VolatilityRegime::Normal {
            return orders;
        }

        if !self.is_trend_confirmed(&ind) {
            return orders;
        }

        if self.is_breakout(candles, &ind) {
            orders.push(OrderRequest::market_buy(ctx.symbol.clone(), 1.0));
        }

        orders
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let current_atr =
            Indicators::atr_only(candles, self.config.atr_period).unwrap_or(entry_price * 0.05);
        let stop_distance = self.config.stop_atr_multiple * current_atr;

        match side {
            Side::Buy => entry_price - stop_distance,
            Side::Sell => entry_price + stop_distance,
        }
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let current_atr =
            Indicators::atr_only(candles, self.config.atr_period).unwrap_or(entry_price * 0.05);
        let target_distance = self.config.target_atr_multiple * current_atr;

        match side {
            Side::Buy => entry_price + target_distance,
            Side::Sell => entry_price - target_distance,
        }
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        let current_atr =
            Indicators::atr_only(candles, self.config.atr_period).unwrap_or(current_price * 0.05);

        if current_atr <= 0.0 {
            return None;
        }

        let entry_price = position.average_entry_price.to_f64();

        match position.side {
            Side::Buy => {
                let profit_atr = (current_price - entry_price) / current_atr;
                if profit_atr >= self.config.trailing_activation {
                    let new_stop = current_price - self.config.trailing_atr_multiple * current_atr;
                    let entry_stop = entry_price - self.config.stop_atr_multiple * current_atr;
                    Some(new_stop.max(entry_stop))
                } else {
                    None
                }
            }
            Side::Sell => {
                let profit_atr = (entry_price - current_price) / current_atr;
                if profit_atr >= self.config.trailing_activation {
                    let new_stop = current_price + self.config.trailing_atr_multiple * current_atr;
                    let entry_stop = entry_price + self.config.stop_atr_multiple * current_atr;
                    Some(new_stop.min(entry_stop))
                } else {
                    None
                }
            }
        }
    }

    fn get_regime_score(&self, candles: &[Candle]) -> f64 {
        let ind = Indicators::new(candles, &self.config);

        match self.classify_regime(candles, &ind) {
            Some(VolatilityRegime::Compression) | Some(VolatilityRegime::Normal) => {
                if self.is_trend_confirmed(&ind) {
                    1.0
                } else {
                    0.5
                }
            }
            _ => 0.0,
        }
    }
}
