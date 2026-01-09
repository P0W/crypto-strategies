//! Trading Strategies Module
//!
//! Production-ready strategy framework with OMS (Order Management System) support.
//! Strategies generate orders via `generate_orders()` instead of signals.
//!
//! ## Available Strategies
//! - volatility_regime: Volatility regime-based trading
//! - momentum_scalper: Fast momentum scalping
//! - quick_flip: Quick reversal trading
//! - range_breakout: Range breakout strategy
//! - regime_grid: Grid trading with regime detection

pub mod momentum_scalper;
pub mod quick_flip;
pub mod range_breakout;
pub mod regime_grid;
pub mod volatility_regime;

use crate::oms::{Fill, Order, OrderRequest, Position, StrategyContext};
use crate::{Candle, Config, Side, Trade};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

// =============================================================================
// Strategy Trait - OMS-based interface
// =============================================================================

/// Trading strategy trait - defines the mandatory interface for all strategies.
///
/// This is the new OMS-based interface where strategies generate orders
/// instead of signals.
///
/// # Per-Symbol Isolation
///
/// Professional trading systems require **per-symbol strategy instances** to ensure
/// indicator state is isolated across instruments. The `clone_boxed()` method enables
/// the backtester/live engine to create independent strategy instances per symbol.
///
/// This architecture provides:
/// - **State Isolation**: No risk of indicator pollution across symbols
/// - **Clean Lifecycle**: Each symbol can be started/stopped independently
/// - **Natural Parallelization**: Strategies can run on separate threads
/// - **Live Trading Ready**: Same architecture for backtest and production
pub trait Strategy: Send + Sync {
    /// Strategy identifier (must match config's strategy_name)
    fn name(&self) -> &'static str;

    /// Create a fresh clone of this strategy instance.
    ///
    /// This is critical for per-symbol isolation in multi-symbol trading.
    /// Each symbol should have its own strategy instance with independent
    /// indicator state.
    ///
    /// # Professional Trading Pattern
    /// ```ignore
    /// // Backtester creates per-symbol instances:
    /// for symbol in symbols {
    ///     let strategy_instance = template_strategy.clone_boxed();
    ///     symbol_strategies.insert(symbol, strategy_instance);
    /// }
    /// ```
    fn clone_boxed(&self) -> Box<dyn Strategy>;

    /// Declare required timeframes (default: empty = use primary only)
    /// Return vector of timeframes this strategy needs (e.g., vec!["1d", "15m"])
    /// Empty vector means single-timeframe strategy
    fn required_timeframes(&self) -> Vec<&'static str> {
        vec![]
    }

    /// Generate orders based on current market context
    ///
    /// This is the primary interface for OMS-based strategies.
    /// Returns a vector of order requests to be validated and executed.
    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest>;

    /// Calculate stop loss price for entry
    /// For Buy positions: stop is below entry (sell to cut loss)
    /// For Sell positions: stop is above entry (buy to cut loss)
    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64;

    /// Calculate take profit price for entry
    /// For Buy positions: target is above entry (sell for profit)
    /// For Sell positions: target is below entry (buy for profit)
    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64;

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

    /// Notification when an order is filled
    fn on_order_filled(&mut self, _fill: &Fill, _position: &Position) {
        // Default: no-op
    }

    /// Notification when an order is cancelled
    fn on_order_cancelled(&mut self, _order: &Order) {
        // Default: no-op
    }

    /// Notification when a complete trade cycle closes (position fully exited)
    fn on_trade_closed(&mut self, _trade: &Trade) {
        // Default: no-op
    }

    /// Notification when a new bar/candle is processed
    fn on_bar(&mut self, _ctx: &StrategyContext) {
        // Default: no-op
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
        map.insert(
            "momentum_scalper",
            momentum_scalper::create as StrategyFactory,
        );
        map.insert("range_breakout", range_breakout::create as StrategyFactory);
        map.insert("quick_flip", quick_flip::create as StrategyFactory);
        map.insert("regime_grid", regime_grid::create as StrategyFactory);
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
