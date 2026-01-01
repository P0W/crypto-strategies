//! Quick Flip (Pattern Scalp) Strategy
//!
//! Range breakout with candlestick pattern confirmation.
//! Adapted for 24/7 crypto markets using rolling range windows.
//! 
//! Core Logic:
//! 1. Define range using N-bar high/low lookback (default: 3 bars = 15 minutes on 5m chart)
//! 2. Filter by ATR (range must be >= min % of ATR)
//! 3. Wait for price to move outside the range
//! 4. Enter on pattern confirmation (Hammer or Bullish Engulfing)
//! 5. Stop at signal candle low, target at range high

use crate::indicators::atr;
use crate::strategies::Strategy;
use crate::{Candle, Position, Signal, Symbol};
use std::sync::RwLock;

use super::config::QuickFlipConfig;

#[derive(Debug, Clone)]
struct TradeState {
    last_trade_bar: usize,
}

impl Default for TradeState {
    fn default() -> Self {
        Self { last_trade_bar: 0 }
    }
}

pub struct QuickFlipStrategy {
    config: QuickFlipConfig,
    state: RwLock<TradeState>,
}

impl QuickFlipStrategy {
    pub fn new(config: QuickFlipConfig) -> Self {
        Self {
            config,
            state: RwLock::new(TradeState::default()),
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

    /// Calculate ATR for volatility measurement
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

    /// Get the range high from lookback period
    fn get_range_high(&self, candles: &[Candle]) -> Option<f64> {
        if candles.len() < self.config.range_lookback + 1 {
            return None;
        }
        // Exclude current bar, look at previous N bars
        let start = candles.len() - self.config.range_lookback - 1;
        let end = candles.len() - 1;
        candles[start..end]
            .iter()
            .map(|c| c.high)
            .fold(None, |max, h| Some(max.map_or(h, |m: f64| m.max(h))))
    }

    /// Get the range low from lookback period
    fn get_range_low(&self, candles: &[Candle]) -> Option<f64> {
        if candles.len() < self.config.range_lookback + 1 {
            return None;
        }
        let start = candles.len() - self.config.range_lookback - 1;
        let end = candles.len() - 1;
        candles[start..end]
            .iter()
            .map(|c| c.low)
            .fold(None, |min, l| Some(min.map_or(l, |m: f64| m.min(l))))
    }

    /// Check if the range passes ATR filter
    fn passes_atr_filter(&self, range_high: f64, range_low: f64, atr_val: f64) -> bool {
        let range_size = range_high - range_low;
        let min_required = atr_val * self.config.min_range_pct;
        range_size >= min_required
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
        let min_bars = self.config.range_lookback.max(self.config.atr_period) + 10;
        if candles.len() < min_bars {
            return Signal::Flat;
        }

        // If already in position, hold
        if position.is_some() {
            return Signal::Long;
        }

        // Cooldown: prevent too frequent trading
        let state = self.state.read().unwrap();
        if candles.len() - state.last_trade_bar < self.config.cooldown_bars {
            return Signal::Flat;
        }
        drop(state);

        let current = candles.last().unwrap();
        
        // Get range and ATR
        let range_high = match self.get_range_high(candles) {
            Some(h) => h,
            None => return Signal::Flat,
        };
        let range_low = match self.get_range_low(candles) {
            Some(l) => l,
            None => return Signal::Flat,
        };
        let atr_val = match self.get_current_atr(candles) {
            Some(a) => a,
            None => return Signal::Flat,
        };

        // Check ATR filter
        if !self.passes_atr_filter(range_high, range_low, atr_val) {
            return Signal::Flat;
        }

        // Need at least 2 candles for pattern detection
        if candles.len() < 2 {
            return Signal::Flat;
        }

        let prev = &candles[candles.len() - 2];

        // BULLISH ENTRY: Price below range low (outside the box)
        // AND showing reversal pattern
        if current.close < range_low {
            // Additional filter: price must be reasonably below range low (but not too far)
            let breakout_distance = (range_low - current.close) / atr_val;
            if breakout_distance < 0.3 || breakout_distance > 2.0 {
                // Either too close or too far from range
                return Signal::Flat;
            }
            
            // Signal 1: Hammer pattern
            if self.is_hammer(current) {
                return Signal::Long;
            }
            // Signal 2: Bullish engulfing
            if self.is_bullish_engulfing(prev, current) {
                return Signal::Long;
            }
        }

        Signal::Flat
    }

    fn calculate_stop_loss(&self, candles: &[Candle], _entry_price: f64) -> f64 {
        // Stop at the low of the signal candle
        let signal_candle = candles.last().unwrap();
        signal_candle.low
    }

    fn calculate_take_profit(&self, candles: &[Candle], _entry_price: f64) -> f64 {
        // Target is the opposite side of the range (or 50% mid-point if conservative)
        let range_high = self.get_range_high(candles).unwrap_or(_entry_price * 1.02);
        let range_low = self.get_range_low(candles).unwrap_or(_entry_price * 0.98);
        
        if self.config.conservative_target {
            // Conservative: mid-point (50%) of range
            (range_high + range_low) / 2.0
        } else {
            // Primary: opposite side of range (range high for long entries)
            range_high
        }
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        // Move stop to break-even once price moves back inside the box or reaches 50% target
        let range_high = self.get_range_high(candles)?;
        let range_low = self.get_range_low(candles)?;
        let mid_point = (range_high + range_low) / 2.0;
        
        // If we've reached the mid-point or moved back inside the range, move to break-even
        if current_price >= mid_point || (current_price >= range_low && current_price <= range_high) {
            // Move to break-even (entry price)
            return Some(position.entry_price);
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
        *self.state.write().unwrap() = TradeState::default();
    }
}
