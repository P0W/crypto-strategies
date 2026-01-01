//! Quick Flip (Pattern Scalp) Strategy
//!
//! Time-of-day range breakout with pattern confirmation.
//! 
//! Core Logic:
//! 1. Define opening range using first 15-minute candle after NY open (9:30 AM EST)
//! 2. Filter by daily ATR (range must be >= 25% of daily ATR)
//! 3. Wait for price to move outside the range
//! 4. Enter on pattern confirmation (Hammer/Inverted Hammer or Engulfing)
//! 5. Exit at opposite side of range or 50% mid-point

use crate::indicators::atr;
use crate::strategies::Strategy;
use crate::{Candle, Position, Signal, Symbol};
use chrono::Timelike;
use std::sync::RwLock;

use super::config::QuickFlipConfig;

#[derive(Debug, Clone)]
struct SessionState {
    range_high: Option<f64>,
    range_low: Option<f64>,
    range_date: Option<chrono::NaiveDate>,
    daily_atr_value: Option<f64>,
    entry_triggered: bool,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            range_high: None,
            range_low: None,
            range_date: None,
            daily_atr_value: None,
            entry_triggered: false,
        }
    }
}

pub struct QuickFlipStrategy {
    config: QuickFlipConfig,
    // Session state (reset each day) - using RwLock for thread safety
    state: RwLock<SessionState>,
}

impl QuickFlipStrategy {
    pub fn new(config: QuickFlipConfig) -> Self {
        Self {
            config,
            state: RwLock::new(SessionState::default()),
        }
    }

    /// Check if candle is a hammer pattern
    /// Hammer: small body at top, long lower wick (bullish reversal)
    fn is_hammer(&self, candle: &Candle) -> bool {
        let body = (candle.close - candle.open).abs();
        let total_range = candle.high - candle.low;
        let lower_wick = candle.open.min(candle.close) - candle.low;
        let upper_wick = candle.high - candle.open.max(candle.close);

        if total_range == 0.0 {
            return false;
        }

        // Body is small (< 30% of range)
        let body_ratio = body / total_range;
        // Lower wick is long (> 60% of range)
        let lower_wick_ratio = lower_wick / total_range;
        // Upper wick is small (< 20% of range)
        let upper_wick_ratio = upper_wick / total_range;

        body_ratio < 0.30 && lower_wick_ratio > 0.60 && upper_wick_ratio < 0.20
    }

    /// Check if candle is an inverted hammer pattern
    /// Inverted Hammer: small body at bottom, long upper wick (bearish reversal)
    #[allow(dead_code)]
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
        // Previous candle is bearish (red)
        let prev_bearish = prev.close < prev.open;
        // Current candle is bullish (green)
        let current_bullish = current.close > current.open;
        // Current candle body engulfs previous candle body
        let engulfs = current.open <= prev.close && current.close >= prev.open;

        prev_bearish && current_bullish && engulfs
    }

    /// Check if current candle is a bearish engulfing pattern
    #[allow(dead_code)]
    fn is_bearish_engulfing(&self, prev: &Candle, current: &Candle) -> bool {
        // Previous candle is bullish (green)
        let prev_bullish = prev.close > prev.open;
        // Current candle is bearish (red)
        let current_bearish = current.close < current.open;
        // Current candle body engulfs previous candle body
        let engulfs = current.open >= prev.close && current.close <= prev.open;

        prev_bullish && current_bearish && engulfs
    }

    /// Calculate daily ATR from daily candles
    /// Note: In backtesting, we need daily data loaded separately or calculated from intraday
    fn calculate_daily_atr(&self, candles: &[Candle]) -> Option<f64> {
        // For simplicity, we'll calculate ATR from the provided candles
        // In production, you'd load daily data separately
        if candles.len() < self.config.daily_atr_period + 1 {
            return None;
        }

        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let atr_vals = atr(&high, &low, &close, self.config.daily_atr_period);
        atr_vals.last().and_then(|&x| x)
    }

    /// Check if we're within the validity window
    fn is_within_validity_window(&self, candle: &Candle) -> bool {
        let time = candle.datetime.time();
        let minutes_from_midnight = time.hour() as usize * 60 + time.minute() as usize;
        
        // Convert EST to UTC offset (EST = UTC-5, but crypto runs 24/7 so we'll use a simpler approach)
        // For crypto markets, we'll treat this as relative to market open time
        let session_end = self.config.session_start_minutes + self.config.validity_window_minutes;
        
        minutes_from_midnight >= self.config.session_start_minutes
            && minutes_from_midnight <= session_end
    }

    /// Update the opening range if needed
    fn update_range(&self, candles: &[Candle]) {
        if candles.is_empty() {
            return;
        }

        let current = candles.last().unwrap();
        let current_date = current.datetime.date_naive();

        let mut state = self.state.write().unwrap();

        // Reset range for new day
        if state.range_date != Some(current_date) {
            state.range_high = None;
            state.range_low = None;
            state.range_date = Some(current_date);
            state.entry_triggered = false;
            state.daily_atr_value = self.calculate_daily_atr(candles);
        }

        // Set range from first 15-minute candle after session start
        // For this implementation, we'll use the high/low of candles in the first 15 minutes
        if state.range_high.is_none() {
            let time = current.datetime.time();
            let minutes_from_midnight = time.hour() as usize * 60 + time.minute() as usize;
            
            // Check if we're in the first 15 minutes after session start
            if minutes_from_midnight >= self.config.session_start_minutes
                && minutes_from_midnight < self.config.session_start_minutes + 15
            {
                // Find all candles in the opening 15-minute window
                let opening_candles: Vec<&Candle> = candles
                    .iter()
                    .rev()
                    .take_while(|c| {
                        c.datetime.date_naive() == current_date
                            && {
                                let t = c.datetime.time();
                                let m = t.hour() as usize * 60 + t.minute() as usize;
                                m >= self.config.session_start_minutes
                                    && m < self.config.session_start_minutes + 15
                            }
                    })
                    .collect();

                if !opening_candles.is_empty() {
                    state.range_high = opening_candles.iter().map(|c| c.high).fold(None, |max, h| {
                        Some(max.map_or(h, |m: f64| m.max(h)))
                    });
                    state.range_low = opening_candles.iter().map(|c| c.low).fold(None, |min, l| {
                        Some(min.map_or(l, |m: f64| m.min(l)))
                    });
                }
            }
        }
    }

    /// Check if the range passes ATR filter
    fn passes_atr_filter(&self) -> bool {
        let state = self.state.read().unwrap();
        if let (Some(high), Some(low), Some(daily_atr)) =
            (state.range_high, state.range_low, state.daily_atr_value)
        {
            let range_size = high - low;
            let min_required = daily_atr * self.config.min_range_pct;
            range_size >= min_required
        } else {
            false
        }
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
        // Need enough data
        if candles.len() < 50 {
            return Signal::Flat;
        }

        // Update range state
        self.update_range(candles);

        // If already in position, hold
        if position.is_some() {
            return Signal::Long;
        }

        // If entry already triggered today, no new entries
        {
            let state = self.state.read().unwrap();
            if state.entry_triggered {
                return Signal::Flat;
            }
        }

        let current = candles.last().unwrap();
        
        // Check if range is set and passes ATR filter
        let (_range_high, range_low) = {
            let state = self.state.read().unwrap();
            if state.range_high.is_none() || state.range_low.is_none() {
                return Signal::Flat;
            }
            (state.range_high.unwrap(), state.range_low.unwrap())
        };

        if !self.passes_atr_filter() {
            return Signal::Flat;
        }

        // Check validity window
        if !self.is_within_validity_window(current) {
            return Signal::Flat;
        }

        // Need at least 2 candles for pattern detection
        if candles.len() < 2 {
            return Signal::Flat;
        }

        let prev = &candles[candles.len() - 2];

        // BULLISH ENTRY: Price below range low
        if current.close < range_low {
            // Signal 1: Hammer pattern
            if self.is_hammer(current) {
                return Signal::Long;
            }
            // Signal 2: Bullish engulfing
            if self.is_bullish_engulfing(prev, current) {
                return Signal::Long;
            }
        }

        // BEARISH ENTRY: Price above range high
        // Note: Current implementation only supports Long positions in the backtest engine
        // For short positions, you would return Signal::Short here
        // if current.close > range_high {
        //     if self.is_inverted_hammer(current) {
        //         return Signal::Short;
        //     }
        //     if self.is_bearish_engulfing(prev, current) {
        //         return Signal::Short;
        //     }
        // }

        Signal::Flat
    }

    fn calculate_stop_loss(&self, candles: &[Candle], _entry_price: f64) -> f64 {
        // Stop at the low of the signal candle
        let signal_candle = candles.last().unwrap();
        signal_candle.low
    }

    fn calculate_take_profit(&self, _candles: &[Candle], _entry_price: f64) -> f64 {
        // Target is the opposite side of the range (or 50% mid-point if conservative)
        let state = self.state.read().unwrap();
        if let (Some(high), Some(low)) = (state.range_high, state.range_low) {
            if self.config.conservative_target {
                // Conservative: mid-point (50%) of range
                (high + low) / 2.0
            } else {
                // Primary: opposite side of range (range high for long entries)
                high
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
        if let (Some(high), Some(low)) = (state.range_high, state.range_low) {
            let mid_point = (high + low) / 2.0;
            
            // If we've reached the mid-point or moved back inside the range, move to break-even
            if current_price >= mid_point || (current_price >= low && current_price <= high) {
                // Move to break-even (entry price)
                return Some(position.entry_price);
            }
        }
        
        None
    }

    fn init(&mut self) {
        *self.state.write().unwrap() = SessionState::default();
    }
}
