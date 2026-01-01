//! Quick Flip (Pattern Scalp) Strategy
//!
//! Original Quick Flip strategy adapted for crypto markets.
//! 
//! Core Logic (as per original strategy):
//! 1. Use daily-like ATR (96 bars on 15m = ~24 hours) for volatility measurement
//! 2. Establish "box" from first bar(s) of each session (default: midnight UTC)
//! 3. Range must be >= 25% of ATR to be valid
//! 4. Wait for price to break outside the box
//! 5. Enter on pattern confirmation (Hammer/Inverted Hammer or Engulfing)
//! 6. Valid only within 90 minutes (6 bars on 15m) after session start
//! 7. Stop at signal candle extreme, target at opposite side of box

use crate::indicators::atr;
use crate::strategies::Strategy;
use crate::{Candle, Position, Signal, Symbol};
use chrono::Timelike;
use std::sync::RwLock;

use super::config::QuickFlipConfig;

#[derive(Debug, Clone)]
struct SessionState {
    current_date: Option<chrono::NaiveDate>,
    range_high: Option<f64>,
    range_low: Option<f64>,
    range_established_bar: usize,
    last_trade_bar: usize,
    last_signal: Signal,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            current_date: None,
            range_high: None,
            range_low: None,
            range_established_bar: 0,
            last_trade_bar: 0,
            last_signal: Signal::Flat,
        }
    }
}

pub struct QuickFlipStrategy {
    config: QuickFlipConfig,
    state: RwLock<SessionState>,
}

impl QuickFlipStrategy {
    pub fn new(config: QuickFlipConfig) -> Self {
        Self {
            config,
            state: RwLock::new(SessionState::default()),
        }
    }

    /// Check if we're at the start of a new session
    fn is_session_start(&self, candle: &Candle) -> bool {
        let hour = candle.datetime.hour() as usize;
        let minute = candle.datetime.minute() as usize;
        
        // Check if this is the session start hour and within the first opening_bars period
        // For 5m chart with opening_bars=3, this means first 15 minutes (00:00, 00:05, 00:10)
        hour == self.config.session_start_hour && minute < (self.config.opening_bars * 5)
    }

    /// Update the opening range for the session
    fn update_session_range(&self, candles: &[Candle], current_bar: usize) {
        if candles.is_empty() {
            return;
        }

        let current = candles.last().unwrap();
        let current_date = current.datetime.date_naive();
        
        let mut state = self.state.write().unwrap();

        // Reset for new day
        if state.current_date != Some(current_date) {
            state.current_date = Some(current_date);
            state.range_high = None;
            state.range_low = None;
            state.range_established_bar = 0;
        }

        // Establish range from opening bar(s) if at session start
        if state.range_high.is_none() && self.is_session_start(current) {
            // Collect the first N bars of the session for the range
            let session_start_idx = candles.len().saturating_sub(self.config.opening_bars);
            let opening_candles = &candles[session_start_idx..];
            
            if !opening_candles.is_empty() {
                state.range_high = opening_candles.iter().map(|c| c.high).fold(None, |max, h| {
                    Some(max.map_or(h, |m: f64| m.max(h)))
                });
                state.range_low = opening_candles.iter().map(|c| c.low).fold(None, |min, l| {
                    Some(min.map_or(l, |m: f64| m.min(l)))
                });
                state.range_established_bar = current_bar;
            }
        }
    }

    /// Check if we're within the validity window
    fn is_within_validity_window(&self, current_bar: usize) -> bool {
        let state = self.state.read().unwrap();
        if state.range_established_bar == 0 {
            return false;
        }
        
        let bars_since_establishment = current_bar.saturating_sub(state.range_established_bar);
        bars_since_establishment <= self.config.validity_window_bars
    }

    /// Get current ATR value
    fn get_current_atr(&self, candles: &[Candle]) -> Option<f64> {
        if candles.len() < self.config.atr_period + 1 {
            return None;
        }

        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_vals = atr(&high, &low, &close, self.config.atr_period);
        atr_vals.last().and_then(|&x| x)
    }

    /// Check if the range passes ATR filter
    fn passes_atr_filter(&self, atr_val: f64) -> bool {
        let state = self.state.read().unwrap();
        if let (Some(high), Some(low)) = (state.range_high, state.range_low) {
            let range_size = high - low;
            let min_required = atr_val * self.config.min_range_pct;
            range_size >= min_required
        } else {
            false
        }
    }

    /// Check if candle is a hammer pattern
    /// Hammer: small body at top, long lower wick (bullish reversal)
    fn is_hammer(&self, candle: &Candle) -> bool {
        let body = (candle.close - candle.open).abs();
        let total_range = candle.high - candle.low;
        let lower_wick = candle.open.min(candle.close) - candle.low;
        let upper_wick = candle.high - candle.open.max(candle.close);

        if total_range <= 0.0 {
            return false;
        }

        // Body must be small (<= 35% of range)
        let body_ratio = body / total_range;
        // Lower wick must be long (>= 55% of range)
        let lower_wick_ratio = lower_wick / total_range;
        // Upper wick must be small (<= 20% of range)
        let upper_wick_ratio = upper_wick / total_range;
        // Candle must close in upper half of body or be neutral (bullish tendency)
        let closes_not_too_low = candle.close >= candle.open * 0.998;

        body_ratio <= 0.35
            && lower_wick_ratio >= 0.55
            && upper_wick_ratio <= 0.20
            && closes_not_too_low
    }

    /// Check if candle is an inverted hammer pattern
    /// Inverted Hammer: small body at bottom, long upper wick (bearish reversal)
    fn is_inverted_hammer(&self, candle: &Candle) -> bool {
        let body = (candle.close - candle.open).abs();
        let total_range = candle.high - candle.low;
        let lower_wick = candle.open.min(candle.close) - candle.low;
        let upper_wick = candle.high - candle.open.max(candle.close);

        if total_range == 0.0 {
            return false;
        }

        let body_ratio = body / total_range;
        let upper_wick_ratio = upper_wick / total_range;
        let lower_wick_ratio = lower_wick / total_range;

        body_ratio < 0.30 && upper_wick_ratio > 0.60 && lower_wick_ratio < 0.20
    }

    /// Check if current candle is a bullish engulfing pattern
    fn is_bullish_engulfing(&self, prev: &Candle, current: &Candle) -> bool {
        // Previous candle must be bearish (red)
        let prev_bearish = prev.close < prev.open;
        let prev_body = (prev.open - prev.close).abs();
        let prev_range = prev.high - prev.low;
        
        // Current candle must be bullish (green)
        let current_bullish = current.close > current.open;
        let current_body = (current.close - current.open).abs();
        let current_range = current.high - current.low;
        
        // Both candles must have reasonable bodies (not dojis)
        let prev_has_body = prev_range > 0.0 && prev_body / prev_range > 0.3;
        let current_has_body = current_range > 0.0 && current_body / current_range > 0.3;
        
        // Current candle body must fully engulf previous candle body
        let engulfs = current.open <= prev.close && current.close >= prev.open;
        
        // Current body should be at least as large as previous body
        let current_adequate = current_body >= prev_body * 0.9;

        prev_bearish && current_bullish && prev_has_body && current_has_body && engulfs && current_adequate
    }

    /// Check if current candle is a bearish engulfing pattern
    fn is_bearish_engulfing(&self, prev: &Candle, current: &Candle) -> bool {
        // Previous candle is bullish (green)
        let prev_bullish = prev.close > prev.open;
        // Current candle is bearish (red)
        let current_bearish = current.close < current.open;
        // Current candle body engulfs previous candle body
        let engulfs = current.open >= prev.close && current.close <= prev.open;

        prev_bullish && current_bearish && engulfs
    }
}

impl Strategy for QuickFlipStrategy {
    fn name(&self) -> &'static str {
        "quick_flip"
    }

    fn generate_signal(
        &self,
        _symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal {
        // Need enough data for ATR calculation
        if candles.len() < self.config.atr_period + 10 {
            return Signal::Flat;
        }

        let current_bar = candles.len();

        // Update session range
        self.update_session_range(candles, current_bar);

        // If already in position, hold
        if position.is_some() {
            return Signal::Long;
        }

        // Cooldown: prevent too frequent trading
        {
            let state = self.state.read().unwrap();
            if current_bar - state.last_trade_bar < self.config.cooldown_bars {
                return Signal::Flat;
            }
        }

        // Check if range is established
        let (range_high, range_low) = {
            let state = self.state.read().unwrap();
            match (state.range_high, state.range_low) {
                (Some(h), Some(l)) => (h, l),
                _ => return Signal::Flat,
            }
        };

        // Check if we're within validity window
        if !self.is_within_validity_window(current_bar) {
            return Signal::Flat;
        }

        // Get ATR and check filter
        let atr_val = match self.get_current_atr(candles) {
            Some(a) => a,
            None => return Signal::Flat,
        };

        if !self.passes_atr_filter(atr_val) {
            return Signal::Flat;
        }

        // Need at least 2 candles for pattern detection
        if candles.len() < 2 {
            return Signal::Flat;
        }

        let current = candles.last().unwrap();
        let prev = &candles[candles.len() - 2];

        // BULLISH ENTRY: Price below range low (outside the box)
        if current.close < range_low {
            // Signal 1: Hammer pattern
            if self.is_hammer(current) {
                self.state.write().unwrap().last_signal = Signal::Long;
                return Signal::Long;
            }
            // Signal 2: Bullish engulfing
            if self.is_bullish_engulfing(prev, current) {
                self.state.write().unwrap().last_signal = Signal::Long;
                return Signal::Long;
            }
        }

        // BEARISH ENTRY: Price above range high (outside the box)
        if current.close > range_high {
            // Signal 1: Inverted Hammer pattern
            if self.is_inverted_hammer(current) {
                self.state.write().unwrap().last_signal = Signal::Short;
                return Signal::Short;
            }
            // Signal 2: Bearish engulfing
            if self.is_bearish_engulfing(prev, current) {
                self.state.write().unwrap().last_signal = Signal::Short;
                return Signal::Short;
            }
        }

        Signal::Flat
    }

    fn calculate_stop_loss(&self, candles: &[Candle], _entry_price: f64) -> f64 {
        // Stop at the low of the signal candle for Long, high for Short
        let signal_candle = candles.last().unwrap();
        let last_signal = self.state.read().unwrap().last_signal;
        
        match last_signal {
            Signal::Long => signal_candle.low,
            Signal::Short => signal_candle.high,
            Signal::Flat => signal_candle.low, // Default to long logic
        }
    }

    fn calculate_take_profit(&self, _candles: &[Candle], _entry_price: f64) -> f64 {
        // Target is the opposite side of the range (or 50% mid-point if conservative)
        let state = self.state.read().unwrap();
        let last_signal = state.last_signal;
        
        if let (Some(range_high), Some(range_low)) = (state.range_high, state.range_low) {
            match last_signal {
                Signal::Long => {
                    if self.config.conservative_target {
                        // Conservative: mid-point (50%) of range
                        (range_high + range_low) / 2.0
                    } else {
                        // Primary: opposite side of range (range high for long entries)
                        range_high
                    }
                }
                Signal::Short => {
                    if self.config.conservative_target {
                        // Conservative: mid-point (50%) of range
                        (range_high + range_low) / 2.0
                    } else {
                        // Primary: opposite side of range (range low for short entries)
                        range_low
                    }
                }
                Signal::Flat => {
                    // Default to long logic
                    if self.config.conservative_target {
                        (range_high + range_low) / 2.0
                    } else {
                        range_high
                    }
                }
            }
        } else {
            // Fallback: 2% profit target
            _entry_price * 1.02
        }
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        _candles: &[Candle],
    ) -> Option<f64> {
        // Move stop to break-even once price moves back inside the box or reaches 50% target
        let state = self.state.read().unwrap();
        if let (Some(range_high), Some(range_low)) = (state.range_high, state.range_low) {
            let mid_point = (range_high + range_low) / 2.0;
            
            match position.side {
                crate::Side::Buy => {
                    // For Long: If we've reached the mid-point or moved back inside the range, move to break-even
                    if current_price >= mid_point || (current_price >= range_low && current_price <= range_high) {
                        // Move to break-even (entry price)
                        return Some(position.entry_price);
                    }
                }
                crate::Side::Sell => {
                    // For Short: If we've reached the mid-point or moved back inside the range, move to break-even
                    if current_price <= mid_point || (current_price >= range_low && current_price <= range_high) {
                        // Move to break-even (entry price)
                        return Some(position.entry_price);
                    }
                }
            }
        }
        
        None
    }

    fn notify_trade(&mut self, trade: &crate::Trade) {
        // Update last trade bar to prevent immediate re-entry
        if let Ok(mut state) = self.state.write() {
            // We don't have access to current bar index here, so we'll use a simple counter
            state.last_trade_bar += self.config.cooldown_bars;
        }
        tracing::debug!(
            symbol = %trade.symbol,
            pnl = format!("{:.2}", trade.net_pnl),
            "Quick flip trade"
        );
    }

    fn init(&mut self) {
        *self.state.write().unwrap() = SessionState::default();
    }
}
