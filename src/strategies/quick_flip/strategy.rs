//! Quick Flip Strategy - Range Breakout with Momentum
//!
//! Similar to range_breakout but with momentum confirmation:
//! 1. Identify N-bar high/low range
//! 2. Enter on breakout with optional strong candle filter
//! 3. ATR-based stop loss and take profit

use crate::indicators::atr;
use crate::oms::{Fill, OrderRequest, StrategyContext};
use crate::strategies::Strategy;
use crate::{Candle, Position, Side, Symbol, Trade};
use std::collections::HashMap;

use super::config::QuickFlipConfig;

pub struct QuickFlipStrategy {
    config: QuickFlipConfig,
    /// Per-symbol cooldown counters
    cooldown_counters: HashMap<Symbol, usize>,
}

impl QuickFlipStrategy {
    pub fn new(config: QuickFlipConfig) -> Self {
        Self {
            config,
            cooldown_counters: HashMap::new(),
        }
    }

    fn get_cooldown(&self, symbol: &Symbol) -> usize {
        *self.cooldown_counters.get(symbol).unwrap_or(&0)
    }

    /// Get range high from last N bars (excluding current)
    fn get_range_high(&self, candles: &[Candle]) -> Option<f64> {
        if candles.len() < self.config.range_bars + 1 {
            return None;
        }
        let start = candles.len() - self.config.range_bars - 1;
        let end = candles.len() - 1;
        candles[start..end]
            .iter()
            .map(|c| c.high)
            .fold(None, |max, h| Some(max.map_or(h, |m: f64| m.max(h))))
    }

    /// Get range low from last N bars (excluding current)
    fn get_range_low(&self, candles: &[Candle]) -> Option<f64> {
        if candles.len() < self.config.range_bars + 1 {
            return None;
        }
        let start = candles.len() - self.config.range_bars - 1;
        let end = candles.len() - 1;
        candles[start..end]
            .iter()
            .map(|c| c.low)
            .fold(None, |min, l| Some(min.map_or(l, |m: f64| m.min(l))))
    }

    /// Calculate ATR
    fn get_atr(&self, candles: &[Candle]) -> f64 {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_vals = atr(&high, &low, &close, self.config.atr_period);
        atr_vals
            .last()
            .and_then(|&x| x)
            .unwrap_or(candles.last().map(|c| c.close * 0.02).unwrap_or(0.0))
    }

    /// Check if candle is bullish with strong body
    fn is_strong_bullish(&self, candle: &Candle) -> bool {
        if candle.close <= candle.open {
            return false;
        }
        // If body_ratio is 0, any bullish candle qualifies
        if self.config.body_ratio <= 0.0 {
            return true;
        }
        let range = candle.high - candle.low;
        if range <= 0.0 {
            return false;
        }
        let body = candle.close - candle.open;
        body / range >= self.config.body_ratio
    }

    /// Check if candle is bearish with strong body
    fn is_strong_bearish(&self, candle: &Candle) -> bool {
        if candle.close >= candle.open {
            return false;
        }
        // If body_ratio is 0, any bearish candle qualifies
        if self.config.body_ratio <= 0.0 {
            return true;
        }
        let range = candle.high - candle.low;
        if range <= 0.0 {
            return false;
        }
        let body = candle.open - candle.close;
        body / range >= self.config.body_ratio
    }
}

impl Strategy for QuickFlipStrategy {
    fn name(&self) -> &'static str {
        "quick_flip"
    }

    fn clone_boxed(&self) -> Box<dyn Strategy> {
        Box::new(QuickFlipStrategy::new(self.config.clone()))
    }

    fn required_timeframes(&self) -> Vec<&'static str> {
        vec![]
    }

    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        let mut orders = Vec::new();

        let min_bars = self.config.range_bars + self.config.atr_period + 5;
        if ctx.candles.len() < min_bars {
            return orders;
        }

        // If in position, don't generate new entries
        if ctx.current_position.is_some() {
            return orders;
        }

        // Cooldown (per-symbol)
        if self.get_cooldown(ctx.symbol) > 0 {
            return orders;
        }

        // Get range boundaries
        let range_high = match self.get_range_high(ctx.candles) {
            Some(h) => h,
            None => return orders,
        };
        let range_low = match self.get_range_low(ctx.candles) {
            Some(l) => l,
            None => return orders,
        };

        let range_size = range_high - range_low;
        let current_atr = self.get_atr(ctx.candles);

        // Filter: range must be significant (not too tight)
        if self.config.min_range_pct > 0.0 && range_size < current_atr * self.config.min_range_pct {
            return orders;
        }

        let current = match ctx.candles.last() {
            Some(c) => c,
            None => return orders,
        };
        let prev = &ctx.candles[ctx.candles.len() - 2];

        // BREAKOUT LONG: Close breaks above range high
        let long_breakout = current.close > range_high && prev.close <= range_high;
        // BREAKOUT SHORT: Close breaks below range low
        let short_breakout = current.close < range_low && prev.close >= range_low;

        if long_breakout && (self.config.body_ratio <= 0.0 || self.is_strong_bullish(current)) {
            orders.push(OrderRequest::market_buy(ctx.symbol.clone(), 1.0));
            return orders;
        }

        if short_breakout
            && self.config.allow_shorts
            && (self.config.body_ratio <= 0.0 || self.is_strong_bearish(current))
        {
            orders.push(OrderRequest::market_sell(ctx.symbol.clone(), 1.0));
            return orders;
        }

        // REVERSAL trades (optional)
        if self.config.enable_reversals {
            let touch_zone = range_size * 0.1;

            // Reversal LONG at range low
            if current.low <= range_low + touch_zone
                && self.is_strong_bullish(current)
                && prev.close > range_low
            {
                orders.push(OrderRequest::market_buy(ctx.symbol.clone(), 1.0));
                return orders;
            }

            // Reversal SHORT at range high (only if shorts allowed)
            if self.config.allow_shorts
                && current.high >= range_high - touch_zone
                && self.is_strong_bearish(current)
                && prev.close < range_high
            {
                orders.push(OrderRequest::market_sell(ctx.symbol.clone(), 1.0));
            }
        }

        orders
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let current_atr = self.get_atr(candles);
        let stop_distance = self.config.stop_atr * current_atr;

        match side {
            Side::Buy => entry_price - stop_distance,
            Side::Sell => entry_price + stop_distance,
        }
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let current_atr = self.get_atr(candles);
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
        // Move stop to breakeven when we're 1 ATR in profit
        let current_atr = self.get_atr(candles);
        let entry = position.average_entry_price;

        match position.side {
            Side::Buy => {
                if current_price >= entry + current_atr {
                    let trail_stop = current_price - current_atr;
                    if trail_stop > entry {
                        return Some(trail_stop);
                    }
                    return Some(entry);
                }
                None
            }
            Side::Sell => {
                if current_price <= entry - current_atr {
                    let trail_stop = current_price + current_atr;
                    if trail_stop < entry {
                        return Some(trail_stop);
                    }
                    return Some(entry);
                }
                None
            }
        }
    }

    fn on_bar(&mut self, ctx: &StrategyContext) {
        // Decrement per-symbol cooldown
        if let Some(counter) = self.cooldown_counters.get_mut(ctx.symbol) {
            if *counter > 0 {
                *counter -= 1;
            }
        }
    }

    fn on_order_filled(&mut self, _fill: &Fill, _position: &Position) {}

    fn on_trade_closed(&mut self, trade: &Trade) {
        // Set per-symbol cooldown after trade closes
        self.cooldown_counters
            .insert(trade.symbol.clone(), self.config.cooldown);
    }

    fn init(&mut self) {
        self.cooldown_counters.clear();
    }
}
