//! Range Breakout Strategy - Simple and Fast
//!
//! Entry: Close breaks above highest high (long) or below lowest low (short) of last N bars
//! Exit: ATR-based stop/target with optional trailing stop
//!
//! Filters:
//! - Trend: Price must be above/below EMA for long/short
//! - ADX: Market must be trending (ADX > min_adx)
//! - Volume: Breakout bar must have above-average volume
//! - Volatility: ATR must be expanding
//!
//! Production-grade implementation with per-symbol cooldown tracking.

use crate::indicators::{adx, atr, ema};
use crate::oms::{Fill, OrderRequest, StrategyContext};
use crate::strategies::Strategy;
use crate::{Candle, Position, Side, Symbol, Trade};
use std::collections::HashMap;

use super::config::RangeBreakoutConfig;

pub struct RangeBreakoutStrategy {
    config: RangeBreakoutConfig,
    /// Per-symbol cooldown counters
    cooldown_counters: HashMap<Symbol, usize>,
}

impl RangeBreakoutStrategy {
    pub fn new(config: RangeBreakoutConfig) -> Self {
        Self {
            config,
            cooldown_counters: HashMap::new(),
        }
    }

    fn get_cooldown(&self, symbol: &Symbol) -> usize {
        *self.cooldown_counters.get(symbol).unwrap_or(&0)
    }

    fn get_range_high(&self, candles: &[Candle]) -> Option<f64> {
        if candles.len() < self.config.lookback + 1 {
            return None;
        }
        // Exclude current bar, look at previous N bars
        let start = candles.len() - self.config.lookback - 1;
        let end = candles.len() - 1;
        candles[start..end]
            .iter()
            .map(|c| c.high)
            .fold(None, |max, h| Some(max.map_or(h, |m: f64| m.max(h))))
    }

    fn get_range_low(&self, candles: &[Candle]) -> Option<f64> {
        if candles.len() < self.config.lookback + 1 {
            return None;
        }
        // Exclude current bar, look at previous N bars
        let start = candles.len() - self.config.lookback - 1;
        let end = candles.len() - 1;
        candles[start..end]
            .iter()
            .map(|c| c.low)
            .fold(None, |min, l| Some(min.map_or(l, |m: f64| m.min(l))))
    }

    /// Check if volatility is expanding (good for breakouts)
    fn is_volatility_expanding(&self, candles: &[Candle]) -> bool {
        if candles.len() < self.config.atr_period * 2 {
            return true;
        }

        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_vals = atr(&high, &low, &close, self.config.atr_period);
        let len = atr_vals.len();
        if len < 5 {
            return true;
        }

        let current = atr_vals[len - 1].unwrap_or(0.0);
        let prev = atr_vals[len - 5].unwrap_or(0.0);

        // ATR should be increasing for valid breakouts
        current > prev * 0.9
    }

    /// Check if volume confirms the breakout
    fn is_volume_confirming(&self, candles: &[Candle]) -> bool {
        if candles.len() < 20 {
            return true;
        }

        // Get average volume of last 20 bars (excluding current)
        let start = candles.len().saturating_sub(21);
        let end = candles.len() - 1;
        let avg_vol: f64 =
            candles[start..end].iter().map(|c| c.volume).sum::<f64>() / (end - start) as f64;

        let current_vol = match candles.last() {
            Some(c) => c.volume,
            None => return true,
        };

        // Volume should be at least 80% of average for valid breakout
        current_vol > avg_vol * 0.8
    }

    /// Check trend direction using EMA
    /// Returns: (is_bullish, is_bearish)
    fn get_trend_direction(&self, candles: &[Candle]) -> (bool, bool) {
        if self.config.trend_ema == 0 || candles.len() < self.config.trend_ema + 1 {
            return (true, true); // No filter if disabled or insufficient data
        }

        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let ema_vals = ema(&close, self.config.trend_ema);

        let current_close = match candles.last() {
            Some(c) => c.close,
            None => return (true, true),
        };
        let current_ema = ema_vals.last().and_then(|&x| x).unwrap_or(current_close);

        let is_bullish = current_close > current_ema;
        let is_bearish = current_close < current_ema;

        (is_bullish, is_bearish)
    }

    /// Check if market is trending (ADX filter)
    fn is_trending(&self, candles: &[Candle]) -> bool {
        if self.config.min_adx == 0.0 {
            return true; // Disabled
        }

        if candles.len() < self.config.adx_period * 2 {
            return true; // Insufficient data
        }

        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let adx_vals = adx(&high, &low, &close, self.config.adx_period);
        let current_adx = adx_vals.last().and_then(|&x| x).unwrap_or(0.0);

        current_adx >= self.config.min_adx
    }

    /// Calculate current ATR value
    fn get_current_atr(&self, candles: &[Candle]) -> f64 {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_vals = atr(&high, &low, &close, self.config.atr_period);
        atr_vals
            .last()
            .and_then(|&x| x)
            .unwrap_or(candles.last().map(|c| c.close * 0.02).unwrap_or(0.0))
    }
}

impl Strategy for RangeBreakoutStrategy {
    fn name(&self) -> &'static str {
        "range_breakout"
    }

    fn clone_boxed(&self) -> Box<dyn Strategy> {
        Box::new(RangeBreakoutStrategy::new(self.config.clone()))
    }

    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        let mut orders = Vec::new();

        let min_bars = self.config.lookback.max(self.config.trend_ema) + self.config.atr_period + 2;
        if ctx.candles.len() < min_bars {
            return orders;
        }

        // If in position, hold
        if ctx.current_position.is_some() {
            return orders;
        }

        // Cooldown (per-symbol)
        if self.get_cooldown(ctx.symbol) > 0 {
            return orders;
        }

        let current = match ctx.candles.last() {
            Some(c) => c,
            None => return orders,
        };
        let prev = &ctx.candles[ctx.candles.len() - 2];

        // Get range boundaries (excluding current bar)
        let range_high = match self.get_range_high(ctx.candles) {
            Some(h) => h,
            None => return orders,
        };
        let range_low = match self.get_range_low(ctx.candles) {
            Some(l) => l,
            None => return orders,
        };

        // Check all filters
        let vol_expanding = self.is_volatility_expanding(ctx.candles);
        let vol_confirming = self.is_volume_confirming(ctx.candles);
        let is_trending = self.is_trending(ctx.candles);
        let (is_bullish, is_bearish) = self.get_trend_direction(ctx.candles);

        // Long breakout: current close > range high AND previous close <= range high
        // Plus: trend is up (price > EMA), market is trending (ADX > min)
        if current.close > range_high
            && prev.close <= range_high
            && vol_expanding
            && vol_confirming
            && is_trending
            && is_bullish
        {
            orders.push(OrderRequest::market_buy(ctx.symbol.clone(), 1.0));
        }
        // Short breakout: current close < range low AND previous close >= range low
        // Plus: trend is down (price < EMA), market is trending (ADX > min)
        else if self.config.allow_shorts
            && current.close < range_low
            && prev.close >= range_low
            && vol_expanding
            && vol_confirming
            && is_trending
            && is_bearish
        {
            orders.push(OrderRequest::market_sell(ctx.symbol.clone(), 1.0));
        }

        orders
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let current_atr = self.get_current_atr(candles);
        let stop_distance = self.config.stop_atr * current_atr;

        match side {
            Side::Buy => entry_price - stop_distance,
            Side::Sell => entry_price + stop_distance,
        }
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let current_atr = self.get_current_atr(candles);
        let target_distance = self.config.target_atr * current_atr;

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
        if !self.config.use_trailing {
            return None;
        }

        let current_atr = self.get_current_atr(candles);
        let trail_distance = self.config.trailing_atr * current_atr;
        let entry_price = position.average_entry_price.to_f64();

        match position.side {
            Side::Buy => {
                if current_price > entry_price {
                    let new_stop = current_price - trail_distance;
                    let min_stop = entry_price - (self.config.stop_atr * current_atr);
                    if new_stop > min_stop {
                        return Some(new_stop);
                    }
                }
                None
            }
            Side::Sell => {
                if current_price < entry_price {
                    let new_stop = current_price + trail_distance;
                    let max_stop = entry_price + (self.config.stop_atr * current_atr);
                    if new_stop < max_stop {
                        return Some(new_stop);
                    }
                }
                None
            }
        }
    }

    fn on_order_filled(&mut self, _fill: &Fill, _position: &Position) {
        // Don't set cooldown here - it's called on BOTH entry and exit fills
        // Cooldown should only be set when trade closes (see on_trade_closed)
    }

    fn on_trade_closed(&mut self, trade: &Trade) {
        // Set per-symbol cooldown after trade closes
        self.cooldown_counters
            .insert(trade.symbol.clone(), self.config.cooldown);
    }

    fn on_bar(&mut self, ctx: &StrategyContext) {
        // Decrement per-symbol cooldown
        if let Some(counter) = self.cooldown_counters.get_mut(ctx.symbol) {
            if *counter > 0 {
                *counter -= 1;
            }
        }
    }

    fn init(&mut self) {
        self.cooldown_counters.clear();
    }
}
