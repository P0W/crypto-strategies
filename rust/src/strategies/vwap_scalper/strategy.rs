//! VWAP Scalper Strategy Implementation
//!
//! ## Entry Logic
//! 1. Price crosses above VWAP -> Long
//! 2. Price crosses below VWAP -> Short (if allowed)
//! 3. Optional: Volume confirmation
//! 4. Optional: Max distance from VWAP filter
//!
//! ## Exit Logic
//! 1. Stop loss at entry - stop ATR multiple
//! 2. Take profit at entry + target ATR multiple
//! 3. Trailing stop after activation threshold
//! 4. Max hold bars exceeded

use crate::indicators::{atr, sma, vwap};
use crate::strategies::Strategy;
use crate::{Candle, Order, OrderStatus, Position, Signal, Symbol, Trade};

use super::config::VwapScalperConfig;

/// VWAP Scalper Strategy
pub struct VwapScalperStrategy {
    config: VwapScalperConfig,
    bars_in_position: usize,
    cooldown_counter: usize,
}

impl VwapScalperStrategy {
    pub fn new(config: VwapScalperConfig) -> Self {
        VwapScalperStrategy {
            config,
            bars_in_position: 0,
            cooldown_counter: 0,
        }
    }

    /// Check VWAP crossover signal OR price position relative to VWAP
    fn check_vwap_crossover(&self, candles: &[Candle]) -> Option<Signal> {
        if candles.len() < 3 {
            return None;
        }

        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let volume: Vec<f64> = candles.iter().map(|c| c.volume).collect();

        let vwap_values = vwap(&high, &low, &close, &volume);

        if vwap_values.len() < 2 {
            return None;
        }

        let len = vwap_values.len();
        let vwap_curr = vwap_values[len - 1];
        let vwap_prev = vwap_values[len - 2];

        let close_curr = close[len - 1];
        let close_prev = close[len - 2];
        let low_curr = low[len - 1];
        let high_curr = high[len - 1];

        // Bullish crossover: price crosses above VWAP
        if close_prev <= vwap_prev && close_curr > vwap_curr {
            return Some(Signal::Long);
        }

        // Bearish crossover: price crosses below VWAP
        if close_prev >= vwap_prev && close_curr < vwap_curr {
            return Some(Signal::Short);
        }

        // Additional: Price bouncing off VWAP (wicks touching)
        // Bullish: Low touches/penetrates VWAP, close above
        if low_curr <= vwap_curr && close_curr > vwap_curr {
            return Some(Signal::Long);
        }

        // Bearish: High touches/penetrates VWAP, close below
        if high_curr >= vwap_curr && close_curr < vwap_curr {
            return Some(Signal::Short);
        }

        None
    }

    /// Check if price is within acceptable distance from VWAP
    fn check_distance_filter(&self, candles: &[Candle]) -> bool {
        if candles.len() < 2 {
            return false;
        }

        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let volume: Vec<f64> = candles.iter().map(|c| c.volume).collect();

        let vwap_values = vwap(&high, &low, &close, &volume);
        let current_vwap = vwap_values.last().copied().unwrap_or(0.0);
        let current_close = candles.last().map(|c| c.close).unwrap_or(0.0);

        if current_vwap <= 0.0 {
            return false;
        }

        let distance_pct = ((current_close - current_vwap) / current_vwap).abs() * 100.0;
        distance_pct <= self.config.max_distance_pct
    }

    /// Check volume condition
    fn check_volume(&self, candles: &[Candle]) -> bool {
        if !self.config.require_volume {
            return true;
        }

        if candles.len() < self.config.volume_period + 1 {
            return true;
        }

        let volumes: Vec<f64> = candles.iter().map(|c| c.volume).collect();
        let volume_ma = sma(&volumes, self.config.volume_period);

        let current_volume = candles.last().map(|c| c.volume).unwrap_or(0.0);
        let avg_volume = volume_ma.last().and_then(|&x| x).unwrap_or(1.0);

        if avg_volume <= 0.0 {
            return true;
        }

        current_volume >= avg_volume * self.config.volume_threshold
    }

    /// Get current ATR value
    fn get_current_atr(&self, candles: &[Candle]) -> f64 {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        atr_values
            .last()
            .and_then(|&x| x)
            .unwrap_or_else(|| candles.last().map(|c| c.close * 0.01).unwrap_or(0.0))
    }
}

impl Strategy for VwapScalperStrategy {
    fn name(&self) -> &'static str {
        "vwap_scalper"
    }

    fn generate_signal(
        &self,
        _symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal {
        // Minimum data requirement
        let min_bars = self.config.atr_period.max(self.config.volume_period) + 5;

        if candles.len() < min_bars {
            return Signal::Flat;
        }

        // Check cooldown
        if self.cooldown_counter > 0 && position.is_none() {
            return Signal::Flat;
        }

        // If in position, check exit conditions
        if let Some(_pos) = position {
            // Exit on max hold bars
            if self.bars_in_position >= self.config.max_hold_bars {
                return Signal::Flat;
            }

            // Hold position - let stops/targets handle exit
            return Signal::Long;
        }

        // Entry logic - check VWAP crossover
        let crossover = self.check_vwap_crossover(candles);

        if crossover.is_none() {
            return Signal::Flat;
        }

        let signal = crossover.unwrap();

        // Filter: distance from VWAP
        if !self.check_distance_filter(candles) {
            return Signal::Flat;
        }

        // Filter: Volume
        if !self.check_volume(candles) {
            return Signal::Flat;
        }

        // Return signal (only long if short not allowed)
        if signal == Signal::Short && !self.config.allow_short {
            return Signal::Flat;
        }

        signal
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let current_atr = self.get_current_atr(candles);
        entry_price - self.config.stop_atr_multiple * current_atr
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let current_atr = self.get_current_atr(candles);
        entry_price + self.config.target_atr_multiple * current_atr
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        let current_atr = self.get_current_atr(candles);

        let profit_atr = if current_atr > 0.0 {
            (current_price - position.entry_price) / current_atr
        } else {
            0.0
        };

        let current_stop = position.trailing_stop.unwrap_or(position.stop_price);

        if profit_atr >= self.config.trailing_activation {
            let new_stop = current_price - self.config.trailing_atr_multiple * current_atr;
            if new_stop > current_stop {
                Some(new_stop)
            } else {
                Some(current_stop)
            }
        } else if position.trailing_stop.is_some() {
            Some(current_stop)
        } else {
            None
        }
    }

    fn get_regime_score(&self, _candles: &[Candle]) -> f64 {
        1.0 // No regime adjustment for simple VWAP strategy
    }

    fn notify_order(&mut self, order: &Order) {
        if order.status == OrderStatus::Completed {
            self.bars_in_position = 0;
            self.cooldown_counter = 0;
        }
    }

    fn notify_trade(&mut self, trade: &Trade) {
        self.cooldown_counter = self.config.cooldown_bars;
        self.bars_in_position = 0;

        let return_pct = trade.return_pct();
        tracing::info!(
            symbol = %trade.symbol,
            return_pct = format!("{:.2}%", return_pct),
            net_pnl = format!("{:.2}", trade.net_pnl),
            "VWAP Scalper trade closed"
        );
    }

    fn init(&mut self) {
        self.bars_in_position = 0;
        self.cooldown_counter = 0;
        tracing::info!("VWAP Scalper strategy initialized");
    }
}
