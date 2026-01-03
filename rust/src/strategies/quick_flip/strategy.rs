//! Quick Flip (Pattern Scalp) Strategy - Production Grade
//!
//! Multi-timeframe range breakout strategy with candlestick pattern confirmation.
//!
//! Architecture:
//! - Primary TF (5m): Pattern detection and entry/exit execution
//! - Range TF (1h): Establishes the "box" - high/low range for breakout detection
//! - Daily TF (1d): ATR calculation for volatility filtering
//!
//! Entry Logic:
//! 1. Daily ATR filter: Range box must be >= min_range_pct of daily ATR
//! 2. Price breaks outside the 1h range box
//! 3. Reversal pattern forms (Hammer/Engulfing for longs, Inverted Hammer/Engulfing for shorts)
//!
//! Exit Logic:
//! - Stop Loss: Signal candle extreme
//! - Take Profit: Opposite side of range (or 50% for conservative mode)
//! - Trailing: Move to breakeven when price re-enters box or hits 50% target

use crate::indicators::atr;
use crate::strategies::Strategy;
use crate::{Candle, MultiTimeframeCandles, Position, Signal, Side, Symbol};
use std::cell::Cell;

use super::config::QuickFlipConfig;

/// Internal state for tracking trade cooldowns and last signal direction
struct State {
    last_trade_bar: Cell<usize>,
    last_signal: Cell<Signal>,
    /// Cached range values for stop/target calculations
    range_high: Cell<f64>,
    range_low: Cell<f64>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            last_trade_bar: Cell::new(0),
            last_signal: Cell::new(Signal::Flat),
            range_high: Cell::new(0.0),
            range_low: Cell::new(0.0),
        }
    }
}

pub struct QuickFlipStrategy {
    config: QuickFlipConfig,
    state: State,
}

impl QuickFlipStrategy {
    pub fn new(config: QuickFlipConfig) -> Self {
        Self {
            config,
            state: State::default(),
        }
    }

    /// Compute ATR from candle slices - optimized for speed
    #[inline]
    fn compute_atr(candles: &[Candle], period: usize) -> Option<f64> {
        if candles.len() < period + 1 {
            return None;
        }

        // Pre-allocate vectors
        let n = candles.len();
        let mut high = Vec::with_capacity(n);
        let mut low = Vec::with_capacity(n);
        let mut close = Vec::with_capacity(n);

        for c in candles {
            high.push(c.high);
            low.push(c.low);
            close.push(c.close);
        }

        let atr_vals = atr(&high, &low, &close, period);
        atr_vals.last().and_then(|&x| x)
    }

    /// Extract range high/low from candles - optimized fold
    #[inline]
    fn compute_range(candles: &[Candle]) -> (f64, f64) {
        let mut high = f64::NEG_INFINITY;
        let mut low = f64::INFINITY;

        for c in candles {
            if c.high > high {
                high = c.high;
            }
            if c.low < low {
                low = c.low;
            }
        }

        (high, low)
    }

    /// Check if candle is a hammer pattern (bullish reversal)
    /// Hammer: small body at top, long lower wick
    #[inline]
    fn is_hammer(candle: &Candle) -> bool {
        let range = candle.high - candle.low;
        if range <= 0.0 {
            return false;
        }

        let body = (candle.close - candle.open).abs();
        let lower_wick = candle.open.min(candle.close) - candle.low;
        let upper_wick = candle.high - candle.open.max(candle.close);

        // Body <= 35% of range, lower wick >= 55%, upper wick <= 20%
        body / range <= 0.35 && lower_wick / range >= 0.55 && upper_wick / range <= 0.20
    }

    /// Check if candle is an inverted hammer (bearish reversal at top)
    #[inline]
    fn is_inverted_hammer(candle: &Candle) -> bool {
        let range = candle.high - candle.low;
        if range <= 0.0 {
            return false;
        }

        let body = (candle.close - candle.open).abs();
        let lower_wick = candle.open.min(candle.close) - candle.low;
        let upper_wick = candle.high - candle.open.max(candle.close);

        // Body < 30% of range, upper wick > 60%, lower wick < 20%
        body / range < 0.30 && upper_wick / range > 0.60 && lower_wick / range < 0.20
    }

    /// Check for bullish engulfing pattern
    #[inline]
    fn is_bullish_engulfing(prev: &Candle, curr: &Candle) -> bool {
        let prev_bearish = prev.close < prev.open;
        let curr_bullish = curr.close > curr.open;

        if !prev_bearish || !curr_bullish {
            return false;
        }

        let prev_body = (prev.open - prev.close).abs();
        let curr_body = (curr.close - curr.open).abs();
        let prev_range = prev.high - prev.low;
        let curr_range = curr.high - curr.low;

        // Both must have meaningful bodies
        let prev_has_body = prev_range > 0.0 && prev_body / prev_range > 0.3;
        let curr_has_body = curr_range > 0.0 && curr_body / curr_range > 0.3;

        // Current engulfs previous
        let engulfs = curr.open <= prev.close && curr.close >= prev.open;
        let adequate_size = curr_body >= prev_body * 0.9;

        prev_has_body && curr_has_body && engulfs && adequate_size
    }

    /// Check for bearish engulfing pattern
    #[inline]
    fn is_bearish_engulfing(prev: &Candle, curr: &Candle) -> bool {
        let prev_bullish = prev.close > prev.open;
        let curr_bearish = curr.close < curr.open;

        if !prev_bullish || !curr_bearish {
            return false;
        }

        // Current engulfs previous
        curr.open >= prev.close && curr.close <= prev.open
    }
}

impl Strategy for QuickFlipStrategy {
    fn name(&self) -> &'static str {
        "quick_flip"
    }

    /// Required timeframes: 1d for ATR, 1h for range box
    fn required_timeframes(&self) -> Vec<&'static str> {
        vec!["1d", "1h"]
    }

    /// Multi-timeframe signal generation - the core trading logic
    fn generate_signal_mtf(
        &self,
        _symbol: &Symbol,
        mtf: &MultiTimeframeCandles,
        position: Option<&Position>,
    ) -> Signal {
        // Get required timeframes - fail fast if missing
        let candles_5m = mtf.primary();
        let candles_1h = mtf.get("1h").unwrap_or(&[]);
        let candles_1d = mtf.get("1d").unwrap_or(&[]);

        // Minimum data requirements - need enough for indicators
        // ATR needs 14+1 bars, so we need at least 15 daily candles
        if candles_5m.len() < 20 {
            return Signal::Flat;
        }
        if candles_1h.len() < self.config.opening_bars.max(1) {
            return Signal::Flat;
        }
        if candles_1d.len() < 15 {
            return Signal::Flat;
        }

        let current_bar = candles_5m.len();

        // If in position, maintain it (exits handled by backtester)
        if let Some(pos) = position {
            return match pos.side {
                Side::Buy => Signal::Long,
                Side::Sell => Signal::Short,
            };
        }

        // Cooldown check - prevent overtrading
        if current_bar.saturating_sub(self.state.last_trade_bar.get()) < self.config.cooldown_bars {
            return Signal::Flat;
        }

        // Step 1: Calculate daily ATR for volatility filter
        let daily_atr = match Self::compute_atr(candles_1d, 14) {
            Some(atr) => atr,
            None => return Signal::Flat,
        };

        // Step 2: Compute 1h range box from the last N bars
        let window_start = candles_1h.len().saturating_sub(self.config.opening_bars.max(1));
        let (range_high, range_low) = Self::compute_range(&candles_1h[window_start..]);

        if range_high <= range_low || range_high == f64::NEG_INFINITY {
            return Signal::Flat;
        }

        // Step 3: ATR filter - range must be significant
        let range_size = range_high - range_low;
        let min_required = daily_atr * self.config.min_range_pct;
        
        // Debug: sample logging
        static DEBUG_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let count = DEBUG_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if count % 5000 == 0 {
            tracing::debug!(
                "QF: bar={} atr={:.2} range={:.2} min_req={:.2} close={:.2} rh={:.2} rl={:.2}",
                current_bar, daily_atr, range_size, min_required,
                candles_5m.last().map(|c| c.close).unwrap_or(0.0),
                range_high, range_low
            );
        }
        
        if range_size < min_required {
            return Signal::Flat;
        }

        // Cache range for stop/target calculations
        self.state.range_high.set(range_high);
        self.state.range_low.set(range_low);

        // Step 4: Pattern detection on 5m
        let curr = &candles_5m[candles_5m.len() - 1];
        let prev = &candles_5m[candles_5m.len() - 2];

        // BULLISH: Price below range + reversal pattern
        if curr.close < range_low {
            let is_hammer = Self::is_hammer(curr);
            let is_engulf = Self::is_bullish_engulfing(prev, curr);
            if count % 1000 == 0 {
                tracing::debug!("QF BELOW: close={:.2} < rl={:.2}, hammer={}, engulf={}", 
                    curr.close, range_low, is_hammer, is_engulf);
            }
            if is_hammer || is_engulf {
                self.state.last_signal.set(Signal::Long);
                self.state.last_trade_bar.set(current_bar);
                tracing::info!("QF LONG SIGNAL at bar {}", current_bar);
                return Signal::Long;
            }
        }

        // BEARISH: Price above range + reversal pattern
        if curr.close > range_high {
            let is_inv_hammer = Self::is_inverted_hammer(curr);
            let is_engulf = Self::is_bearish_engulfing(prev, curr);
            if count % 1000 == 0 {
                tracing::debug!("QF ABOVE: close={:.2} > rh={:.2}, inv_hammer={}, engulf={}", 
                    curr.close, range_high, is_inv_hammer, is_engulf);
            }
            if is_inv_hammer || is_engulf {
                self.state.last_signal.set(Signal::Short);
                self.state.last_trade_bar.set(current_bar);
                tracing::info!("QF SHORT SIGNAL at bar {}", current_bar);
                return Signal::Short;
            }
        }

        Signal::Flat
    }

    /// Single-TF fallback - uses same logic with primary TF only
    fn generate_signal(
        &self,
        _symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal {
        let min_len = self.config.atr_period + self.config.opening_bars + 10;
        if candles.len() < min_len {
            return Signal::Flat;
        }

        let current_bar = candles.len();

        if let Some(pos) = position {
            return match pos.side {
                Side::Buy => Signal::Long,
                Side::Sell => Signal::Short,
            };
        }

        if current_bar.saturating_sub(self.state.last_trade_bar.get()) < self.config.cooldown_bars {
            return Signal::Flat;
        }

        // Single-TF: use primary data for both ATR and range
        let atr_val = match Self::compute_atr(candles, self.config.atr_period) {
            Some(a) => a,
            None => return Signal::Flat,
        };

        let window_start = candles.len().saturating_sub(self.config.opening_bars + 1);
        let window_end = candles.len() - 1;
        let (range_high, range_low) = Self::compute_range(&candles[window_start..window_end]);

        let range_size = range_high - range_low;
        if range_size < atr_val * self.config.min_range_pct {
            return Signal::Flat;
        }

        self.state.range_high.set(range_high);
        self.state.range_low.set(range_low);

        let curr = &candles[candles.len() - 1];
        let prev = &candles[candles.len() - 2];

        if curr.close < range_low {
            if Self::is_hammer(curr) || Self::is_bullish_engulfing(prev, curr) {
                self.state.last_signal.set(Signal::Long);
                self.state.last_trade_bar.set(current_bar);
                return Signal::Long;
            }
        }

        if curr.close > range_high {
            if Self::is_inverted_hammer(curr) || Self::is_bearish_engulfing(prev, curr) {
                self.state.last_signal.set(Signal::Short);
                self.state.last_trade_bar.set(current_bar);
                return Signal::Short;
            }
        }

        Signal::Flat
    }

    fn calculate_stop_loss(&self, candles: &[Candle], _entry_price: f64) -> f64 {
        let signal_candle = candles.last().unwrap();
        match self.state.last_signal.get() {
            Signal::Long => signal_candle.low,
            Signal::Short => signal_candle.high,
            Signal::Flat => signal_candle.low,
        }
    }

    fn calculate_take_profit(&self, _candles: &[Candle], _entry_price: f64) -> f64 {
        let range_high = self.state.range_high.get();
        let range_low = self.state.range_low.get();

        match self.state.last_signal.get() {
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
            Signal::Flat => range_high,
        }
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        _candles: &[Candle],
    ) -> Option<f64> {
        let range_high = self.state.range_high.get();
        let range_low = self.state.range_low.get();
        let mid_point = (range_high + range_low) / 2.0;

        // Move to breakeven when price re-enters box or hits 50% target
        match position.side {
            Side::Buy => {
                if current_price >= mid_point || (current_price >= range_low && current_price <= range_high) {
                    Some(position.entry_price)
                } else {
                    None
                }
            }
            Side::Sell => {
                if current_price <= mid_point || (current_price >= range_low && current_price <= range_high) {
                    Some(position.entry_price)
                } else {
                    None
                }
            }
        }
    }

    fn init(&mut self) {
        self.state = State::default();
    }
}

// Strategy must be Send + Sync for parallel optimization
unsafe impl Send for QuickFlipStrategy {}
unsafe impl Sync for QuickFlipStrategy {}
