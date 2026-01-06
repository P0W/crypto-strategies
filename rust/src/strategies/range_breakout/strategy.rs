//! Range Breakout Strategy - Simple and Fast
//!
//! Entry: Close breaks above highest high of last N bars
//! Exit: ATR-based stop/target

use crate::indicators::atr;
use crate::oms::{Fill, OrderRequest, StrategyContext};
use crate::strategies::Strategy;
use crate::{Candle, Position, Trade};

use super::config::RangeBreakoutConfig;

pub struct RangeBreakoutStrategy {
    config: RangeBreakoutConfig,
    cooldown_counter: usize,
}

impl RangeBreakoutStrategy {
    pub fn new(config: RangeBreakoutConfig) -> Self {
        Self {
            config,
            cooldown_counter: 0,
        }
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
}

impl Strategy for RangeBreakoutStrategy {
    fn name(&self) -> &'static str {
        "range_breakout"
    }

    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        let mut orders = Vec::new();

        let min_bars = self.config.lookback + self.config.atr_period + 2;
        if ctx.candles.len() < min_bars {
            return orders;
        }

        // If in position, hold
        if ctx.current_position.is_some() {
            return orders;
        }

        // Cooldown
        if self.cooldown_counter > 0 {
            return orders;
        }

        let current = ctx.candles.last().unwrap();
        let prev = &ctx.candles[ctx.candles.len() - 2];

        // Get range high (excluding current bar)
        let range_high = match self.get_range_high(ctx.candles) {
            Some(h) => h,
            None => return orders,
        };

        // Breakout: current close > range high AND previous close <= range high
        // Plus volatility filter to avoid false breakouts
        if current.close > range_high
            && prev.close <= range_high
            && self.is_volatility_expanding(ctx.candles)
        {
            orders.push(OrderRequest::market_buy(ctx.symbol.clone(), 1.0));
        }

        orders
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_vals = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_vals
            .last()
            .and_then(|&x| x)
            .unwrap_or(entry_price * 0.02);

        entry_price - self.config.stop_atr * current_atr
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_vals = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_vals
            .last()
            .and_then(|&x| x)
            .unwrap_or(entry_price * 0.02);

        entry_price + self.config.target_atr * current_atr
    }

    fn update_trailing_stop(
        &self,
        _position: &Position,
        _current_price: f64,
        _candles: &[Candle],
    ) -> Option<f64> {
        None // No trailing for simplicity
    }

    fn on_order_filled(&mut self, _fill: &Fill, _position: &Position) {
        // Don't set cooldown here - it's called on BOTH entry and exit fills
        // Cooldown should only be set when trade closes (see on_trade_closed)
    }

    fn on_trade_closed(&mut self, _trade: &Trade) {
        // Set cooldown after trade closes (matches main branch behavior)
        self.cooldown_counter = self.config.cooldown;
    }

    fn on_bar(&mut self, _ctx: &StrategyContext) {
        if self.cooldown_counter > 0 {
            self.cooldown_counter -= 1;
        }
    }

    fn init(&mut self) {
        self.cooldown_counter = 0;
    }
}
