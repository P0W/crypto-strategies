//! Micro Scalper Strategy Implementation
//!
//! Optimized for 5-minute charts with high trade frequency.
//!
//! ## Entry Logic (Trend Following)
//! - Long: Fast EMA > Slow EMA AND Close > Fast EMA (pullback to EMA)
//! - Short: Fast EMA < Slow EMA AND Close < Fast EMA
//!
//! ## Exit Logic
//! - Take profit at target ATR
//! - Stop loss at stop ATR
//! - Trailing stop after activation
//! - Exit on trend reversal

use crate::indicators::{atr, ema};
use crate::strategies::Strategy;
use crate::{Candle, Order, OrderStatus, Position, Signal, Symbol, Trade};

use super::config::MicroScalperConfig;

pub struct MicroScalperStrategy {
    config: MicroScalperConfig,
    cooldown_counter: usize,
    bars_in_position: usize,
}

impl MicroScalperStrategy {
    pub fn new(config: MicroScalperConfig) -> Self {
        Self {
            config,
            cooldown_counter: 0,
            bars_in_position: 0,
        }
    }

    /// Check trend direction and strength
    fn get_trend_signal(&self, candles: &[Candle]) -> Option<(Signal, f64, f64)> {
        if candles.len() < self.config.ema_slow + 2 {
            return None;
        }

        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let fast_ema = ema(&close, self.config.ema_fast);
        let slow_ema = ema(&close, self.config.ema_slow);

        let fast_curr = fast_ema.last().and_then(|&x| x)?;
        let slow_curr = slow_ema.last().and_then(|&x| x)?;

        let current_close = candles.last()?.close;

        // Strong uptrend: fast > slow AND price above fast EMA
        if fast_curr > slow_curr && current_close > fast_curr * 0.998 {
            Some((Signal::Long, fast_curr, slow_curr))
        } else if fast_curr < slow_curr && current_close < fast_curr * 1.002 {
            Some((Signal::Short, fast_curr, slow_curr))
        } else {
            Some((Signal::Flat, fast_curr, slow_curr))
        }
    }

    /// Check for momentum confirmation using recent candles
    fn has_momentum(&self, candles: &[Candle], is_long: bool) -> bool {
        if candles.len() < 5 {
            return false;
        }

        let recent = &candles[candles.len() - 5..];

        // Count bullish/bearish candles
        let bullish_count = recent.iter().filter(|c| c.close > c.open).count();
        let bearish_count = recent.iter().filter(|c| c.close < c.open).count();

        if is_long {
            bullish_count >= 3 // At least 3 of 5 candles bullish
        } else {
            bearish_count >= 3
        }
    }

    /// Get current ATR
    fn get_atr(&self, candles: &[Candle]) -> f64 {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_vals = atr(&high, &low, &close, self.config.atr_period);
        atr_vals
            .last()
            .and_then(|&x| x)
            .unwrap_or_else(|| candles.last().map(|c| c.close * 0.01).unwrap_or(0.01))
    }
}

impl Strategy for MicroScalperStrategy {
    fn name(&self) -> &'static str {
        "micro_scalper"
    }

    fn generate_signal(
        &self,
        _symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal {
        let min_bars = self.config.ema_slow.max(self.config.atr_period) + 10;

        if candles.len() < min_bars {
            return Signal::Flat;
        }

        // If in position
        if let Some(pos) = position {
            // Check max hold
            if self.bars_in_position >= self.config.max_hold_bars {
                return Signal::Flat;
            }

            // Exit on trend reversal
            if let Some((signal, _fast, _slow)) = self.get_trend_signal(candles) {
                if signal != Signal::Long {
                    return Signal::Flat; // Trend reversed, exit
                }
            }

            // Hold if profitable and momentum continues
            let current_price = candles.last().unwrap().close;
            let pnl_pct = (current_price - pos.entry_price) / pos.entry_price;

            if pnl_pct > 0.02 && !self.has_momentum(candles, true) {
                return Signal::Flat; // Take profit when momentum fades
            }

            return Signal::Long; // Hold position
        }

        // Cooldown check
        if self.cooldown_counter > 0 {
            return Signal::Flat;
        }

        // Get trend signal
        let (signal, _fast, _slow) = match self.get_trend_signal(candles) {
            Some(s) => s,
            None => return Signal::Flat,
        };

        // Only enter in strong trend with momentum
        if signal == Signal::Long && self.has_momentum(candles, true) {
            return Signal::Long;
        }

        // Short: only if allowed AND bearish trend
        if self.config.allow_short && signal == Signal::Short && self.has_momentum(candles, false) {
            return Signal::Short;
        }

        Signal::Flat
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let current_atr = self.get_atr(candles);
        entry_price - self.config.stop_atr * current_atr
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let current_atr = self.get_atr(candles);
        entry_price + self.config.target_atr * current_atr
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        let current_atr = self.get_atr(candles);
        let profit_atr = (current_price - position.entry_price) / current_atr;
        let current_stop = position.trailing_stop.unwrap_or(position.stop_price);

        if profit_atr >= self.config.trailing_activation {
            let new_stop = current_price - self.config.trailing_atr * current_atr;
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

    fn notify_order(&mut self, order: &Order) {
        if order.status == OrderStatus::Completed {
            self.bars_in_position = 0;
            self.cooldown_counter = 0;
        }
    }

    fn notify_trade(&mut self, trade: &Trade) {
        self.cooldown_counter = self.config.cooldown;
        self.bars_in_position = 0;
        tracing::debug!(
            symbol = %trade.symbol,
            pnl = format!("{:.2}", trade.net_pnl),
            "Micro scalper trade"
        );
    }

    fn init(&mut self) {
        self.cooldown_counter = 0;
        self.bars_in_position = 0;
    }
}
