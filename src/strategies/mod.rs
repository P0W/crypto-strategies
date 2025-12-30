//! Trading Strategies Module
//!
//! Contains all available trading strategies and common abstractions.

pub mod volatility_regime;

use crate::{Candle, Position, Signal, Symbol};

/// Trading strategy trait
pub trait Strategy: Send + Sync {
    /// Generate trading signal for the given candle data
    fn generate_signal(
        &self,
        symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal;

    /// Calculate stop loss price for entry
    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64;

    /// Calculate take profit price for entry
    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64;

    /// Update trailing stop if applicable
    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64>;
}
