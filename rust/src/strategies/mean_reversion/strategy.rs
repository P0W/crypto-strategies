//! Mean Reversion Scalper Strategy
//!
//! A professional-grade mean reversion strategy for short timeframe trading.
//!
//! ## Strategy Logic
//!
//! ### Entry Conditions (Long)
//! 1. Price at or below lower Bollinger Band
//! 2. RSI below oversold threshold (default: 30)
//! 3. Volume spike detected (optional but recommended)
//! 4. Price above trend EMA (optional trend filter)
//!
//! ### Exit Conditions
//! 1. Take profit at middle Bollinger Band (mean reversion target)
//! 2. Stop loss at entry - (stop_atr_multiple × ATR)
//! 3. Trailing stop after profit threshold reached
//! 4. Exit if RSI reaches overbought (momentum exhaustion)
//!
//! ## Why This Works
//! - Crypto markets exhibit strong mean reversion on short timeframes
//! - Bollinger Band extremes indicate overextension
//! - RSI confirms momentum exhaustion
//! - Volume spikes indicate institutional interest (not just retail noise)

use crate::indicators::{atr, bollinger_bands, ema, rsi, sma};
use crate::strategies::Strategy;
use crate::{Candle, Position, Side, Signal, Symbol};

use super::config::MeanReversionConfig;
use super::{MarketState, VolumeState};

/// Mean Reversion Scalper Strategy
pub struct MeanReversionStrategy {
    config: MeanReversionConfig,
    /// Track consecutive losses for cooldown
    consecutive_losses: usize,
    /// Cooldown counter (bars since last loss streak)
    cooldown_counter: usize,
}

impl MeanReversionStrategy {
    pub fn new(config: MeanReversionConfig) -> Self {
        MeanReversionStrategy {
            config,
            consecutive_losses: 0,
            cooldown_counter: 0,
        }
    }

    /// Classify market state based on Bollinger Bands and RSI
    fn classify_market_state(&self, candles: &[Candle]) -> Option<MarketState> {
        if candles.len() < self.config.bb_period {
            return None;
        }

        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let (upper, middle, lower) =
            bollinger_bands(&close, self.config.bb_period, self.config.bb_std);
        let rsi_values = rsi(&close, self.config.rsi_period);

        // Get current values
        let current_close = candles.last()?.close;
        let upper_band = upper.last().and_then(|&x| x)?;
        let middle_band = middle.last().and_then(|&x| x)?;
        let lower_band = lower.last().and_then(|&x| x)?;
        let current_rsi = rsi_values.last().and_then(|&x| x)?;

        // Calculate band width for penetration check
        let band_width = upper_band - lower_band;
        let penetration_distance = band_width * self.config.bb_penetration;

        // Check for extreme conditions (>3 std deviations)
        let extreme_upper = middle_band + 3.0 * (upper_band - middle_band) / self.config.bb_std;
        let extreme_lower = middle_band - 3.0 * (middle_band - lower_band) / self.config.bb_std;

        if current_close >= extreme_upper || current_close <= extreme_lower {
            return Some(MarketState::Extreme);
        }

        // Check oversold conditions
        if current_close <= lower_band + penetration_distance
            && current_rsi <= self.config.rsi_oversold
        {
            return Some(MarketState::Oversold);
        }

        // Check overbought conditions
        if current_close >= upper_band - penetration_distance
            && current_rsi >= self.config.rsi_overbought
        {
            return Some(MarketState::Overbought);
        }

        Some(MarketState::Neutral)
    }

    /// Check volume condition
    fn check_volume_state(&self, candles: &[Candle]) -> VolumeState {
        if candles.len() < self.config.volume_period + 1 {
            return VolumeState::Normal;
        }

        let volumes: Vec<f64> = candles.iter().map(|c| c.volume).collect();
        let volume_ma = sma(&volumes, self.config.volume_period);

        let current_volume = candles.last().map(|c| c.volume).unwrap_or(0.0);
        let avg_volume = volume_ma.last().and_then(|&x| x).unwrap_or(1.0);

        if avg_volume <= 0.0 {
            return VolumeState::Normal;
        }

        let volume_ratio = current_volume / avg_volume;

        if volume_ratio >= self.config.volume_spike_threshold {
            VolumeState::Spike
        } else if volume_ratio < 0.5 {
            VolumeState::Low
        } else {
            VolumeState::Normal
        }
    }

    /// Check trend filter condition
    fn is_trend_favorable(&self, candles: &[Candle], for_long: bool) -> bool {
        if !self.config.use_trend_filter {
            return true; // Filter disabled
        }

        if candles.len() < self.config.trend_ema_period {
            return false;
        }

        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let trend_ema = ema(&close, self.config.trend_ema_period);

        let current_close = candles.last().map(|c| c.close).unwrap_or(0.0);
        let ema_value = trend_ema.last().and_then(|&x| x).unwrap_or(0.0);

        if for_long {
            // For long: price should be above trend EMA (uptrend)
            // Relaxed condition: within 1% of EMA is acceptable
            current_close >= ema_value * 0.99
        } else {
            // For short: price should be below trend EMA (downtrend)
            current_close <= ema_value * 1.01
        }
    }

    /// Check if in cooldown period
    fn is_in_cooldown(&self) -> bool {
        self.consecutive_losses >= self.config.max_consecutive_losses
            && self.cooldown_counter < self.config.cooldown_bars
    }

    /// Get current Bollinger Band middle for take profit calculation
    fn get_bb_middle(&self, candles: &[Candle]) -> Option<f64> {
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let (_, middle, _) = bollinger_bands(&close, self.config.bb_period, self.config.bb_std);
        middle.last().and_then(|&x| x)
    }

    /// Check RSI-based exit condition
    fn should_exit_on_rsi(&self, candles: &[Candle], is_long: bool) -> bool {
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let rsi_values = rsi(&close, self.config.rsi_period);

        if let Some(current_rsi) = rsi_values.last().and_then(|&x| x) {
            if is_long && current_rsi >= self.config.rsi_overbought {
                return true; // Momentum exhaustion for long
            }
            if !is_long && current_rsi <= self.config.rsi_oversold {
                return true; // Momentum exhaustion for short
            }
        }
        false
    }
}

impl Strategy for MeanReversionStrategy {
    fn name(&self) -> &'static str {
        "mean_reversion"
    }

    fn generate_signal(
        &self,
        _symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal {
        // Calculate minimum warmup period
        let min_warmup = self
            .config
            .bb_period
            .max(self.config.rsi_period)
            .max(self.config.volume_period)
            .max(self.config.trend_ema_period)
            .max(self.config.atr_period);

        // Don't generate signals if insufficient data
        if candles.len() < min_warmup + 5 {
            return Signal::Flat;
        }

        // Check cooldown
        if self.is_in_cooldown() && position.is_none() {
            return Signal::Flat;
        }

        // If we have a position, check exit conditions
        if let Some(pos) = position {
            let current_price = candles.last().unwrap().close;

            // Exit on RSI extreme (momentum exhaustion)
            let is_long = pos.side == Side::Buy;
            if self.should_exit_on_rsi(candles, is_long) {
                return Signal::Flat;
            }

            // Exit if price reaches middle band (take profit - handled by backtester but
            // we can also signal exit if using different TP mode)
            if self.config.take_profit_mode == "bb_middle" {
                if let Some(bb_middle) = self.get_bb_middle(candles) {
                    // For Long: If price crosses above middle band, consider exiting
                    // For Short: If price crosses below middle band, consider exiting
                    let should_exit = match pos.side {
                        Side::Buy => {
                            current_price >= bb_middle && pos.unrealized_pnl(current_price) > 0.0
                        }
                        Side::Sell => {
                            current_price <= bb_middle && pos.unrealized_pnl(current_price) > 0.0
                        }
                    };
                    if should_exit {
                        return Signal::Flat;
                    }
                }
            }

            // Hold position - return appropriate signal
            return match pos.side {
                Side::Buy => Signal::Long,
                Side::Sell => Signal::Short,
            };
        }

        // Entry logic - classify market state
        let market_state = match self.classify_market_state(candles) {
            Some(state) => state,
            None => return Signal::Flat,
        };

        // Don't trade in extreme conditions
        if market_state == MarketState::Extreme {
            return Signal::Flat;
        }

        // Check volume condition
        let volume_state = self.check_volume_state(candles);
        if self.config.require_volume_spike && volume_state != VolumeState::Spike {
            return Signal::Flat;
        }

        // Avoid low volume periods
        if volume_state == VolumeState::Low {
            return Signal::Flat;
        }

        // Long entry conditions
        if market_state == MarketState::Oversold && self.is_trend_favorable(candles, true) {
            return Signal::Long;
        }

        // Short entry conditions (if enabled)
        if self.config.allow_short
            && market_state == MarketState::Overbought
            && self.is_trend_favorable(candles, false)
        {
            return Signal::Short;
        }

        Signal::Flat
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_values
            .last()
            .and_then(|&x| x)
            .unwrap_or(entry_price * 0.02); // Fallback: 2% of price

        entry_price - self.config.stop_atr_multiple * current_atr
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        // Mean reversion target: Bollinger Band middle
        if self.config.take_profit_mode == "bb_middle" {
            if let Some(bb_middle) = self.get_bb_middle(candles) {
                return bb_middle;
            }
        }

        // Fallback: ATR-based target
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_values = atr(&high, &low, &close, self.config.atr_period);
        let current_atr = atr_values
            .last()
            .and_then(|&x| x)
            .unwrap_or(entry_price * 0.02);

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
            .unwrap_or(current_price * 0.02);

        // Calculate profit in ATR terms
        let profit_atr = if current_atr > 0.0 {
            (current_price - position.entry_price) / current_atr
        } else {
            0.0
        };

        // Get current stop level
        let current_stop = position.trailing_stop.unwrap_or(position.stop_price);

        // Check if profit threshold is met for trailing activation
        if profit_atr >= self.config.trailing_activation {
            let new_stop = current_price - self.config.trailing_atr_multiple * current_atr;

            // Only update if new stop is higher (ratchet up only)
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

    /// Get regime score for position sizing
    /// Mean reversion works best in normal/compression regimes
    fn get_regime_score(&self, candles: &[Candle]) -> f64 {
        match self.classify_market_state(candles) {
            Some(MarketState::Oversold) => 1.2, // Good setup, slightly larger
            Some(MarketState::Overbought) => 1.2, // Good setup for shorts
            Some(MarketState::Neutral) => 0.8,  // Not in setup zone
            Some(MarketState::Extreme) => 0.3,  // Dangerous, minimal size
            None => 1.0,
        }
    }

    fn notify_trade(&mut self, trade: &crate::Trade) {
        // Track consecutive losses for cooldown logic
        if trade.net_pnl < 0.0 {
            self.consecutive_losses += 1;
            if self.consecutive_losses >= self.config.max_consecutive_losses {
                self.cooldown_counter = 0; // Reset cooldown counter
            }
        } else {
            self.consecutive_losses = 0; // Reset on win
        }

        // Log trade
        let return_pct = trade.return_pct();
        let net_pnl_post_tax = if trade.net_pnl > 0.0 {
            trade.net_pnl * 0.7 // 30% tax on profits
        } else {
            trade.net_pnl
        };

        tracing::info!(
            symbol = %trade.symbol,
            entry_price = trade.entry_price,
            exit_price = trade.exit_price,
            quantity = trade.quantity,
            return_pct = format!("{:.2}%", return_pct),
            gross_pnl = trade.pnl,
            commission = trade.commission,
            net_pnl = trade.net_pnl,
            net_pnl_post_tax = format!("{:.2}", net_pnl_post_tax),
            consecutive_losses = self.consecutive_losses,
            "Mean Reversion trade closed"
        );
    }

    fn init(&mut self) {
        self.consecutive_losses = 0;
        self.cooldown_counter = 0;
        tracing::info!("Mean Reversion Scalper strategy initialized");
    }
}
