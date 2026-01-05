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
use crate::oms::{OrderRequest, StrategyContext};
use crate::strategies::Strategy;
use crate::{Candle, MultiTimeframeCandles, Position, Side};
use std::sync::Mutex;

use super::config::QuickFlipConfig;

/// Internal state for tracking trade cooldowns
/// Uses Mutex for thread-safe interior mutability (no unsafe code)
struct State {
    last_trade_bar: usize,
    range_high: f64,
    range_low: f64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            last_trade_bar: 0,
            range_high: 0.0,
            range_low: 0.0,
        }
    }
}

pub struct QuickFlipStrategy {
    config: QuickFlipConfig,
    state: Mutex<State>,
}

impl QuickFlipStrategy {
    pub fn new(config: QuickFlipConfig) -> Self {
        Self {
            config,
            state: Mutex::new(State::default()),
        }
    }

    /// Generate orders using multi-timeframe data
    fn generate_orders_mtf(
        &self,
        ctx: &StrategyContext,
        mtf: &MultiTimeframeCandles,
    ) -> Vec<OrderRequest> {
        let mut orders = Vec::new();

        let candles_primary = mtf.primary();
        let candles_4h = mtf.get("4h").unwrap_or(&[]);
        let candles_1d = mtf.get("1d").unwrap_or(&[]);

        // Minimum data requirements
        if candles_primary.len() < 20 || candles_4h.len() < 3 || candles_1d.len() < 15 {
            return orders;
        }

        let current_bar = candles_primary.len();

        // If in position, hold
        if ctx.current_position.is_some() {
            return orders;
        }

        // Cooldown after last trade
        {
            let state = self.state.lock().unwrap();
            if current_bar.saturating_sub(state.last_trade_bar) < self.config.cooldown_bars {
                return orders;
            }
        }

        // Daily ATR volatility filter
        let atr_val = match Self::compute_atr(candles_1d, self.config.atr_period) {
            Some(a) => a,
            None => return orders,
        };

        // Range box = past N bars of the range TF (e.g., 4h)
        let (range_high, range_low) = Self::compute_range(candles_4h);
        let range_size = range_high - range_low;

        // Filter: ignore tiny ranges (noise)
        if range_size < atr_val * self.config.min_range_pct {
            return orders;
        }

        {
            let mut state = self.state.lock().unwrap();
            state.range_high = range_high;
            state.range_low = range_low;
        }

        let curr = &candles_primary[candles_primary.len() - 1];
        let prev = &candles_primary[candles_primary.len() - 2];

        // STRATEGY: Quick Flip = trade BOTH reversals AND breakouts in both directions

        // BREAKOUT LONG: Price breaks above range high with momentum
        let breakout_long =
            curr.close > range_high && Self::is_bullish(curr) && Self::is_strong_candle(curr);
        if breakout_long {
            let mut state = self.state.lock().unwrap();
            state.last_trade_bar = current_bar;
            orders.push(OrderRequest::market_buy(ctx.symbol.clone(), 1.0));
            return orders;
        }

        // BREAKOUT SHORT: Price breaks below range low with momentum
        let breakout_short =
            curr.close < range_low && Self::is_bearish(curr) && Self::is_strong_candle(curr);
        if breakout_short {
            let mut state = self.state.lock().unwrap();
            state.last_trade_bar = current_bar;
            orders.push(OrderRequest::market_sell(ctx.symbol.clone(), 1.0));
            return orders;
        }

        // REVERSAL LONG: Price near/below range low, bullish candle
        let touch_threshold = range_size * 0.30;
        let near_low = curr.close <= range_low + touch_threshold || curr.low <= range_low;
        if near_low && Self::is_bullish_pattern(prev, curr) {
            let mut state = self.state.lock().unwrap();
            state.last_trade_bar = current_bar;
            orders.push(OrderRequest::market_buy(ctx.symbol.clone(), 1.0));
            return orders;
        }

        // REVERSAL SHORT: Price near/above range high, bearish candle
        let near_high = curr.close >= range_high - touch_threshold || curr.high >= range_high;
        if near_high && Self::is_bearish_pattern(prev, curr) {
            let mut state = self.state.lock().unwrap();
            state.last_trade_bar = current_bar;
            orders.push(OrderRequest::market_sell(ctx.symbol.clone(), 1.0));
        }

        orders
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
            if c.high > high {
                high = c.high;
            }
            if c.low < low {
                low = c.low;
            }
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
        if range <= 0.0 {
            return false;
        }
        let body = (candle.close - candle.open).abs();
        body / range > 0.5 // Body is more than 50% of range
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

    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        let mut orders = Vec::new();

        // Check if we have multi-timeframe data
        if let Some(mtf) = ctx.mtf_candles {
            // Use multi-timeframe logic
            return self.generate_orders_mtf(ctx, mtf);
        }

        // Single-timeframe logic
        let min_len = self.config.atr_period + self.config.opening_bars + 20;
        if ctx.candles.len() < min_len {
            return orders;
        }

        let current_bar = ctx.candles.len();

        if ctx.current_position.is_some() {
            // Hold position
            return orders;
        }

        {
            let state = self.state.lock().unwrap();
            if current_bar.saturating_sub(state.last_trade_bar) < self.config.cooldown_bars {
                return orders;
            }
        }

        // Single-TF mode: use same data for ATR and range
        let atr_val = match Self::compute_atr(ctx.candles, self.config.atr_period) {
            Some(a) => a,
            None => return orders,
        };

        let window_size = self.config.opening_bars.max(2);
        let window_end = ctx.candles.len() - 1;
        let window_start = window_end.saturating_sub(window_size);
        let (range_high, range_low) = Self::compute_range(&ctx.candles[window_start..window_end]);

        let range_size = range_high - range_low;
        if range_size < atr_val * self.config.min_range_pct {
            return orders;
        }

        {
            let mut state = self.state.lock().unwrap();
            state.range_high = range_high;
            state.range_low = range_low;
        }

        let curr = &ctx.candles[ctx.candles.len() - 1];
        let prev = &ctx.candles[ctx.candles.len() - 2];

        // STRATEGY: Quick Flip = trade BOTH reversals AND breakouts in both directions

        // BREAKOUT LONG: Price breaks above range high with momentum
        let breakout_long =
            curr.close > range_high && Self::is_bullish(curr) && Self::is_strong_candle(curr);
        if breakout_long {
            let mut state = self.state.lock().unwrap();
            state.last_trade_bar = current_bar;
            orders.push(OrderRequest::market_buy(ctx.symbol.clone(), 1.0));
            return orders;
        }

        // BREAKOUT SHORT: Price breaks below range low with momentum
        let breakout_short =
            curr.close < range_low && Self::is_bearish(curr) && Self::is_strong_candle(curr);
        if breakout_short {
            let mut state = self.state.lock().unwrap();
            state.last_trade_bar = current_bar;
            orders.push(OrderRequest::market_sell(ctx.symbol.clone(), 1.0));
            return orders;
        }

        // REVERSAL LONG: Price near/below range low, bullish candle
        let touch_threshold = range_size * 0.30;
        let near_low = curr.close <= range_low + touch_threshold || curr.low <= range_low;
        if near_low && Self::is_bullish_pattern(prev, curr) {
            let mut state = self.state.lock().unwrap();
            state.last_trade_bar = current_bar;
            orders.push(OrderRequest::market_buy(ctx.symbol.clone(), 1.0));
            return orders;
        }

        // REVERSAL SHORT: Price near/above range high, bearish candle
        let near_high = curr.close >= range_high - touch_threshold || curr.high >= range_high;
        if near_high && Self::is_bearish_pattern(prev, curr) {
            let mut state = self.state.lock().unwrap();
            state.last_trade_bar = current_bar;
            orders.push(OrderRequest::market_sell(ctx.symbol.clone(), 1.0));
        }

        orders
    }

    fn calculate_stop_loss(&self, candles: &[Candle], _entry_price: f64) -> f64 {
        let state = self.state.lock().unwrap();
        let atr_val = Self::compute_atr(candles, self.config.atr_period).unwrap_or(1.0);
        state.range_low - atr_val * 0.5
    }

    fn calculate_take_profit(&self, _candles: &[Candle], entry_price: f64) -> f64 {
        let state = self.state.lock().unwrap();
        let range_mid = (state.range_high + state.range_low) / 2.0;

        if self.config.conservative_target {
            entry_price + (range_mid - entry_price) * 0.5
        } else {
            state.range_high
        }
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        _candles: &[Candle],
    ) -> Option<f64> {
        let state = self.state.lock().unwrap();
        let range_high = state.range_high;
        let range_low = state.range_low;
        let mid = (range_high + range_low) / 2.0;

        // Move to breakeven when price reaches mid-point or re-enters range
        match position.side {
            Side::Buy => {
                if current_price >= mid {
                    Some(position.average_entry_price)
                } else {
                    None
                }
            }
            Side::Sell => {
                if current_price <= mid {
                    Some(position.average_entry_price)
                } else {
                    None
                }
            }
        }
    }

    fn on_bar(&mut self, _ctx: &StrategyContext) {
        // No per-bar counter decrement needed as it uses absolute candle length
    }

    fn init(&mut self) {
        *self.state.lock().unwrap() = State::default();
    }
}
