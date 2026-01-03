//! Momentum Scalper Strategy Implementation
//!
//! ## Entry Logic
//! 1. EMA fast crosses above EMA slow (bullish crossover)
//! 2. Price above trend EMA (trading with trend)
//! 3. MACD histogram positive (momentum confirmation)
//! 4. Optional: Volume above threshold
//!
//! ## Exit Logic
//! 1. Take profit at target ATR multiple
//! 2. Stop loss at entry - stop ATR multiple
//! 3. Trailing stop after activation threshold
//! 4. Exit on EMA cross back (fast below slow)
//! 5. Max hold bars exceeded

use crate::indicators::{adx, atr, ema, macd};
use crate::strategies::Strategy;
use crate::{Candle, Order, OrderStatus, Position, Signal, Symbol, Trade};

use super::config::MomentumScalperConfig;
use super::MomentumState;

/// Momentum Scalper Strategy
pub struct MomentumScalperStrategy {
    config: MomentumScalperConfig,
    bars_in_position: usize,
    cooldown_counter: usize,
    last_signal: Signal,
}

impl MomentumScalperStrategy {
    pub fn new(config: MomentumScalperConfig) -> Self {
        MomentumScalperStrategy {
            config,
            bars_in_position: 0,
            cooldown_counter: 0,
            last_signal: Signal::Flat,
        }
    }

    /// Check if EMAs are still in bullish/bearish alignment
    fn check_ema_alignment(&self, candles: &[Candle]) -> Option<Signal> {
        if candles.len() < self.config.ema_slow {
            return None;
        }

        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let fast_ema = ema(&close, self.config.ema_fast);
        let slow_ema = ema(&close, self.config.ema_slow);

        let fast_curr = fast_ema.last().and_then(|&x| x)?;
        let slow_curr = slow_ema.last().and_then(|&x| x)?;

        if fast_curr > slow_curr {
            Some(Signal::Long)
        } else if fast_curr < slow_curr {
            Some(Signal::Short)
        } else {
            Some(Signal::Flat)
        }
    }

    /// Get MACD momentum state
    fn get_macd_momentum(&self, candles: &[Candle]) -> MomentumState {
        if !self.config.use_macd || candles.len() < self.config.macd_slow + self.config.macd_signal
        {
            return MomentumState::Neutral;
        }

        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let (macd_line, signal_line, histogram) = macd(
            &close,
            self.config.macd_fast,
            self.config.macd_slow,
            self.config.macd_signal,
        );

        let hist_curr = histogram.last().and_then(|&x| x).unwrap_or(0.0);
        let hist_prev = if histogram.len() >= 2 {
            histogram[histogram.len() - 2].unwrap_or(0.0)
        } else {
            0.0
        };

        let macd_curr = macd_line.last().and_then(|&x| x).unwrap_or(0.0);
        let signal_curr = signal_line.last().and_then(|&x| x).unwrap_or(0.0);

        // Strong momentum: histogram positive and increasing, MACD above signal
        if hist_curr > 0.0 && hist_curr > hist_prev && macd_curr > signal_curr {
            MomentumState::StrongBullish
        } else if hist_curr > 0.0 {
            MomentumState::WeakBullish
        } else if hist_curr < 0.0 && hist_curr < hist_prev && macd_curr < signal_curr {
            MomentumState::StrongBearish
        } else if hist_curr < 0.0 {
            MomentumState::WeakBearish
        } else {
            MomentumState::Neutral
        }
    }

    /// Check ADX trend strength
    fn check_adx_strength(&self, candles: &[Candle]) -> bool {
        if candles.len() < self.config.adx_period * 2 {
            return true; // Not enough data, allow trade
        }

        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let adx_values = adx(&high, &low, &close, self.config.adx_period);
        let current_adx = adx_values.last().and_then(|&x| x).unwrap_or(0.0);

        current_adx >= self.config.adx_threshold
    }

    /// Check if should exit on EMA cross back
    fn should_exit_on_cross(&self, candles: &[Candle], is_long: bool) -> bool {
        if !self.config.exit_on_cross {
            return false;
        }

        if let Some(alignment) = self.check_ema_alignment(candles) {
            if is_long && alignment == Signal::Short {
                return true;
            }
            if !is_long && alignment == Signal::Long {
                return true;
            }
        }
        false
    }
}

impl Strategy for MomentumScalperStrategy {
    fn name(&self) -> &'static str {
        "momentum_scalper"
    }

    fn generate_signal(
        &self,
        _symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal {
        // Minimum data requirement
        let min_bars = self
            .config
            .ema_slow
            .max(self.config.ema_trend)
            .max(self.config.macd_slow + self.config.macd_signal)
            .max(self.config.adx_period * 2);

        if candles.len() < min_bars + 5 {
            return Signal::Flat;
        }

        // Check cooldown
        if self.cooldown_counter > 0 && position.is_none() {
            return Signal::Flat;
        }

        // If in position, check exit conditions
        if let Some(_pos) = position {
            // Exit on EMA cross back
            if self.should_exit_on_cross(candles, true) {
                return Signal::Flat;
            }

            // Exit on max hold bars
            if self.bars_in_position >= self.config.max_hold_bars {
                return Signal::Flat;
            }

            // Check momentum exhaustion
            let momentum = self.get_macd_momentum(candles);
            if matches!(
                momentum,
                MomentumState::WeakBearish | MomentumState::StrongBearish
            ) {
                return Signal::Flat;
            }

            // Hold position
            return Signal::Long;
        }

        // Simple entry logic - EMA alignment is enough
        // We want to be in position when fast EMA > slow EMA (uptrend)
        let alignment = self.check_ema_alignment(candles);

        let signal = match alignment {
            Some(s) => s,
            None => return Signal::Flat,
        };

        // Only apply ADX filter if explicitly configured
        if self.config.adx_threshold > 0.0 && !self.check_adx_strength(candles) {
            return Signal::Flat;
        }

        // Only apply MACD filter if explicitly enabled
        if self.config.use_macd {
            let momentum = self.get_macd_momentum(candles);
            if signal == Signal::Long
                && !matches!(
                    momentum,
                    MomentumState::StrongBullish
                        | MomentumState::WeakBullish
                        | MomentumState::Neutral
                )
            {
                return Signal::Flat;
            }
        }

        // Return signal (only long if short not allowed)
        if signal == Signal::Short && !self.config.allow_short {
            return Signal::Flat;
        }

        signal
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_values
            .last()
            .and_then(|&x| x)
            .unwrap_or(entry_price * 0.01);

        entry_price - self.config.stop_atr_multiple * current_atr
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_values
            .last()
            .and_then(|&x| x)
            .unwrap_or(entry_price * 0.01);

        entry_price + self.config.target_atr_multiple * current_atr
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_values
            .last()
            .and_then(|&x| x)
            .unwrap_or(current_price * 0.01);

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

    fn get_regime_score(&self, candles: &[Candle]) -> f64 {
        let momentum = self.get_macd_momentum(candles);
        match momentum {
            MomentumState::StrongBullish => 1.3,
            MomentumState::WeakBullish => 1.1,
            MomentumState::Neutral => 0.8,
            MomentumState::WeakBearish => 0.6,
            MomentumState::StrongBearish => 0.5,
        }
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
            "Momentum Scalper trade closed"
        );
    }

    fn init(&mut self) {
        self.bars_in_position = 0;
        self.cooldown_counter = 0;
        self.last_signal = Signal::Flat;
        tracing::info!("Momentum Scalper strategy initialized");
    }
}
