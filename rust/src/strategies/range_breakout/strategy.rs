//! Range Breakout Strategy - Simple and Fast
//!
//! Entry: Close breaks above highest high of last N bars
//! Exit: ATR-based stop/target
//!
//! Production-grade implementation with per-symbol cooldown tracking.

use crate::indicators::atr;
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

    fn clone_boxed(&self) -> Box<dyn Strategy> {
        Box::new(RangeBreakoutStrategy::new(self.config.clone()))
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

        // Cooldown (per-symbol)
        if self.get_cooldown(ctx.symbol) > 0 {
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

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_vals = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_vals
            .last()
            .and_then(|&x| x)
            .unwrap_or(entry_price * 0.02);
        let stop_distance = self.config.stop_atr * current_atr;

        match side {
            Side::Buy => entry_price - stop_distance,
            Side::Sell => entry_price + stop_distance,
        }
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_vals = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_vals
            .last()
            .and_then(|&x| x)
            .unwrap_or(entry_price * 0.02);
        let target_distance = self.config.target_atr * current_atr;

        match side {
            Side::Buy => entry_price + target_distance,
            Side::Sell => entry_price - target_distance,
        }
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
