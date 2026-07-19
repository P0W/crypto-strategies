//! Common utilities shared across all trading strategies
//!
//! This module provides reusable building blocks for strategy implementations:
//! - OHLC vector extraction from candles
//! - ATR calculation helpers
//! - ATR-based stop loss and take profit calculations
//! - Cooldown counter management for per-symbol state

use crate::indicators::atr;
use crate::oms::{OrderRequest, Position};
use crate::{Candle, Side, Symbol};
use std::collections::HashMap;

/// OHLC vectors extracted from candles for indicator calculations
pub struct OhlcVectors {
    pub high: Vec<f64>,
    pub low: Vec<f64>,
    pub close: Vec<f64>,
}

impl OhlcVectors {
    /// Extract OHLC vectors from candle data
    pub fn from_candles(candles: &[Candle]) -> Self {
        Self {
            high: candles.iter().map(|c| c.high).collect(),
            low: candles.iter().map(|c| c.low).collect(),
            close: candles.iter().map(|c| c.close).collect(),
        }
    }
}

/// Calculate current ATR value from candles
///
/// Returns `None` if there's insufficient data for the ATR calculation.
pub fn current_atr(candles: &[Candle], period: usize) -> Option<f64> {
    let ohlc = OhlcVectors::from_candles(candles);
    atr(&ohlc.high, &ohlc.low, &ohlc.close, period)
        .last()
        .and_then(|&x| x)
}

/// Calculate current ATR with a fallback default value
///
/// Uses `entry_price * default_pct` as the fallback if ATR calculation fails.
pub fn current_atr_or_default(
    candles: &[Candle],
    period: usize,
    entry_price: f64,
    default_pct: f64,
) -> f64 {
    current_atr(candles, period).unwrap_or(entry_price * default_pct)
}

/// Calculate ATR-based stop loss price
///
/// For Buy positions: stop is below entry (entry - distance)
/// For Sell positions: stop is above entry (entry + distance)
pub fn atr_stop_loss(entry_price: f64, atr_value: f64, multiplier: f64, side: Side) -> f64 {
    let distance = atr_value * multiplier;
    match side {
        Side::Buy => entry_price - distance,
        Side::Sell => entry_price + distance,
    }
}

/// Calculate ATR-based take profit price
///
/// For Buy positions: target is above entry (entry + distance)
/// For Sell positions: target is below entry (entry - distance)
pub fn atr_take_profit(entry_price: f64, atr_value: f64, multiplier: f64, side: Side) -> f64 {
    let distance = atr_value * multiplier;
    match side {
        Side::Buy => entry_price + distance,
        Side::Sell => entry_price - distance,
    }
}

pub fn volume_ratio_confirmed(candles: &[Candle], period: usize, minimum_ratio: f64) -> bool {
    if period == 0 || candles.len() <= period {
        return false;
    }

    let end = candles.len() - 1;
    let start = end - period;
    let average = candles[start..end]
        .iter()
        .map(|candle| candle.volume)
        .sum::<f64>()
        / period as f64;
    candles[end].volume >= average * minimum_ratio
}

pub fn close_position_order(symbol: &Symbol, position: &Position) -> OrderRequest {
    match position.side {
        Side::Buy => OrderRequest::market_sell(symbol.clone(), position.quantity.to_f64()),
        Side::Sell => OrderRequest::market_buy(symbol.clone(), position.quantity.to_f64()),
    }
}

#[derive(Default, Clone)]
pub struct PositionLifecycleManager {
    states: HashMap<Symbol, PositionLifecycleState>,
}

#[derive(Default, Clone)]
struct PositionLifecycleState {
    bars_in_position: usize,
    cooldown: usize,
}

impl PositionLifecycleManager {
    pub fn on_bar(&mut self, symbol: &Symbol, in_position: bool) {
        let state = self.states.entry(symbol.clone()).or_default();
        if in_position {
            state.bars_in_position += 1;
        } else if state.cooldown > 0 {
            state.cooldown -= 1;
        }
    }

    pub fn bars_in_position(&self, symbol: &Symbol) -> usize {
        self.states
            .get(symbol)
            .map_or(0, |state| state.bars_in_position)
    }

    pub fn is_cooling_down(&self, symbol: &Symbol) -> bool {
        self.states
            .get(symbol)
            .is_some_and(|state| state.cooldown > 0)
    }

    pub fn close_trade(&mut self, symbol: &Symbol, cooldown_bars: usize) {
        let state = self.states.entry(symbol.clone()).or_default();
        state.bars_in_position = 0;
        state.cooldown = cooldown_bars.saturating_add(1);
    }

    pub fn clear(&mut self) {
        self.states.clear();
    }
}

/// Per-symbol cooldown counter manager
///
/// Tracks cooldown periods after trades close to prevent overtrading.
#[derive(Default, Clone)]
pub struct CooldownManager {
    counters: HashMap<Symbol, usize>,
}

impl CooldownManager {
    /// Create a new empty cooldown manager
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the current cooldown value for a symbol (0 if not set)
    pub fn get(&self, symbol: &Symbol) -> usize {
        *self.counters.get(symbol).unwrap_or(&0)
    }

    /// Check if cooldown is active for a symbol
    pub fn is_active(&self, symbol: &Symbol) -> bool {
        self.get(symbol) > 0
    }

    /// Set the cooldown value for a symbol
    pub fn set(&mut self, symbol: Symbol, value: usize) {
        self.counters.insert(symbol, value);
    }

    /// Decrement the cooldown counter for a symbol (stops at 0)
    pub fn decrement(&mut self, symbol: &Symbol) {
        if let Some(counter) = self.counters.get_mut(symbol) {
            if *counter > 0 {
                *counter -= 1;
            }
        }
    }

    /// Clear all cooldown counters
    pub fn clear(&mut self) {
        self.counters.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn create_test_candle(close: f64) -> Candle {
        Candle::new(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            close - 5.0,
            close + 10.0,
            close - 10.0,
            close,
            1000.0,
        )
        .unwrap()
    }

    #[test]
    fn test_ohlc_vectors_from_candles() {
        let candles = vec![create_test_candle(100.0), create_test_candle(105.0)];
        let ohlc = OhlcVectors::from_candles(&candles);

        assert_eq!(ohlc.close, vec![100.0, 105.0]);
        assert_eq!(ohlc.high, vec![110.0, 115.0]);
        assert_eq!(ohlc.low, vec![90.0, 95.0]);
    }

    #[test]
    fn test_atr_stop_loss() {
        // Buy position: stop below entry
        assert_eq!(atr_stop_loss(100.0, 2.0, 2.0, Side::Buy), 96.0);
        // Sell position: stop above entry
        assert_eq!(atr_stop_loss(100.0, 2.0, 2.0, Side::Sell), 104.0);
    }

    #[test]
    fn test_atr_take_profit() {
        // Buy position: target above entry
        assert_eq!(atr_take_profit(100.0, 2.0, 3.0, Side::Buy), 106.0);
        // Sell position: target below entry
        assert_eq!(atr_take_profit(100.0, 2.0, 3.0, Side::Sell), 94.0);
    }

    #[test]
    fn test_cooldown_manager() {
        let mut mgr = CooldownManager::new();
        let symbol = Symbol::new("BTCINR");

        assert_eq!(mgr.get(&symbol), 0);
        assert!(!mgr.is_active(&symbol));

        mgr.set(symbol.clone(), 3);
        assert_eq!(mgr.get(&symbol), 3);
        assert!(mgr.is_active(&symbol));

        mgr.decrement(&symbol);
        assert_eq!(mgr.get(&symbol), 2);

        mgr.decrement(&symbol);
        mgr.decrement(&symbol);
        assert_eq!(mgr.get(&symbol), 0);
        assert!(!mgr.is_active(&symbol));

        // Decrement at 0 should stay at 0
        mgr.decrement(&symbol);
        assert_eq!(mgr.get(&symbol), 0);
    }

    #[test]
    fn test_cooldown_manager_clear() {
        let mut mgr = CooldownManager::new();
        mgr.set(Symbol::new("BTCINR"), 5);
        mgr.set(Symbol::new("ETHINR"), 3);

        mgr.clear();

        assert_eq!(mgr.get(&Symbol::new("BTCINR")), 0);
        assert_eq!(mgr.get(&Symbol::new("ETHINR")), 0);
    }

    #[test]
    fn test_position_lifecycle_manager() {
        let symbol = Symbol::new("BTCINR");
        let mut lifecycle = PositionLifecycleManager::default();

        lifecycle.on_bar(&symbol, true);
        lifecycle.on_bar(&symbol, true);
        assert_eq!(lifecycle.bars_in_position(&symbol), 2);

        lifecycle.close_trade(&symbol, 2);
        assert!(lifecycle.is_cooling_down(&symbol));
        lifecycle.on_bar(&symbol, false);
        lifecycle.on_bar(&symbol, false);
        assert!(lifecycle.is_cooling_down(&symbol));
        lifecycle.on_bar(&symbol, false);
        assert!(!lifecycle.is_cooling_down(&symbol));
    }

    #[test]
    fn test_volume_ratio_confirmed() {
        let mut candles = vec![create_test_candle(100.0); 21];
        for candle in candles.iter_mut().take(20) {
            candle.volume = 100.0;
        }
        candles[20].volume = 120.0;

        assert!(volume_ratio_confirmed(&candles, 20, 1.2));
        assert!(!volume_ratio_confirmed(&candles, 20, 1.21));
    }
}
