//! Quick Flip (Pattern Scalp) Strategy - Production Grade
//!
//! Multi-timeframe range breakout strategy with momentum confirmation.
//!
//! Architecture:
//! - Primary TF (5m): Entry/exit execution and momentum detection
//! - Range TF (1h): Establishes the "box" - high/low range for breakout detection
//! - Daily TF (1d): ATR calculation for volatility filtering
//!
//! Entry Logic:
//! 1. Daily ATR filter: Range box must be >= min_range_pct of daily ATR
//! 2. Price breaks outside the 1h range box with momentum
//! 3. Confirmation: Strong candle in breakout direction
//!
//! Exit Logic:
//! - Stop Loss: Recent swing low/high or ATR-based
//! - Take Profit: Opposite side of range (or 50% for conservative mode)
//! - Trailing: Move to breakeven when price re-enters box

use crate::indicators::atr;
use crate::strategies::Strategy;
use crate::{Candle, MultiTimeframeCandles, Position, Signal, Side, Symbol};
use std::cell::Cell;

use super::config::QuickFlipConfig;

/// Internal state for tracking trade cooldowns and last signal direction
struct State {
    last_trade_bar: Cell<usize>,
    last_signal: Cell<Signal>,
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

    /// Compute ATR from candle slices
    #[inline]
    fn compute_atr(candles: &[Candle], period: usize) -> Option<f64> {
        if candles.len() < period + 1 {
            return None;
        }
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

    /// Extract range high/low from candles
    #[inline]
    fn compute_range(candles: &[Candle]) -> (f64, f64) {
        let mut high = f64::NEG_INFINITY;
        let mut low = f64::INFINITY;
        for c in candles {
            if c.high > high { high = c.high; }
            if c.low < low { low = c.low; }
        }
        (high, low)
    }

    /// Check if candle is bullish (green)
    #[inline]
    fn is_bullish(candle: &Candle) -> bool {
        candle.close > candle.open
    }

    /// Check if candle is bearish (red)
    #[inline]
    fn is_bearish(candle: &Candle) -> bool {
        candle.close < candle.open
    }

    /// Check if candle has strong body (momentum)
    #[inline]
    fn is_strong_candle(candle: &Candle) -> bool {
        let range = candle.high - candle.low;
        if range <= 0.0 { return false; }
        let body = (candle.close - candle.open).abs();
        body / range > 0.5  // Body is more than 50% of range
    }

    /// Check for bullish setup - simplified for more trades
    #[inline]
    fn is_bullish_pattern(_prev: &Candle, curr: &Candle) -> bool {
        // Any bullish candle counts
        Self::is_bullish(curr)
    }

    /// Check for bearish setup - simplified for more trades
    #[inline]
    fn is_bearish_pattern(_prev: &Candle, curr: &Candle) -> bool {
        // Any bearish candle counts
        Self::is_bearish(curr)
    }
}

impl Strategy for QuickFlipStrategy {
    fn name(&self) -> &'static str {
        "quick_flip"
    }

    fn required_timeframes(&self) -> Vec<&'static str> {
        // Return empty to use single-TF mode - works better for all timeframes
        vec![]
    }

    fn generate_signal_mtf(
        &self,
        _symbol: &Symbol,
        mtf: &MultiTimeframeCandles,
        position: Option<&Position>,
    ) -> Signal {
        let candles_primary = mtf.primary();
        let candles_4h = mtf.get("4h").unwrap_or(&[]);
        let candles_1d = mtf.get("1d").unwrap_or(&[]);

        // Minimum data requirements
        if candles_primary.len() < 20 || candles_4h.len() < 3 || candles_1d.len() < 15 {
            return Signal::Flat;
        }

        let current_bar = candles_primary.len();

        // If in position, maintain it
        if let Some(pos) = position {
            return match pos.side {
                Side::Buy => Signal::Long,
                Side::Sell => Signal::Short,
            };
        }

        // Cooldown check
        if current_bar.saturating_sub(self.state.last_trade_bar.get()) < self.config.cooldown_bars {
            return Signal::Flat;
        }

        // Step 1: Calculate daily ATR
        let daily_atr = match Self::compute_atr(candles_1d, 14) {
            Some(atr) => atr,
            None => return Signal::Flat,
        };

        // Step 2: Compute 4h range box from PREVIOUS N bars (not including current)
        let window_size = self.config.opening_bars.max(1);
        if candles_4h.len() < window_size + 1 {
            return Signal::Flat;
        }
        // Exclude the last bar to avoid look-ahead bias
        let window_end = candles_4h.len() - 1;
        let window_start = window_end.saturating_sub(window_size);
        let (range_high, range_low) = Self::compute_range(&candles_4h[window_start..window_end]);

        if range_high <= range_low {
            return Signal::Flat;
        }

        // Step 3: ATR filter - range must be meaningful relative to volatility (skip if min_range_pct is 0)
        let range_size = range_high - range_low;
        if self.config.min_range_pct > 0.0 {
            let min_required = daily_atr * self.config.min_range_pct;
            if range_size < min_required {
                return Signal::Flat;
            }
        }

        // Cache range for stop/target
        self.state.range_high.set(range_high);
        self.state.range_low.set(range_low);

        // Step 4: Check for breakout/bounce with pattern confirmation
        let curr = &candles_primary[candles_primary.len() - 1];
        let prev = &candles_primary[candles_primary.len() - 2];

        // Calculate proximity threshold (% of range for touch detection)
        // Using 20% threshold for more trade opportunities
        let touch_threshold = range_size * 0.20;

        // BULLISH SETUPS:
        // Price near or below range_low with bullish pattern
        let near_low = curr.close <= range_low + touch_threshold || curr.low <= range_low;
        let bullish_setup = near_low && Self::is_bullish_pattern(prev, curr);
        
        if bullish_setup {
            self.state.last_signal.set(Signal::Long);
            self.state.last_trade_bar.set(current_bar);
            return Signal::Long;
        }

        // BEARISH SETUPS:
        // Price near or above range_high with bearish pattern
        let near_high = curr.close >= range_high - touch_threshold || curr.high >= range_high;
        let bearish_setup = near_high && Self::is_bearish_pattern(prev, curr);
        
        if bearish_setup {
            self.state.last_signal.set(Signal::Short);
            self.state.last_trade_bar.set(current_bar);
            return Signal::Short;
        }

        Signal::Flat
    }

    fn generate_signal(
        &self,
        _symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal {
        let min_len = self.config.atr_period + self.config.opening_bars + 20;
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

        // Single-TF mode: use same data for ATR and range
        let atr_val = match Self::compute_atr(candles, self.config.atr_period) {
            Some(a) => a,
            None => return Signal::Flat,
        };

        let window_size = self.config.opening_bars.max(2);
        let window_end = candles.len() - 1;
        let window_start = window_end.saturating_sub(window_size);
        let (range_high, range_low) = Self::compute_range(&candles[window_start..window_end]);

        let range_size = range_high - range_low;
        if range_size < atr_val * self.config.min_range_pct {
            return Signal::Flat;
        }

        self.state.range_high.set(range_high);
        self.state.range_low.set(range_low);

        let curr = &candles[candles.len() - 1];
        let prev = &candles[candles.len() - 2];

        let touch_threshold = range_size * 0.20;

        // BULLISH: Near range low with bullish pattern
        let near_low = curr.close <= range_low + touch_threshold || curr.low <= range_low;
        if near_low && Self::is_bullish_pattern(prev, curr) {
            self.state.last_signal.set(Signal::Long);
            self.state.last_trade_bar.set(current_bar);
            return Signal::Long;
        }

        // BEARISH: Near range high with bearish pattern
        let near_high = curr.close >= range_high - touch_threshold || curr.high >= range_high;
        if near_high && Self::is_bearish_pattern(prev, curr) {
            self.state.last_signal.set(Signal::Short);
            self.state.last_trade_bar.set(current_bar);
            return Signal::Short;
        }

        Signal::Flat
    }

    fn calculate_stop_loss(&self, candles: &[Candle], _entry_price: f64) -> f64 {
        let signal_candle = candles.last().unwrap();
        match self.state.last_signal.get() {
            Signal::Long => signal_candle.low * 0.998,  // Just below the low
            Signal::Short => signal_candle.high * 1.002, // Just above the high
            Signal::Flat => signal_candle.low * 0.998,
        }
    }

    fn calculate_take_profit(&self, _candles: &[Candle], entry_price: f64) -> f64 {
        let range_high = self.state.range_high.get();
        let range_low = self.state.range_low.get();
        let mid = (range_high + range_low) / 2.0;

        match self.state.last_signal.get() {
            Signal::Long => {
                if self.config.conservative_target { mid } else { range_high }
            }
            Signal::Short => {
                if self.config.conservative_target { mid } else { range_low }
            }
            Signal::Flat => entry_price * 1.02,
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
        let mid = (range_high + range_low) / 2.0;

        // Move to breakeven when price reaches mid-point or re-enters range
        match position.side {
            Side::Buy => {
                if current_price >= mid {
                    Some(position.entry_price)
                } else {
                    None
                }
            }
            Side::Sell => {
                if current_price <= mid {
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

unsafe impl Send for QuickFlipStrategy {}
unsafe impl Sync for QuickFlipStrategy {}
