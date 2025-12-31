//! Range Breakout Strategy - Simple and Fast
//!
//! Entry: Close breaks above highest high of last N bars
//! Exit: ATR-based stop/target

use crate::indicators::atr;
use crate::strategies::Strategy;
use crate::{Candle, Position, Signal, Symbol, Trade};

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

    #[allow(dead_code)]
    fn get_range_low(&self, candles: &[Candle]) -> Option<f64> {
        if candles.len() < self.config.lookback + 1 {
            return None;
        }
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
}

impl Strategy for RangeBreakoutStrategy {
    fn name(&self) -> &'static str {
        "range_breakout"
    }

    fn generate_signal(
        &self,
        _symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal {
        let min_bars = self.config.lookback + self.config.atr_period + 2;
        if candles.len() < min_bars {
            return Signal::Flat;
        }

        // If in position, hold
        if position.is_some() {
            return Signal::Long;
        }

        // Cooldown
        if self.cooldown_counter > 0 {
            return Signal::Flat;
        }

        let current = candles.last().unwrap();
        let prev = &candles[candles.len() - 2];

        // Get range high (excluding current bar)
        let range_high = match self.get_range_high(candles) {
            Some(h) => h,
            None => return Signal::Flat,
        };

        // Breakout: current close > range high AND previous close <= range high
        // Plus volatility filter to avoid false breakouts
        if current.close > range_high
            && prev.close <= range_high
            && self.is_volatility_expanding(candles)
        {
            return Signal::Long;
        }

        Signal::Flat
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

    fn notify_trade(&mut self, trade: &Trade) {
        self.cooldown_counter = self.config.cooldown;
        tracing::debug!(
            symbol = %trade.symbol,
            pnl = format!("{:.2}", trade.net_pnl),
            "Range breakout trade"
        );
    }

    fn init(&mut self) {
        self.cooldown_counter = 0;
    }
}
