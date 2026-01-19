//! Quick Flip Strategy - Range Breakout with Momentum
//!
//! Similar to range_breakout but with momentum confirmation:
//! 1. Identify N-bar high/low range
//! 2. Enter on breakout with optional strong candle filter
//! 3. ATR-based stop loss and take profit

use crate::oms::{Fill, OrderRequest, StrategyContext};
use crate::strategies::{atr_stop_loss, atr_take_profit, current_atr, CooldownManager, Strategy};
use crate::{Candle, Position, Side, Trade};

use super::config::QuickFlipConfig;

pub struct QuickFlipStrategy {
    config: QuickFlipConfig,
    cooldown: CooldownManager,
}

impl QuickFlipStrategy {
    pub fn new(config: QuickFlipConfig) -> Self {
        Self {
            config,
            cooldown: CooldownManager::new(),
        }
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

    /// Calculate current ATR value with default fallback
    fn get_atr_or_default(&self, candles: &[Candle]) -> f64 {
        let default = candles.last().map(|c| c.close * 0.02).unwrap_or(0.0);
        current_atr(candles, self.config.atr_period).unwrap_or(default)
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
        if self.cooldown.is_active(ctx.symbol) {
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
        let current_atr = self.get_atr_or_default(ctx.candles);

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
        let atr = self.get_atr_or_default(candles);
        atr_stop_loss(entry_price, atr, self.config.stop_atr, side)
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let atr = self.get_atr_or_default(candles);
        atr_take_profit(entry_price, atr, self.config.target_atr, side)
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        let atr = self.get_atr_or_default(candles);
        let entry = position.average_entry_price.to_f64();

        match position.side {
            Side::Buy => {
                if current_price >= entry + atr {
                    let trail_stop = current_price - atr;
                    if trail_stop > entry {
                        return Some(trail_stop);
                    }
                    return Some(entry);
                }
                None
            }
            Side::Sell => {
                if current_price <= entry - atr {
                    let trail_stop = current_price + atr;
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
        self.cooldown.decrement(ctx.symbol);
    }

    fn on_order_filled(&mut self, _fill: &Fill, _position: &Position) {}

    fn on_trade_closed(&mut self, trade: &Trade) {
        self.cooldown
            .set(trade.symbol.clone(), self.config.cooldown);
    }

    fn init(&mut self) {
        self.cooldown.clear();
    }
}
