//! Trading Strategies Module
//!
//! Contains all available trading strategies and common abstractions.
//!
//! ## Available Strategies
//!
//! - `volatility_regime`: Volatility Regime Adaptive Strategy - trades breakouts in
//!   compression/normal volatility regimes with trend confirmation
//! - `mean_reversion`: Mean Reversion Scalper - professional-grade mean reversion
//!   strategy for short timeframes (5m, 15m, 1h) using Bollinger Bands, RSI, and volume

pub mod mean_reversion;
pub mod momentum_scalper;
pub mod range_breakout;
pub mod volatility_regime;

use crate::{Candle, Config, Order, Position, Signal, Symbol, Trade};
use anyhow::Result;
use std::collections::HashMap;

// ============================================================================
// Strategy Factory - Creates strategies from config
// ============================================================================

/// Create a strategy from configuration
pub fn create_strategy(config: &Config) -> Result<Box<dyn Strategy>> {
    match config.strategy_name.as_str() {
        "volatility_regime" => Ok(Box::new(volatility_regime::create_strategy_from_config(config)?)),
        "mean_reversion" => Ok(Box::new(mean_reversion::create_strategy_from_config(config)?)),
        "momentum_scalper" => Ok(Box::new(momentum_scalper::create_strategy_from_config(config)?)),
        "range_breakout" => Ok(Box::new(range_breakout::create_strategy_from_config(config)?)),
        other => anyhow::bail!(
            "Unknown strategy: {}. Available: volatility_regime, mean_reversion, momentum_scalper, range_breakout",
            other
        ),
    }
}

// ============================================================================
// Grid Parameter Generation - Strategy-agnostic optimization
// ============================================================================

/// Generate grid search configs for any strategy
pub fn generate_grid_configs(config: &Config, mode: &str) -> Vec<Config> {
    match config.strategy_name.as_str() {
        "volatility_regime" => {
            let grid = match mode {
                "full" => volatility_regime::GridParams::full(),
                _ => volatility_regime::GridParams::quick(),
            };
            grid.generate_configs(config)
        }
        "mean_reversion" => {
            let grid = match mode {
                "full" => mean_reversion::GridParams::full(),
                _ => mean_reversion::GridParams::quick(),
            };
            grid.generate_configs(config)
        }
        "momentum_scalper" => {
            let grid = match mode {
                "full" => momentum_scalper::GridParams::full(),
                _ => momentum_scalper::GridParams::quick(),
            };
            grid.generate_configs(config)
        }
        "range_breakout" => {
            let grid = match mode {
                "full" => range_breakout::GridParams::full(),
                _ => range_breakout::GridParams::quick(),
            };
            grid.generate_configs(config)
        }
        _ => vec![config.clone()], // Unknown strategy: return base config
    }
}

/// Get total grid combinations for any strategy
pub fn get_grid_combinations(strategy_name: &str, mode: &str) -> usize {
    match strategy_name {
        "volatility_regime" => {
            let grid = match mode {
                "full" => volatility_regime::GridParams::full(),
                _ => volatility_regime::GridParams::quick(),
            };
            grid.total_combinations()
        }
        "mean_reversion" => {
            let grid = match mode {
                "full" => mean_reversion::GridParams::full(),
                _ => mean_reversion::GridParams::quick(),
            };
            grid.total_combinations()
        }
        "momentum_scalper" => {
            let grid = match mode {
                "full" => momentum_scalper::GridParams::full(),
                _ => momentum_scalper::GridParams::quick(),
            };
            grid.total_combinations()
        }
        "range_breakout" => {
            let grid = match mode {
                "full" => range_breakout::GridParams::full(),
                _ => range_breakout::GridParams::quick(),
            };
            grid.total_combinations()
        }
        _ => 1, // Unknown strategy: 1 combination (base config only)
    }
}

/// Extract parameters from config for reporting
pub fn extract_params(config: &Config) -> HashMap<String, f64> {
    match config.strategy_name.as_str() {
        "volatility_regime" => {
            if let Ok(vr_config) = serde_json::from_value::<volatility_regime::VolatilityRegimeConfig>(
                config.strategy.clone(),
            ) {
                volatility_regime::config_to_params(&vr_config)
            } else {
                HashMap::new()
            }
        }
        "mean_reversion" => {
            if let Ok(mr_config) = serde_json::from_value::<mean_reversion::MeanReversionConfig>(
                config.strategy.clone(),
            ) {
                mean_reversion::config_to_params(&mr_config)
            } else {
                HashMap::new()
            }
        }
        "momentum_scalper" => {
            if let Ok(ms_config) = serde_json::from_value::<momentum_scalper::MomentumScalperConfig>(
                config.strategy.clone(),
            ) {
                momentum_scalper::config_to_params(&ms_config)
            } else {
                HashMap::new()
            }
        }
        "range_breakout" => {
            if let Ok(rb_config) = serde_json::from_value::<range_breakout::RangeBreakoutConfig>(
                config.strategy.clone(),
            ) {
                range_breakout::config_to_params(&rb_config)
            } else {
                HashMap::new()
            }
        }
        _ => HashMap::new(),
    }
}

/// Format parameters for display based on strategy
/// Dispatches to strategy-specific format_params in utils.rs
pub fn format_params(params: &HashMap<String, f64>, strategy_name: &str) -> String {
    match strategy_name {
        "mean_reversion" => mean_reversion::format_params(params),
        "momentum_scalper" => momentum_scalper::format_params(params),
        "range_breakout" => range_breakout::format_params(params),
        _ => volatility_regime::format_params(params),
    }
}

/// Trading strategy trait
///
/// This trait defines the interface for trading strategies, matching the backtrader interface
/// for compatibility and providing hooks for order and trade notifications.
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

    /// Get regime score for position sizing (default: 1.0)
    ///
    /// Returns a multiplier for position sizing based on market regime:
    /// - Compression: 1.5 (higher conviction for breakouts)
    /// - Normal: 1.0 (standard sizing)
    /// - Expansion: 0.8 (reduced size)
    /// - Extreme: 0.5 (minimal exposure)
    fn get_regime_score(&self, _candles: &[Candle]) -> f64 {
        1.0 // Default implementation returns 1.0
    }

    /// Notification when an order state changes
    ///
    /// Called when:
    /// - Order is submitted
    /// - Order is accepted by exchange
    /// - Order is partially filled
    /// - Order is completed
    /// - Order is canceled, rejected, or failed
    fn notify_order(&mut self, order: &Order) {
        // Default implementation: log order status
        match order.status {
            crate::OrderStatus::Submitted | crate::OrderStatus::Accepted => {
                // Order in flight, no action needed by default
            }
            crate::OrderStatus::Completed => {
                if let Some(ref exec) = order.executed {
                    tracing::info!(
                        symbol = %order.symbol,
                        side = ?order.side,
                        price = exec.price,
                        size = exec.size,
                        commission = exec.commission,
                        "Order executed"
                    );
                }
            }
            crate::OrderStatus::Canceled
            | crate::OrderStatus::Margin
            | crate::OrderStatus::Rejected => {
                tracing::warn!(
                    symbol = %order.symbol,
                    status = ?order.status,
                    "Order failed"
                );
            }
            _ => {}
        }
    }

    /// Notification when a trade (position) is closed
    ///
    /// Called with the completed trade details including P&L, commission, etc.
    /// Strategies can use this to track performance, adjust parameters, or log results.
    fn notify_trade(&mut self, trade: &Trade) {
        // Default implementation: log trade results
        let return_pct = trade.return_pct();
        let net_pnl_post_tax = if trade.net_pnl > 0.0 {
            trade.net_pnl * 0.7 // 30% tax on profits
        } else {
            trade.net_pnl
        };

        tracing::info!(
            symbol = %trade.symbol,
            entry_price = trade.entry_price,
            exit_price = trade.exit_price,
            quantity = trade.quantity,
            return_pct = format!("{:.2}%", return_pct),
            gross_pnl = trade.pnl,
            commission = trade.commission,
            net_pnl = trade.net_pnl,
            net_pnl_post_tax = format!("{:.2}", net_pnl_post_tax),
            "Trade closed"
        );
    }

    /// Initialize strategy (called once before trading starts)
    ///
    /// Use this to set up any internal state, load models, etc.
    fn init(&mut self) {
        // Default: no initialization needed
    }
}
