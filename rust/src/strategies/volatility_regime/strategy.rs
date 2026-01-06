//! Volatility Regime Adaptive Strategy
//!
//! Strategy implementation for trading based on volatility regime classification.
//!
//! Performance optimized: Indicators are calculated incrementally in `on_bar`
//! using the `ta` crate and custom incremental ADX, avoiding O(N²) complexity.

use crate::indicators::IncrementalAdx;
use crate::indicators::make_data_item;
use crate::oms::{OrderRequest, StrategyContext};
use crate::strategies::Strategy;
use crate::{Candle, Position};
use chrono::{DateTime, Utc};
use ta::indicators::{AverageTrueRange, ExponentialMovingAverage};
use ta::Next;

use super::config::VolatilityRegimeConfig;
use super::VolatilityRegime;

/// Volatility Regime Adaptive Strategy
pub struct VolatilityRegimeStrategy {
    config: VolatilityRegimeConfig,
    
    // Stateful Indicators
    ema_fast: ExponentialMovingAverage,
    ema_slow: ExponentialMovingAverage,
    atr: AverageTrueRange,
    adx: IncrementalAdx,

    // State Tracking
    last_processed_time: Option<DateTime<Utc>>,
    atr_history: Vec<f64>,
    
    // Cached Values for O(1) access
    current_ema_fast: f64,
    current_ema_slow: f64,
    current_atr: f64,
    current_adx: f64,
    current_regime: Option<VolatilityRegime>,
}

impl VolatilityRegimeStrategy {
    pub fn new(config: VolatilityRegimeConfig) -> Self {
        let ema_fast = ExponentialMovingAverage::new(config.ema_fast).unwrap();
        let ema_slow = ExponentialMovingAverage::new(config.ema_slow).unwrap();
        let atr = AverageTrueRange::new(config.atr_period).unwrap();
        let adx = IncrementalAdx::new(config.adx_period);

        Self {
            config,
            ema_fast,
            ema_slow,
            atr,
            adx,
            last_processed_time: None,
            atr_history: Vec::new(),
            current_ema_fast: 0.0,
            current_ema_slow: 0.0,
            current_atr: 0.0,
            current_adx: 0.0,
            current_regime: None,
        }
    }

    /// Classify volatility regime using cached values (O(1))
    fn update_regime(&mut self) {
        if self.atr_history.len() < self.config.volatility_lookback {
            self.current_regime = None;
            return;
        }

        let lookback_start = self.atr_history.len().saturating_sub(self.config.volatility_lookback);
        // Calculate mean of recent ATRs
        let sum: f64 = self.atr_history[lookback_start..].iter().sum();
        let count = self.atr_history.len() - lookback_start;
        
        if count == 0 {
            self.current_regime = None;
            return;
        }

        let atr_mean = sum / count as f64;
        
        if atr_mean == 0.0 {
            self.current_regime = Some(VolatilityRegime::Normal);
            return;
        }

        let atr_ratio = self.current_atr / atr_mean;

        self.current_regime = if atr_ratio >= self.config.extreme_threshold {
            Some(VolatilityRegime::Extreme)
        } else if atr_ratio >= self.config.expansion_threshold {
            Some(VolatilityRegime::Expansion)
        } else if atr_ratio <= self.config.compression_threshold {
            Some(VolatilityRegime::Compression)
        } else {
            Some(VolatilityRegime::Normal)
        };
    }

    /// Check if trend is confirmed using cached values (O(1))
    fn is_trend_confirmed(&self) -> bool {
        self.current_ema_fast > self.current_ema_slow && self.current_adx > self.config.adx_threshold
    }

    /// Check for breakout (O(N) over lookback window, but N is small constant)
    fn is_breakout(&self, candles: &[Candle]) -> bool {
        let n = candles.len();
        let lookback = self.config.volatility_lookback;
        
        if n < lookback + 1 {
            return false;
        }

        let start = n.saturating_sub(lookback + 1);
        let end = n - 1;

        let recent_high = candles[start..end]
            .iter()
            .map(|c| c.high)
            .fold(f64::MIN, f64::max);

        let breakout_level = recent_high - self.config.breakout_atr_multiple * self.current_atr;
        let current_close = candles.last().unwrap().close;
        let prev_close = candles[n - 2].close;

        current_close > breakout_level && prev_close <= breakout_level
    }
}

impl Strategy for VolatilityRegimeStrategy {
    fn name(&self) -> &'static str {
        "volatility_regime"
    }

    fn on_bar(&mut self, ctx: &StrategyContext) {
        // Incremental update of indicators
        if ctx.candles.is_empty() {
            return;
        }

        let start_idx = if let Some(last_time) = self.last_processed_time {
            if ctx.candles.last().unwrap().datetime > last_time {
                ctx.candles.iter()
                    .position(|c| c.datetime > last_time)
                    .unwrap_or(ctx.candles.len())
            } else {
                return; // Already processed
            }
        } else {
            0
        };

        for candle in &ctx.candles[start_idx..] {
            self.current_ema_fast = self.ema_fast.next(candle.close);
            self.current_ema_slow = self.ema_slow.next(candle.close);
            
            let item = make_data_item(candle.open, candle.high, candle.low, candle.close, candle.volume);
            self.current_atr = self.atr.next(&item);
            
            self.current_adx = self.adx.next(candle.high, candle.low, candle.close);

            // Maintain history
            self.atr_history.push(self.current_atr);
            
            if self.atr_history.len() > self.config.volatility_lookback * 2 + 1000 {
                let remove_count = self.atr_history.len() - (self.config.volatility_lookback + 100);
                self.atr_history.drain(0..remove_count);
            }

            self.last_processed_time = Some(candle.datetime);
        }

        // Update derived state
        self.update_regime();
    }

    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        let mut orders = Vec::new();
        
        // Use cached regime
        let regime = match self.current_regime {
            Some(r) => r,
            None => return orders, // Not warmed up
        };

        // Position exit logic
        if let Some(pos) = ctx.current_position {
            if regime == VolatilityRegime::Extreme {
                // Close position in extreme regime
                orders.push(OrderRequest::market_sell(ctx.symbol.clone(), pos.quantity));
                return orders;
            }

            // Early exit with minimum profit threshold
            let min_profit_pct = 0.015;
            let min_profit_amount = pos.quantity * pos.average_entry_price * min_profit_pct;
            let current_price = ctx.candles.last().map(|c| c.close).unwrap_or(0.0);

            if pos.unrealized_pnl >= min_profit_amount {
                if current_price < self.current_ema_slow {
                    orders.push(OrderRequest::market_sell(ctx.symbol.clone(), pos.quantity));
                    return orders;
                }
            }
            return orders;
        }

        // Entry logic
        if regime != VolatilityRegime::Compression && regime != VolatilityRegime::Normal {
            return orders;
        }

        if !self.is_trend_confirmed() {
            return orders;
        }

        if self.is_breakout(ctx.candles) {
            orders.push(OrderRequest::market_buy(ctx.symbol.clone(), 1.0));
        }

        orders
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        use crate::indicators::atr;
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        
        let atr_val = atr(&high, &low, &close, self.config.atr_period)
            .last()
            .and_then(|&x| x)
            .unwrap_or(entry_price * 0.05);

        entry_price - self.config.stop_atr_multiple * atr_val
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        use crate::indicators::atr;
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        
        let atr_val = atr(&high, &low, &close, self.config.atr_period)
            .last()
            .and_then(|&x| x)
            .unwrap_or(entry_price * 0.05);

        entry_price + self.config.target_atr_multiple * atr_val
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        let current_atr = if self.current_atr > 0.0 {
            self.current_atr
        } else {
            current_price * 0.05
        };

        let profit_atr = if current_atr > 0.0 {
            (current_price - position.average_entry_price) / current_atr
        } else {
            0.0
        };

        if profit_atr >= self.config.trailing_activation {
            let new_stop = current_price - self.config.trailing_atr_multiple * current_atr;
            Some(new_stop)
        } else {
            None
        }
    }

    fn get_regime_score(&self, _candles: &[Candle]) -> f64 {
        match self.current_regime {
            Some(VolatilityRegime::Compression) => 1.5,
            Some(VolatilityRegime::Normal) => 1.0,
            Some(VolatilityRegime::Expansion) => 0.8,
            Some(VolatilityRegime::Extreme) => 0.5,
            None => 1.0,
        }
    }
}
