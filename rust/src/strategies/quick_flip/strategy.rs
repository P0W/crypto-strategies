//! Quick Flip (Pattern Scalp) Strategy
//!
//! Rolling range breakout strategy adapted for crypto 24/7 markets.
//! 
//! Core Logic:
//! 1. Use ATR for volatility measurement (default: 96 bars = ~8 hours on 5m)
//! 2. Establish range from rolling N-bar window (default: 6 bars = 30 min on 5m)
//! 3. Range must be >= min % of ATR to be valid (default: 25%)
//! 4. Wait for price to break outside the range
//! 5. Enter on pattern confirmation (Hammer/Inverted Hammer or Engulfing)
//! 6. Stop at signal candle extreme, target at opposite side of range

use crate::indicators::atr;
use crate::strategies::Strategy;
use crate::{Candle, Position, Signal, Symbol};
use std::sync::RwLock;

use super::config::QuickFlipConfig;

#[derive(Debug, Clone)]
struct SessionState {
    last_trade_bar: usize,
    last_signal: Signal,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
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

    /// Get the opening range using a rolling window approach
    /// Uses the high/low from the last N bars (opening_bars parameter)
    fn get_range_from_window(&self, candles: &[Candle]) -> (Option<f64>, Option<f64>) {
        if candles.len() < self.config.opening_bars + 1 {
            return (None, None);
        }
        
        // Look at the previous opening_bars (not including current bar)
        let start_idx = candles.len() - self.config.opening_bars - 1;
        let end_idx = candles.len() - 1;
        let window = &candles[start_idx..end_idx];
        
        let range_high = window.iter().map(|c| c.high).fold(None, |max, h| {
            Some(max.map_or(h, |m: f64| m.max(h)))
        });
        let range_low = window.iter().map(|c| c.low).fold(None, |min, l| {
            Some(min.map_or(l, |m: f64| m.min(l)))
        });
        
        (range_high, range_low)
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
    fn passes_atr_filter(&self, range_high: f64, range_low: f64, atr_val: f64) -> bool {
        let range_size = range_high - range_low;
        let min_required = atr_val * self.config.min_range_pct;
        range_size >= min_required
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

    /// Quick Flip uses 1d and 15m timeframes in addition to the primary 5m
    fn required_timeframes(&self) -> Vec<&'static str> {
        vec!["1d", "15m"]
    }

    /// Multi-timeframe signal generation
    /// Uses: 1d for ATR, 15m for range box, 5m (primary) for pattern detection
    fn generate_signal_mtf(
        &self,
        _symbol: &Symbol,
        mtf_candles: &crate::MultiTimeframeCandles,
        position: Option<&Position>,
    ) -> Signal {
        // Get all required timeframes
        let candles_5m = mtf_candles.primary(); // 5m for pattern detection
        let candles_15m = match mtf_candles.get("15m") {
            Some(c) => c,
            None => {
                // Fallback to single-timeframe mode if 15m not available
                tracing::warn!("15m data not available, falling back to single-TF mode");
                return self.generate_signal(_symbol, candles_5m, position);
            }
        };
        let candles_1d = match mtf_candles.get("1d") {
            Some(c) => c,
            None => {
                tracing::warn!("1d data not available, falling back to single-TF mode");
                return self.generate_signal(_symbol, candles_5m, position);
            }
        };

        // Need minimum data
        if candles_5m.len() < 20 || candles_15m.len() < 5 || candles_1d.len() < 14 {
            return Signal::Flat;
        }

        let current_bar = candles_5m.len();

        // If already in position, hold
        if position.is_some() {
            return Signal::Long;
        }

        // Cooldown check
        {
            let state = self.state.read().unwrap();
            if current_bar - state.last_trade_bar < self.config.cooldown_bars {
                return Signal::Flat;
            }
        }

        // Calculate daily ATR (14 period on 1d chart)
        let daily_atr = {
            let high: Vec<f64> = candles_1d.iter().map(|c| c.high).collect();
            let low: Vec<f64> = candles_1d.iter().map(|c| c.low).collect();
            let close: Vec<f64> = candles_1d.iter().map(|c| c.close).collect();
            let atr_vals = atr(&high, &low, &close, 14);
            match atr_vals.last() {
                Some(&Some(val)) => val,
                _ => return Signal::Flat,
            }
        };

        // Get 15m range box (first N bars, or latest N bars for rolling window)
        let (range_high, range_low) = {
            if candles_15m.len() < self.config.opening_bars {
                return Signal::Flat;
            }
            
            // Use latest N bars on 15m as the range
            let window_start = candles_15m.len().saturating_sub(self.config.opening_bars);
            let window = &candles_15m[window_start..];
            
            let high = window.iter().map(|c| c.high).fold(None, |max, h| {
                Some(max.map_or(h, |m: f64| m.max(h)))
            });
            let low = window.iter().map(|c| c.low).fold(None, |min, l| {
                Some(min.map_or(l, |m: f64| m.min(l)))
            });
            
            match (high, low) {
                (Some(h), Some(l)) => (h, l),
                _ => return Signal::Flat,
            }
        };

        // Check ATR filter: range must be >= 25% of daily ATR
        let range_size = range_high - range_low;
        let min_required = daily_atr * self.config.min_range_pct;
        if range_size < min_required {
            return Signal::Flat;
        }

        // Pattern detection on 5m chart
        if candles_5m.len() < 2 {
            return Signal::Flat;
        }

        let current = candles_5m.last().unwrap();
        let prev = &candles_5m[candles_5m.len() - 2];

        // BULLISH ENTRY: Price below 15m range low
        if current.close < range_low {
            if self.is_hammer(current) || self.is_bullish_engulfing(prev, current) {
                self.state.write().unwrap().last_signal = Signal::Long;
                return Signal::Long;
            }
        }

        // BEARISH ENTRY: Price above 15m range high
        if current.close > range_high {
            if self.is_inverted_hammer(current) || self.is_bearish_engulfing(prev, current) {
                self.state.write().unwrap().last_signal = Signal::Short;
                return Signal::Short;
            }
        }

        Signal::Flat
    }

    fn generate_signal(
        &self,
        _symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal {
        // Single-timeframe fallback mode
        // Need enough data for ATR calculation and range window
        if candles.len() < self.config.atr_period + self.config.opening_bars + 10 {
            return Signal::Flat;
        }

        let current_bar = candles.len();

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

        // Get rolling window range
        let (range_high, range_low) = match self.get_range_from_window(candles) {
            (Some(h), Some(l)) => (h, l),
            _ => return Signal::Flat,
        };

        // Get ATR and check filter
        let atr_val = match self.get_current_atr(candles) {
            Some(a) => a,
            None => return Signal::Flat,
        };

        if !self.passes_atr_filter(range_high, range_low, atr_val) {
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

    fn calculate_take_profit(&self, candles: &[Candle], _entry_price: f64) -> f64 {
        // Target is the opposite side of the range (or 50% mid-point if conservative)
        let state = self.state.read().unwrap();
        let last_signal = state.last_signal;
        drop(state);
        
        // Get current range from rolling window
        let (range_high, range_low) = match self.get_range_from_window(candles) {
            (Some(h), Some(l)) => (h, l),
            _ => return _entry_price * 1.02, // Fallback
        };
        
        match last_signal {
            Signal::Long => {
                if self.config.conservative_target {
                    (range_high + range_low) / 2.0
                } else {
                    range_high
                }
            }
            Signal::Short => {
                if self.config.conservative_target {
                    (range_high + range_low) / 2.0
                } else {
                    range_low
                }
            }
            Signal::Flat => {
                if self.config.conservative_target {
                    (range_high + range_low) / 2.0
                } else {
                    range_high
                }
            }
        }
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        // Move stop to break-even once price moves back inside the box or reaches 50% target
        let (range_high, range_low) = match self.get_range_from_window(candles) {
            (Some(h), Some(l)) => (h, l),
            _ => return None,
        };
        
        let mid_point = (range_high + range_low) / 2.0;
        
        match position.side {
            crate::Side::Buy => {
                // For Long: If we've reached the mid-point or moved back inside the range, move to break-even
                if current_price >= mid_point || (current_price >= range_low && current_price <= range_high) {
                    return Some(position.entry_price);
                }
            }
            crate::Side::Sell => {
                // For Short: If we've reached the mid-point or moved back inside the range, move to break-even
                if current_price <= mid_point || (current_price >= range_low && current_price <= range_high) {
                    return Some(position.entry_price);
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
