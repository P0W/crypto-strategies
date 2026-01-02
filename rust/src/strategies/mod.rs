//! Trading Strategies Module
//!
//! Production-ready strategy framework with:
//! - Clean trait interface that all strategies must implement
//! - Dynamic strategy registry (no hardcoded names)
//! - Automatic strategy discovery via registration

pub mod mean_reversion;
pub mod momentum_scalper;
pub mod quick_flip;
pub mod range_breakout;
pub mod volatility_regime;
pub mod vwap_scalper;

use crate::{Candle, Config, Order, Position, Signal, Symbol, Trade};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

// =============================================================================
// Strategy Trait - The contract all strategies must implement
// =============================================================================

/// Trading strategy trait - defines the mandatory interface for all strategies.
///
/// Every strategy must implement these core methods. Default implementations
/// are provided for optional callbacks.
pub trait Strategy: Send + Sync {
    /// Strategy identifier (must match config's strategy_name)
    fn name(&self) -> &'static str;

    /// Declare required timeframes (default: empty = use primary only)
    /// Return vector of timeframes this strategy needs (e.g., vec!["1d", "15m"])
    /// Empty vector means single-timeframe strategy (backward compatible)
    fn required_timeframes(&self) -> Vec<&'static str> {
        vec![]
    }

    /// Generate trading signal for the given candle data
    ///
    /// **DEPRECATED in multi-timeframe mode** - Override `generate_signal_mtf` instead
    /// This is called for single-timeframe strategies (when required_timeframes() is empty)
    fn generate_signal(
        &self,
        symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal;

    /// Generate trading signal using multi-timeframe data
    ///
    /// Override this for multi-timeframe strategies. Default implementation
    /// calls `generate_signal()` with primary timeframe for backward compatibility.
    fn generate_signal_mtf(
        &self,
        symbol: &Symbol,
        mtf_candles: &crate::MultiTimeframeCandles,
        position: Option<&Position>,
    ) -> Signal {
        // Backward compatibility: call single-TF generate_signal with primary timeframe
        self.generate_signal(symbol, mtf_candles.primary(), position)
    }

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

    /// Get regime score for position sizing (default: 1.0)
    fn get_regime_score(&self, _candles: &[Candle]) -> f64 {
        1.0
    }

    /// Notification when an order state changes
    fn notify_order(&mut self, order: &Order) {
        match order.status {
            crate::OrderStatus::Completed => {
                if let Some(ref exec) = order.executed {
                    tracing::debug!(
                        symbol = %order.symbol,
                        side = ?order.side,
                        price = exec.price,
                        size = exec.size,
                        "Order executed"
                    );
                }
            }
            crate::OrderStatus::Canceled
            | crate::OrderStatus::Margin
            | crate::OrderStatus::Rejected => {
                tracing::warn!(symbol = %order.symbol, status = ?order.status, "Order failed");
            }
            _ => {}
        }
    }

    /// Notification when a trade is closed
    fn notify_trade(&mut self, trade: &Trade) {
        tracing::debug!(
            symbol = %trade.symbol,
            pnl = trade.net_pnl,
            return_pct = format!("{:.2}%", trade.return_pct()),
            "Trade closed"
        );
    }

    /// Initialize strategy (called once before trading starts)
    fn init(&mut self) {}
}

// =============================================================================
// Strategy Factory - Type alias for strategy constructor functions
// =============================================================================

/// Factory function type for creating strategies from config
pub type StrategyFactory = fn(&Config) -> Result<Box<dyn Strategy>>;

// =============================================================================
// Strategy Registry - Dynamic registration without hardcoding
// =============================================================================

/// Global strategy registry
static REGISTRY: OnceLock<RwLock<HashMap<&'static str, StrategyFactory>>> = OnceLock::new();

fn get_registry() -> &'static RwLock<HashMap<&'static str, StrategyFactory>> {
    REGISTRY.get_or_init(|| {
        let mut map = HashMap::new();
        map.insert(
            "volatility_regime",
            volatility_regime::create as StrategyFactory,
        );
        map.insert("mean_reversion", mean_reversion::create as StrategyFactory);
        map.insert(
            "momentum_scalper",
            momentum_scalper::create as StrategyFactory,
        );
        map.insert("range_breakout", range_breakout::create as StrategyFactory);
        map.insert("vwap_scalper", vwap_scalper::create as StrategyFactory);
        map.insert("quick_flip", quick_flip::create as StrategyFactory);
        RwLock::new(map)
    })
}

/// Create a strategy from configuration
pub fn create_strategy(config: &Config) -> Result<Box<dyn Strategy>> {
    let registry = get_registry().read().unwrap();

    let strategy_name = config.strategy_name();
    let factory = registry.get(strategy_name.as_str()).ok_or_else(|| {
        let available: Vec<_> = registry.keys().copied().collect();
        anyhow::anyhow!(
            "Unknown strategy: '{}'. Available: {}",
            strategy_name,
            available.join(", ")
        )
    })?;

    factory(config)
}

/// Get list of available strategy names
pub fn available_strategies() -> Vec<&'static str> {
    get_registry().read().unwrap().keys().copied().collect()
}

/// Register a new strategy (for plugins or testing)
pub fn register_strategy(name: &'static str, factory: StrategyFactory) {
    get_registry().write().unwrap().insert(name, factory);
}
